//! Bottom panel: response status, body, headers and test results.

use crate::app::{PakPosApp, RespSection, ResponseState};
use crate::theme;
use crate::widgets;
use egui::{RichText, Ui};
use pakpos_core::http::{format_size, PreparedBody};

impl PakPosApp {
    pub fn response_panel(&mut self, ui: &mut Ui) {
        let p = self.palette;
        let idx = self.active;
        let Some(tab) = self.tabs.get_mut(idx) else { return };
        if tab.request.kind.is_realtime() {
            crate::ws_view::log_view(ui, &mut tab.ws, tab.uid, &p);
            return;
        }
        if tab.request.kind == pakpos_core::model::RequestKind::Grpc && tab.grpc.live_view {
            crate::ws_view::log_view(ui, &mut tab.grpc.live, tab.uid, &p);
            return;
        }
        let mut copy: Option<String> = None;
        let mut cancel = false;
        let mut save_to_file: Option<Vec<u8>> = None;
        let mut save_example = false;
        let mut open_html: Option<Vec<u8>> = None;
        let mut save_name = String::from("response.txt");

        match &tab.response {
            ResponseState::Empty => {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(crate::i18n::tr("Response")).strong().color(p.text_weak));
                });
                widgets::empty_state(
                    ui,
                    &p,
                    crate::i18n::tr("Ready when you are"),
                    crate::i18n::tr(crate::i18n::tr(
                        "Enter a URL and press Send (Ctrl+Enter). Paste a cURL command into the URL bar to import it.",
                    )),
                );
            }
            ResponseState::Loading { started, .. } => {
                let elapsed = started.elapsed().as_millis();
                ui.vertical_centered(|ui| {
                    ui.add_space(ui.available_height() * 0.3);
                    ui.add(egui::Spinner::new().size(28.0).color(theme::ORANGE));
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new(crate::i18n::trf("Sending request… {ms} ms", &[("ms", &elapsed.to_string())]))
                            .color(p.text_weak),
                    );
                    if ui.button(crate::i18n::tr("Cancel")).clicked() {
                        cancel = true;
                    }
                });
            }
            ResponseState::Error(e) => {
                ui.label(RichText::new(crate::i18n::tr("Could not send request")).strong().color(theme::RED));
                ui.add_space(6.0);
                egui::Frame::new().fill(theme::RED.gamma_multiply(0.12)).corner_radius(8).inner_margin(12).show(
                    ui,
                    |ui| {
                        ui.set_width(ui.available_width());
                        ui.label(RichText::new(e).monospace().color(p.text));
                    },
                );
                ui.add_space(8.0);
                ui.label(
                    RichText::new(crate::i18n::tr("Check the URL, your network/proxy settings, or disable TLS verification in Settings for self-signed certificates."))
                        .small()
                        .color(p.text_weak),
                );
            }
            ResponseState::Done(outcome) => {
                let resp = &outcome.response;
                let passed = outcome.tests.iter().filter(|t| t.passed).count();
                ui.horizontal(|ui| {
                    ui.label(RichText::new(crate::i18n::tr("Response")).strong().color(p.text_weak));
                    ui.add_space(10.0);
                    if resp.http_version == "gRPC" {
                        let sc = if resp.status == 0 { theme::GREEN } else { theme::RED };
                        widgets::pill(
                            ui,
                            format!("gRPC {} {}", resp.status, resp.status_text),
                            sc,
                            sc.gamma_multiply(0.16),
                        );
                    } else {
                        let sc = theme::status_color(resp.status);
                        widgets::pill(ui, format!("{} {}", resp.status, resp.status_text), sc, sc.gamma_multiply(0.16));
                    }
                    widgets::pill(ui, format!("{} ms", resp.elapsed_ms), p.text, p.surface_alt);
                    widgets::pill(ui, format_size(resp.size_bytes()), p.text, p.surface_alt);
                    if !outcome.tests.is_empty() {
                        let all = passed == outcome.tests.len();
                        let c = if all { theme::GREEN } else { theme::RED };
                        widgets::pill(
                            ui,
                            format!("{} {passed}/{}", crate::i18n::tr("Tests"), outcome.tests.len()),
                            c,
                            c.gamma_multiply(0.16),
                        );
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if resp.http_version != "gRPC"
                            && ui
                                .add_enabled(tab.source.is_some(), egui::Button::new(crate::i18n::tr("+ Example")))
                                .on_hover_text(crate::i18n::tr(crate::i18n::tr(
                                    "Save as an example of this request (used by the mock server)",
                                )))
                                .on_disabled_hover_text(crate::i18n::tr("Save the request to a collection first"))
                                .clicked()
                        {
                            save_example = true;
                        }
                        if ui
                            .button(crate::i18n::tr("⬇ Save"))
                            .on_hover_text(crate::i18n::tr("Save response body to a file"))
                            .clicked()
                        {
                            save_to_file = Some(resp.body.clone());
                            save_name = file_name_for(resp);
                        }
                        if ui
                            .button(crate::i18n::tr("📋 Copy"))
                            .on_hover_text(crate::i18n::tr("Copy response body"))
                            .clicked()
                        {
                            copy = Some(outcome.pretty.clone());
                        }
                    });
                });
                ui.add_space(6.0);
                let items = [
                    (RespSection::Body, crate::i18n::tr("Body").to_string()),
                    (RespSection::Headers, widgets::counted("Headers", resp.headers.len())),
                    (
                        RespSection::Tests,
                        if outcome.tests.is_empty() {
                            crate::i18n::tr("Tests").into()
                        } else {
                            format!("{} ({passed}/{})", crate::i18n::tr("Tests"), outcome.tests.len())
                        },
                    ),
                    (RespSection::Console, widgets::counted("Console", outcome.logs.len())),
                    (RespSection::Request, crate::i18n::tr("Request").to_string()),
                ];
                widgets::tab_bar(ui, &mut tab.resp_section, &items);

                match tab.resp_section {
                    RespSection::Body => {
                        use crate::body_view::{self, BodyKind, BodyView};
                        let kind = body_view::kind_of(resp);
                        let views = body_view::views_for(kind);
                        if !views.is_empty() && !views.iter().any(|(v, _)| *v == tab.body_view) {
                            tab.body_view = views[0].0;
                        }
                        if views.is_empty() {
                            tab.body_view = BodyView::Pretty;
                        }
                        let image = kind == BodyKind::Image && tab.body_view == BodyView::Preview;
                        let focus_search = ui.input_mut(|i| {
                            i.consume_shortcut(&egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::F))
                        });
                        let text: String = match tab.body_view {
                            BodyView::Pretty if kind == BodyKind::Json => outcome.pretty.clone(),
                            _ => resp_text(resp),
                        };
                        let matches = if tab.body_view == BodyView::Tree || image {
                            Vec::new()
                        } else {
                            body_view::find_matches(&text, tab.body_search.trim())
                        };
                        ui.horizontal(|ui| {
                            for (v, label) in views {
                                ui.selectable_value(&mut tab.body_view, *v, crate::i18n::tr(label));
                            }
                            if kind == BodyKind::Html
                                && tab.body_view == BodyView::Preview
                                && ui
                                    .button(crate::i18n::tr("Open in browser"))
                                    .on_hover_text(crate::i18n::tr("Write the page to a temporary file and open it"))
                                    .clicked()
                            {
                                open_html = Some(resp.body.clone());
                            }
                            if !image {
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    let r = ui.add(
                                        egui::TextEdit::singleline(&mut tab.body_search)
                                            .hint_text(crate::i18n::tr("Search (Ctrl+F)"))
                                            .min_size(egui::vec2(0.0, theme::CONTROL_HEIGHT))
                                            .vertical_align(egui::Align::Center)
                                            .desired_width(200.0),
                                    );
                                    if focus_search {
                                        r.request_focus();
                                    }
                                    if !tab.body_search.trim().is_empty() && tab.body_view != BodyView::Tree {
                                        let n = matches.len();
                                        let c = if n == 0 { theme::RED } else { p.text_weak };
                                        ui.label(
                                            RichText::new(crate::i18n::trf(
                                                if n == 1 { "{n} match" } else { "{n} matches" },
                                                &[("n", &n.to_string())],
                                            ))
                                            .small()
                                            .color(c),
                                        );
                                    }
                                });
                            }
                        });
                        egui::ScrollArea::both().id_salt(("resp_body", tab.uid)).auto_shrink([false, false]).show(
                            ui,
                            |ui| match (kind, tab.body_view) {
                                (BodyKind::Image, BodyView::Preview) => {
                                    let uri = format!("bytes://response-{}-{}.img", tab.uid, resp.elapsed_ms);
                                    ui.add(
                                        egui::Image::from_bytes(uri, resp.body.clone())
                                            .max_size(ui.available_size())
                                            .fit_to_original_size(1.0),
                                    );
                                }
                                (BodyKind::Html, BodyView::Preview) => {
                                    let job = body_view::html_preview_job(&text, &p, ui.available_width());
                                    egui::Frame::new().fill(p.surface).corner_radius(8).inner_margin(14).show(ui, |ui| {
                                        ui.set_width(ui.available_width());
                                        ui.label(job);
                                    });
                                    ui.label(
                                        RichText::new(crate::i18n::tr("Simplified preview: text, headings, lists and links. Styles, images and scripts are not rendered."))
                                            .small()
                                            .color(p.text_weak),
                                    );
                                }
                                (BodyKind::Json, BodyView::Tree) => match &outcome.json {
                                    Some(v) => {
                                        if let Some(body_view::TreeAction::Copy(t)) =
                                            body_view::json_tree(ui, v, tab.body_search.trim(), tab.uid, &p)
                                        {
                                            copy = Some(t);
                                        }
                                    }
                                    None => {
                                        ui.label(RichText::new(crate::i18n::tr("The body is too large for the tree view.")).color(p.text_weak));
                                    }
                                },
                                _ => {
                                    let lang = guess_lang(resp.header("content-type").unwrap_or(""), kind == BodyKind::Json);
                                    let mut view = text.as_str();
                                    let highlight = text.len() < 300_000;
                                    let theme = egui_extras::syntax_highlighting::CodeTheme::from_style(ui.style());
                                    let search_bg = theme::ORANGE.gamma_multiply(0.35);
                                    let mut layouter = |ui: &Ui, buf: &dyn egui::TextBuffer, wrap_width: f32| {
                                        let mut job = if highlight && lang != "txt" {
                                            egui_extras::syntax_highlighting::highlight(ui.ctx(), ui.style(), &theme, buf.as_str(), lang)
                                        } else {
                                            egui::text::LayoutJob::simple(
                                                buf.as_str().to_string(),
                                                egui::TextStyle::Monospace.resolve(ui.style()),
                                                ui.visuals().text_color(),
                                                wrap_width,
                                            )
                                        };
                                        body_view::highlight_ranges(&mut job, &matches, search_bg, None);
                                        job.wrap.max_width = wrap_width;
                                        ui.ctx().fonts_mut(|f| f.layout_job(job))
                                    };
                                    crate::widgets::code_view(ui, &mut view, |e| e
                                            .desired_width(f32::INFINITY)
                                            .layouter(&mut layouter));
                                }
                            },
                        );
                    }
                    RespSection::Headers => {
                        egui::ScrollArea::vertical()
                            .id_salt(("resp_headers", tab.uid))
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                egui::Grid::new("resp_headers_grid")
                                    .striped(true)
                                    .num_columns(2)
                                    .spacing([24.0, 6.0])
                                    .show(ui, |ui| {
                                        for (k, v) in &resp.headers {
                                            ui.label(RichText::new(k).strong().color(p.text));
                                            ui.add(egui::Label::new(RichText::new(v).monospace()).wrap());
                                            ui.end_row();
                                        }
                                    });
                            });
                    }
                    RespSection::Tests => {
                        if outcome.tests.is_empty() && outcome.extracted.is_empty() {
                            ui.label(
                                RichText::new(crate::i18n::tr(crate::i18n::tr(
                                    "No tests for this request. Add some in the Tests tab above.",
                                )))
                                .color(p.text_weak),
                            );
                        }
                        egui::ScrollArea::vertical().id_salt(("resp_tests", tab.uid)).auto_shrink([false, false]).show(
                            ui,
                            |ui| {
                                for t in &outcome.tests {
                                    ui.horizontal(|ui| {
                                        let (mark, c) =
                                            if t.passed { ("PASS", theme::GREEN) } else { ("FAIL", theme::RED) };
                                        widgets::pill(ui, mark, c, c.gamma_multiply(0.16));
                                        ui.label(RichText::new(&t.name).color(p.text));
                                        if !t.passed {
                                            ui.label(RichText::new(&t.message).small().color(p.text_weak));
                                        }
                                    });
                                }
                                if !outcome.extracted.is_empty() {
                                    ui.add_space(10.0);
                                    widgets::section_title(ui, crate::i18n::tr("Extracted variables"));
                                    for (k, v) in &outcome.extracted {
                                        ui.horizontal(|ui| {
                                            ui.label(
                                                RichText::new(format!("{{{{{k}}}}}")).monospace().color(theme::GREEN),
                                            );
                                            ui.label(RichText::new(truncate(v, 100)).monospace().color(p.text_weak));
                                        });
                                    }
                                }
                            },
                        );
                    }
                    RespSection::Console => crate::scripts_view::console(ui, &outcome.logs, tab.uid, &p),
                    RespSection::Request => {
                        let pr = &outcome.prepared;
                        egui::ScrollArea::vertical().id_salt(("resp_req", tab.uid)).auto_shrink([false, false]).show(
                            ui,
                            |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(&pr.method).strong().monospace());
                                    ui.add(egui::Label::new(RichText::new(&pr.url).monospace()).wrap());
                                });
                                ui.label(
                                    RichText::new(crate::i18n::trf(
                                        "Final URL: {url}  ·  {version}",
                                        &[("url", &resp.url), ("version", &resp.http_version)],
                                    ))
                                    .small()
                                    .color(p.text_weak),
                                );
                                ui.add_space(6.0);
                                widgets::section_title(ui, crate::i18n::tr("Request headers"));
                                egui::Grid::new("sent_headers").striped(true).num_columns(2).spacing([24.0, 4.0]).show(
                                    ui,
                                    |ui| {
                                        for (k, v) in &pr.headers {
                                            ui.label(RichText::new(k).strong());
                                            let shown = if k.eq_ignore_ascii_case("authorization") {
                                                mask(v)
                                            } else {
                                                v.clone()
                                            };
                                            ui.label(RichText::new(shown).monospace());
                                            ui.end_row();
                                        }
                                    },
                                );
                                let body = match &pr.body {
                                    PreparedBody::None => None,
                                    PreparedBody::Text(t) => Some(t.clone()),
                                    PreparedBody::Bytes(b) => Some(format!("<{} bytes>", b.len())),
                                    PreparedBody::Form(f) => {
                                        Some(f.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("&"))
                                    }
                                    PreparedBody::Multipart(f) => Some(
                                        f.iter()
                                            .map(
                                                |(k, v, file)| {
                                                    if *file {
                                                        format!("{k}=@{v}")
                                                    } else {
                                                        format!("{k}={v}")
                                                    }
                                                },
                                            )
                                            .collect::<Vec<_>>()
                                            .join("\n"),
                                    ),
                                };
                                if let Some(b) = body {
                                    ui.add_space(6.0);
                                    widgets::section_title(ui, crate::i18n::tr("Request body"));
                                    ui.label(RichText::new(truncate(&b, 5000)).monospace());
                                }
                            },
                        );
                    }
                }
            }
        }

        if cancel {
            tab.response = ResponseState::Empty;
        }
        if save_example {
            self.save_example(idx);
        }
        if let Some(text) = copy {
            ui.ctx().copy_text(text);
            self.toast(crate::i18n::tr("Copied to clipboard"));
        }
        if let Some(html) = open_html {
            let path = std::env::temp_dir()
                .join(format!("pakpos-preview-{}.html", self.tabs.get(idx).map(|t| t.uid).unwrap_or(0)));
            match std::fs::write(&path, html) {
                Ok(()) => {
                    if webbrowser::open(&format!("file://{}", path.display())).is_err() {
                        self.toast_error(crate::i18n::tr("Could not open a browser"));
                    }
                }
                Err(e) => self.toast_error(e.to_string()),
            }
        }
        if let Some(bytes) = save_to_file {
            if let Some(path) = rfd::FileDialog::new().set_file_name(save_name).save_file() {
                match std::fs::write(&path, bytes) {
                    Ok(()) => self.toast(crate::i18n::trf("Saved {path}", &[("path", &path.display().to_string())])),
                    Err(e) => self.toast_error(e.to_string()),
                }
            }
        }
    }
}

/// `response.json`, `response.png`, … from the content type.
fn file_name_for(resp: &pakpos_core::http::Response) -> String {
    let ct = resp.header("content-type").unwrap_or("").to_ascii_lowercase();
    let ext = [
        ("json", "json"),
        ("html", "html"),
        ("xml", "xml"),
        ("png", "png"),
        ("jpeg", "jpg"),
        ("gif", "gif"),
        ("webp", "webp"),
        ("svg", "svg"),
        ("pdf", "pdf"),
        ("csv", "csv"),
        ("javascript", "js"),
        ("css", "css"),
        ("zip", "zip"),
    ]
    .iter()
    .find(|(k, _)| ct.contains(k))
    .map(|(_, e)| *e)
    .unwrap_or(if resp.is_json() { "json" } else { "txt" });
    format!("response.{ext}")
}

fn resp_text(resp: &pakpos_core::http::Response) -> String {
    resp.text()
}

fn guess_lang(content_type: &str, is_json: bool) -> &'static str {
    let ct = content_type.to_lowercase();
    if is_json {
        "json"
    } else if ct.contains("html") || ct.contains("xml") {
        "html"
    } else if ct.contains("javascript") {
        "js"
    } else if ct.contains("css") {
        "css"
    } else {
        "txt"
    }
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() > n {
        format!("{}…", s.chars().take(n).collect::<String>())
    } else {
        s.to_string()
    }
}

fn mask(v: &str) -> String {
    match v.split_once(' ') {
        Some((scheme, secret)) if secret.chars().count() > 8 => {
            let head: String = secret.chars().take(4).collect();
            let tail: String = secret.chars().rev().take(2).collect::<Vec<_>>().into_iter().rev().collect();
            format!("{scheme} {head}…{tail}")
        }
        _ => v.to_string(),
    }
}
