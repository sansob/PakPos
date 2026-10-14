//! Response body views: highlighted text with search, JSON tree, image and
//! HTML preview.

use crate::theme::{self, Palette};
use egui::text::{LayoutJob, LayoutSection, TextFormat};
use egui::{Color32, RichText, Ui};
use serde_json::Value;

/// How the body is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BodyView {
    #[default]
    Pretty,
    Raw,
    Tree,
    Preview,
}

/// What the body is, from the content type and the bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyKind {
    Json,
    Html,
    Image,
    Text,
}

pub fn kind_of(resp: &pakpos_core::http::Response) -> BodyKind {
    let ct = resp.header("content-type").unwrap_or("").to_ascii_lowercase();
    if ct.starts_with("image/") && !ct.contains("svg") || sniff_image(&resp.body) {
        BodyKind::Image
    } else if resp.is_json() {
        BodyKind::Json
    } else if ct.contains("html") {
        BodyKind::Html
    } else {
        BodyKind::Text
    }
}

fn sniff_image(b: &[u8]) -> bool {
    b.starts_with(b"\x89PNG")
        || b.starts_with(&[0xff, 0xd8, 0xff])
        || b.starts_with(b"GIF8")
        || b.starts_with(b"BM")
        || (b.len() > 12 && &b[..4] == b"RIFF" && &b[8..12] == b"WEBP")
}

/// Views that make sense for a kind, in toolbar order.
pub fn views_for(kind: BodyKind) -> &'static [(BodyView, &'static str)] {
    match kind {
        BodyKind::Json => &[(BodyView::Pretty, "Pretty"), (BodyView::Raw, "Raw"), (BodyView::Tree, "Tree")],
        BodyKind::Html => &[(BodyView::Pretty, "Source"), (BodyView::Preview, "Preview")],
        BodyKind::Image => &[(BodyView::Preview, "Image"), (BodyView::Raw, "Raw")],
        BodyKind::Text => &[],
    }
}

/// Byte ranges of `needle` in `haystack`, ignoring ASCII case.
pub fn find_matches(haystack: &str, needle: &str) -> Vec<(usize, usize)> {
    if needle.is_empty() {
        return Vec::new();
    }
    let hay = haystack.to_ascii_lowercase();
    let needle = needle.to_ascii_lowercase();
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(i) = hay[from..].find(&needle) {
        let start = from + i;
        out.push((start, start + needle.len()));
        from = start + needle.len().max(1);
        if out.len() >= 10_000 {
            break;
        }
    }
    out
}

/// Gives `ranges` a background colour in an already laid out job, splitting
/// sections at the range borders.
pub fn highlight_ranges(job: &mut LayoutJob, ranges: &[(usize, usize)], bg: Color32, current: Option<usize>) {
    if ranges.is_empty() {
        return;
    }
    let mut out: Vec<LayoutSection> = Vec::with_capacity(job.sections.len() + ranges.len() * 2);
    for section in job.sections.drain(..) {
        let (s, e): (usize, usize) = (section.byte_range.start.into(), section.byte_range.end.into());
        let mut cursor = s;
        for (i, &(rs, re)) in ranges.iter().enumerate() {
            if re <= cursor || rs >= e {
                continue;
            }
            let (a, b) = (rs.max(cursor), re.min(e));
            if a > cursor {
                out.push(LayoutSection { byte_range: cursor.into()..a.into(), ..section.clone() });
            }
            let mut format = section.format.clone();
            format.background = if Some(i) == current { theme::ORANGE } else { bg };
            if Some(i) == current {
                format.color = theme::INK;
            }
            out.push(LayoutSection { byte_range: a.into()..b.into(), format, ..section.clone() });
            cursor = b;
        }
        if cursor < e {
            out.push(LayoutSection { byte_range: cursor.into()..e.into(), ..section });
        }
    }
    job.sections = out;
}

/// `json.a.b[0]` style path, the syntax used by tests and extractions.
fn child_path(parent: &str, key: &PathKey) -> String {
    match key {
        PathKey::Index(i) => format!("{parent}[{i}]"),
        PathKey::Key(k) if k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') && !k.is_empty() => {
            format!("{parent}.{k}")
        }
        PathKey::Key(k) => format!("{parent}[\"{}\"]", k.replace('"', "\\\"")),
    }
}

enum PathKey<'a> {
    Key(&'a str),
    Index(usize),
}

/// Does this value (or anything below it) contain the search text?
fn subtree_matches(key: &str, v: &Value, needle: &str) -> bool {
    if key.to_ascii_lowercase().contains(needle) {
        return true;
    }
    match v {
        Value::Object(m) => m.iter().any(|(k, v)| subtree_matches(k, v, needle)),
        Value::Array(a) => a.iter().any(|v| subtree_matches("", v, needle)),
        other => scalar(other).to_ascii_lowercase().contains(needle),
    }
}

fn scalar(v: &Value) -> String {
    match v {
        Value::String(s) => format!("\"{s}\""),
        other => other.to_string(),
    }
}

fn scalar_color(v: &Value, p: &Palette) -> Color32 {
    match v {
        Value::String(_) => theme::GREEN,
        Value::Number(_) => theme::ORANGE,
        Value::Bool(_) => theme::BLUE,
        Value::Null => p.text_weak,
        _ => p.text,
    }
}

/// Things the tree asks the panel to do.
pub enum TreeAction {
    Copy(String),
}

/// Collapsible JSON tree. Right-click copies a value or its test path.
pub fn json_tree(ui: &mut Ui, root: &Value, search: &str, uid: u64, p: &Palette) -> Option<TreeAction> {
    let mut action = None;
    let needle = search.to_ascii_lowercase();
    node(ui, None, root, "json", &needle, uid, p, &mut action, 0);
    action
}

#[allow(clippy::too_many_arguments)]
fn node(
    ui: &mut Ui,
    key: Option<&PathKey>,
    v: &Value,
    path: &str,
    needle: &str,
    uid: u64,
    p: &Palette,
    action: &mut Option<TreeAction>,
    depth: usize,
) {
    let key_text = match key {
        Some(PathKey::Key(k)) => format!("{k}: "),
        Some(PathKey::Index(i)) => format!("{i}: "),
        None => String::new(),
    };
    let key_hit = !needle.is_empty() && key_text.to_ascii_lowercase().contains(needle);
    let key_rich = RichText::new(&key_text).monospace().color(if key_hit { theme::ORANGE } else { p.text });
    let menu = |resp: egui::Response, action: &mut Option<TreeAction>, value: &Value| {
        resp.on_hover_text(path).context_menu(|ui| {
            if ui.button(crate::i18n::tr("Copy value")).clicked() {
                let text = match value {
                    Value::String(s) => s.clone(),
                    other => serde_json::to_string_pretty(other).unwrap_or_default(),
                };
                *action = Some(TreeAction::Copy(text));
                ui.close();
            }
            if ui.button(format!("Copy path  {path}")).clicked() {
                *action = Some(TreeAction::Copy(path.to_string()));
                ui.close();
            }
        });
    };
    match v {
        Value::Object(_) | Value::Array(_) => {
            let (open_char, close_char, len) = match v {
                Value::Object(m) => ('{', '}', m.len()),
                Value::Array(a) => ('[', ']', a.len()),
                _ => unreachable!(),
            };
            let summary = format!("{open_char} {len} {} {close_char}", if len == 1 { "item" } else { "items" });
            let id = ui.make_persistent_id(("json_tree", uid, path));
            let searching = !needle.is_empty();
            let mut state = egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, depth < 2);
            if searching {
                state.set_open(subtree_matches("", v, needle));
            }
            let header = state.show_header(ui, |ui| {
                let r = ui.label(key_rich);
                menu(r, action, v);
                ui.label(RichText::new(summary).monospace().small().color(p.text_weak));
            });
            header.body(|ui| match v {
                Value::Object(m) => {
                    for (k, child) in m {
                        if searching && !subtree_matches(k, child, needle) {
                            continue;
                        }
                        let key = PathKey::Key(k);
                        node(ui, Some(&key), child, &child_path(path, &key), needle, uid, p, action, depth + 1);
                    }
                }
                Value::Array(a) => {
                    for (i, child) in a.iter().enumerate().take(5_000) {
                        if searching && !subtree_matches("", child, needle) {
                            continue;
                        }
                        let key = PathKey::Index(i);
                        node(ui, Some(&key), child, &child_path(path, &key), needle, uid, p, action, depth + 1);
                    }
                }
                _ => {}
            });
        }
        scalar_value => {
            ui.horizontal(|ui| {
                ui.add_space(18.0);
                let r = ui.label(key_rich);
                menu(r, action, scalar_value);
                let text = scalar(scalar_value);
                let hit = !needle.is_empty() && text.to_ascii_lowercase().contains(needle);
                let mut rich = RichText::new(text).monospace().color(scalar_color(scalar_value, p));
                if hit {
                    rich = rich.background_color(theme::ORANGE.gamma_multiply(0.3));
                }
                let r = ui.add(egui::Label::new(rich).wrap());
                menu(r, action, scalar_value);
            });
        }
    }
}

/// A readable rendering of an HTML page: headings, paragraphs, lists, links,
/// bold / italic and code; scripts and styles are dropped. This is not a
/// browser: layout, CSS and images are ignored.
pub fn html_preview_job(html: &str, p: &Palette, wrap: f32) -> LayoutJob {
    let mut job = LayoutJob::default();
    job.wrap.max_width = wrap;
    let base = TextFormat { font_id: egui::FontId::proportional(14.0), color: p.text, ..Default::default() };
    let mut bold = 0usize;
    let mut italic = 0usize;
    let mut code = 0usize;
    let mut link = 0usize;
    let mut heading: Option<f32> = None;
    let mut skip: Option<String> = None;
    let mut pending_space = false;
    let mut at_line_start = true;
    let mut rest = html;

    let newline = |job: &mut LayoutJob, at_line_start: &mut bool, blank: bool| {
        if !*at_line_start {
            job.append("\n", 0.0, base.clone());
        }
        if blank && !job.text.ends_with("\n\n") && !job.text.is_empty() {
            job.append("\n", 0.0, base.clone());
        }
        *at_line_start = true;
    };

    while !rest.is_empty() {
        if let Some(lt) = rest.find('<') {
            let text = &rest[..lt];
            if skip.is_none() && !text.is_empty() {
                let decoded = decode_entities(text);
                let mut words = String::new();
                for (i, w) in decoded.split_whitespace().enumerate() {
                    if i > 0
                        || (pending_space && !at_line_start)
                        || (decoded.starts_with(char::is_whitespace) && !at_line_start)
                    {
                        words.push(' ');
                    }
                    words.push_str(w);
                }
                pending_space = decoded.ends_with(char::is_whitespace);
                if !words.trim().is_empty() {
                    let mut f = base.clone();
                    if let Some(size) = heading {
                        f.font_id = egui::FontId::proportional(size);
                        f.color = p.text;
                    }
                    if bold > 0 || heading.is_some() {
                        f.color = if p.dark { Color32::WHITE } else { theme::INK };
                    }
                    f.italics = italic > 0;
                    if code > 0 {
                        f.font_id = egui::FontId::monospace(13.0);
                        f.background = p.surface_alt;
                    }
                    if link > 0 {
                        f.color = theme::BLUE;
                        f.underline = egui::Stroke::new(1.0, theme::BLUE);
                    }
                    job.append(&words, 0.0, f);
                    at_line_start = false;
                }
            }
            let Some(gt) = rest[lt..].find('>') else { break };
            let tag_src = &rest[lt + 1..lt + gt];
            rest = &rest[lt + gt + 1..];
            if tag_src.starts_with("!--") {
                if let Some(end) = rest.find("-->") {
                    rest = &rest[end + 3..];
                }
                continue;
            }
            let closing = tag_src.starts_with('/');
            let name: String = tag_src
                .trim_start_matches('/')
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric())
                .collect::<String>()
                .to_ascii_lowercase();
            if let Some(s) = &skip {
                if closing && &name == s {
                    skip = None;
                }
                continue;
            }
            match (name.as_str(), closing) {
                ("script" | "style" | "head" | "noscript" | "svg" | "template", false) => skip = Some(name),
                ("br", _) => {
                    job.append("\n", 0.0, base.clone());
                    at_line_start = true;
                }
                ("h1" | "h2" | "h3" | "h4" | "h5" | "h6", false) => {
                    newline(&mut job, &mut at_line_start, true);
                    heading = Some(match name.as_str() {
                        "h1" => 26.0,
                        "h2" => 22.0,
                        "h3" => 19.0,
                        _ => 16.0,
                    });
                }
                ("h1" | "h2" | "h3" | "h4" | "h5" | "h6", true) => {
                    heading = None;
                    newline(&mut job, &mut at_line_start, true);
                }
                (
                    "p" | "div" | "section" | "article" | "header" | "footer" | "main" | "nav" | "table" | "form"
                    | "blockquote" | "pre",
                    _,
                ) => newline(&mut job, &mut at_line_start, name == "p"),
                ("tr", _) => newline(&mut job, &mut at_line_start, false),
                ("td" | "th", false) => {
                    if !at_line_start {
                        job.append("   ", 0.0, base.clone());
                    }
                }
                ("li", false) => {
                    newline(&mut job, &mut at_line_start, false);
                    job.append("  •  ", 0.0, TextFormat { color: theme::ORANGE, ..base.clone() });
                    at_line_start = false;
                    pending_space = false;
                }
                ("ul" | "ol", _) => newline(&mut job, &mut at_line_start, false),
                ("hr", _) => {
                    newline(&mut job, &mut at_line_start, false);
                    job.append("────────────────────────\n", 0.0, TextFormat { color: p.border, ..base.clone() });
                    at_line_start = true;
                }
                ("b" | "strong", c) => bold = if c { bold.saturating_sub(1) } else { bold + 1 },
                ("i" | "em", c) => italic = if c { italic.saturating_sub(1) } else { italic + 1 },
                ("code" | "kbd" | "samp", c) => code = if c { code.saturating_sub(1) } else { code + 1 },
                ("a", c) => link = if c { link.saturating_sub(1) } else { link + 1 },
                _ => {}
            }
        } else {
            if skip.is_none() {
                let text = decode_entities(rest);
                if !text.trim().is_empty() {
                    job.append(text.trim(), 0.0, base.clone());
                }
            }
            break;
        }
    }
    job
}

fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let after = &rest[i..];
        let Some(end) = after.find(';').filter(|e| *e <= 10) else {
            out.push('&');
            rest = &after[1..];
            continue;
        };
        let entity = &after[1..end];
        let decoded = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" | "#39" => Some('\''),
            "nbsp" => Some(' '),
            "copy" => Some('©'),
            "mdash" => Some('—'),
            "ndash" => Some('–'),
            e if e.starts_with("#x") => u32::from_str_radix(&e[2..], 16).ok().and_then(char::from_u32),
            e if e.starts_with('#') => e[1..].parse().ok().and_then(char::from_u32),
            _ => None,
        };
        match decoded {
            Some(c) => {
                out.push(c);
                rest = &after[end + 1..];
            }
            None => {
                out.push('&');
                rest = &after[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_are_case_insensitive() {
        assert_eq!(find_matches("Budi budi BUDI", "budi"), vec![(0, 4), (5, 9), (10, 14)]);
        assert!(find_matches("x", "").is_empty());
    }

    #[test]
    fn highlight_splits_sections() {
        let mut job = LayoutJob::default();
        job.append("hello world", 0.0, TextFormat::default());
        highlight_ranges(&mut job, &[(6, 11)], Color32::RED, None);
        assert_eq!(job.sections.len(), 2);
        assert_eq!(job.sections[1].byte_range, 6.into()..11.into());
        assert_eq!(job.sections[1].format.background, Color32::RED);
    }

    #[test]
    fn paths_use_test_syntax() {
        assert_eq!(child_path("json", &PathKey::Key("data")), "json.data");
        assert_eq!(child_path("json.data", &PathKey::Index(0)), "json.data[0]");
        assert_eq!(child_path("json", &PathKey::Key("a b")), "json[\"a b\"]");
    }

    #[test]
    fn html_preview_text() {
        let p = Palette::of(pakpos_core::storage::Theme::Dark);
        let job = html_preview_job(
            "<html><head><title>x</title><style>p{}</style></head><body><h1>Halo &amp; selamat</h1><p>Satu <b>dua</b></p><ul><li>a</li><li>b</li></ul><script>alert(1)</script></body></html>",
            &p,
            400.0,
        );
        assert!(job.text.contains("Halo & selamat"), "{:?}", job.text);
        assert!(job.text.contains("Satu dua"), "{:?}", job.text);
        assert!(job.text.contains("•  a"));
        assert!(!job.text.contains("alert"));
        assert!(!job.text.contains("p{}"));
    }
}
