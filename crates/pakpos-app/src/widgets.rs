//! Small reusable widgets.

use crate::theme::{self, Palette};
use egui::text::{LayoutJob, TextFormat};
use egui::{Color32, CornerRadius, FontId, Margin, RichText, Sense, Stroke, Ui, Vec2};
use pakpos_core::model::{FormField, KeyValue};
use pakpos_core::vars::VarScope;
use std::sync::Arc;

/// Coloured method label, e.g. `GET` in green.
pub fn method_label(ui: &mut Ui, r: &pakpos_core::Request) -> egui::Response {
    let (text, color) = theme::badge(r);
    ui.add(egui::Label::new(RichText::new(text).color(color).strong().monospace().size(11.5)).selectable(false))
}

/// A filled rounded "pill" with text.
pub fn pill(ui: &mut Ui, text: impl Into<String>, fg: Color32, bg: Color32) -> egui::Response {
    egui::Frame::new()
        .fill(bg)
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::symmetric(8, 2))
        .show(ui, |ui| {
            ui.add(egui::Label::new(RichText::new(text.into()).color(fg).strong().size(12.5)).selectable(false))
        })
        .inner
}

/// Primary (orange) button.
pub fn primary_button(ui: &mut Ui, text: &str) -> egui::Response {
    let btn = egui::Button::new(RichText::new(text).color(theme::INK).strong())
        .fill(theme::ORANGE)
        .stroke(Stroke::NONE)
        .corner_radius(CornerRadius::same(6))
        .min_size(Vec2::new(84.0, 32.0));
    let resp = ui.add(btn);
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

/// Small borderless icon-like button.
pub fn ghost_button(ui: &mut Ui, text: &str, tooltip: &str) -> egui::Response {
    ui.add(egui::Button::new(text).frame(false)).on_hover_text(tooltip)
}

/// Section heading in weak colour.
pub fn section_title(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text.to_uppercase()).size(11.0).strong().color(ui.visuals().weak_text_color()));
}

/// Tab-like selector row. Returns true if the selection changed.
pub fn tab_bar<T: PartialEq + Copy>(ui: &mut Ui, current: &mut T, items: &[(T, String)]) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 18.0;
        for (value, label) in items {
            let selected = *current == *value;
            let color = if selected { ui.visuals().text_color() } else { ui.visuals().weak_text_color() };
            let resp = ui.add(
                egui::Label::new(RichText::new(label).color(color).strong()).selectable(false).sense(Sense::click()),
            );
            if resp.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if selected {
                let r = resp.rect;
                ui.painter().line_segment(
                    [egui::pos2(r.left(), r.bottom() + 4.0), egui::pos2(r.right(), r.bottom() + 4.0)],
                    Stroke::new(2.5, theme::ORANGE),
                );
            }
            if resp.clicked() && !selected {
                *current = *value;
                changed = true;
            }
        }
    });
    ui.add_space(6.0);
    changed
}

/// Label with a count badge, e.g. "Headers (3)".
pub fn counted(label: &str, n: usize) -> String {
    if n == 0 {
        label.to_string()
    } else {
        format!("{label} ({n})")
    }
}

/// Builds a layout job that highlights `{{variables}}`: green when the
/// variable resolves, red when it does not.
pub fn highlight_vars(text: &str, scope: &VarScope, font: FontId, base: Color32) -> LayoutJob {
    let mut job = LayoutJob::default();
    let plain = TextFormat { font_id: font.clone(), color: base, ..Default::default() };
    let mut rest = text;
    while let Some(start) = rest.find("{{") {
        job.append(&rest[..start], 0.0, plain.clone());
        let after = &rest[start + 2..];
        match after.find("}}") {
            Some(end) => {
                let name = after[..end].trim();
                let known = scope.get(name).is_some();
                let color = if known { theme::GREEN } else { theme::RED };
                job.append(
                    &rest[start..start + 2 + end + 2],
                    0.0,
                    TextFormat {
                        font_id: font.clone(),
                        color,
                        background: color.gamma_multiply(0.14),
                        ..Default::default()
                    },
                );
                rest = &after[end + 2..];
            }
            None => {
                job.append(&rest[start..], 0.0, plain.clone());
                rest = "";
            }
        }
    }
    job.append(rest, 0.0, plain);
    job
}

/// Single-line text edit with `{{var}}` highlighting.
pub fn var_text_edit(ui: &mut Ui, value: &mut String, scope: &VarScope, hint: &str, width: f32) -> egui::Response {
    let font = egui::TextStyle::Body.resolve(ui.style());
    let base = ui.visuals().text_color();
    let mut layouter = |ui: &Ui, buf: &dyn egui::TextBuffer, wrap_width: f32| -> Arc<egui::Galley> {
        let mut job = highlight_vars(buf.as_str(), scope, font.clone(), base);
        let _ = wrap_width; // single line: never wrap
        job.wrap.max_width = f32::INFINITY;
        ui.ctx().fonts_mut(|f| f.layout_job(job))
    };
    ui.add(egui::TextEdit::singleline(value).hint_text(hint).desired_width(width).layouter(&mut layouter))
}

fn header_row(ui: &mut Ui, cols: &[(&str, f32)]) {
    ui.horizontal(|ui| {
        for (label, w) in cols {
            let color = ui.visuals().weak_text_color();
            ui.allocate_ui_with_layout(egui::vec2(*w, 16.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.set_min_width(*w);
                ui.add_space(4.0);
                ui.label(RichText::new(*label).small().color(color));
            });
        }
    });
}

/// Editable key/value table. Keeps one empty trailing row so users can type
/// a new entry directly.
pub fn kv_table(ui: &mut Ui, id: &str, rows: &mut Vec<KeyValue>, scope: &VarScope, key_hint: &str) -> bool {
    let mut changed = false;
    if rows.last().is_none_or(|r| !r.is_blank()) {
        rows.push(KeyValue { enabled: true, ..Default::default() });
    }
    let spacing = ui.spacing().item_spacing.x;
    let total = ui.available_width() - 2.0 * 22.0 - 3.0 * spacing;
    let key_w = (total * 0.38).clamp(120.0, 420.0);
    let val_w = (total - key_w).max(140.0);
    let mut remove = None;
    let len = rows.len();
    ui.push_id(id, |ui| {
        header_row(ui, &[("", 22.0), ("Key", key_w), ("Value", val_w)]);
        for (i, row) in rows.iter_mut().enumerate() {
            let last = i + 1 == len;
            ui.push_id(i, |ui| {
                ui.horizontal(|ui| {
                    if last {
                        ui.allocate_exact_size(egui::vec2(22.0, 22.0), Sense::hover());
                    } else {
                        changed |= ui.add_sized([22.0, 22.0], egui::Checkbox::without_text(&mut row.enabled)).changed();
                    }
                    changed |=
                        var_text_edit(ui, &mut row.key, scope, if last { key_hint } else { "" }, key_w).changed();
                    changed |=
                        var_text_edit(ui, &mut row.value, scope, if last { "Value" } else { "" }, val_w).changed();
                    if !last && ghost_button(ui, "×", "Remove").clicked() {
                        remove = Some(i);
                    }
                });
            });
        }
    });
    if let Some(i) = remove {
        rows.remove(i);
        changed = true;
    }
    changed
}

/// Editor for multipart fields (text or file).
pub fn form_table(ui: &mut Ui, rows: &mut Vec<FormField>, scope: &VarScope) -> bool {
    let mut changed = false;
    if rows.last().is_none_or(|r| !r.key.is_empty() || !r.value.is_empty()) {
        rows.push(FormField::default());
    }
    let spacing = ui.spacing().item_spacing.x;
    let total = ui.available_width() - 2.0 * 22.0 - 76.0 - 4.0 * spacing;
    let key_w = (total * 0.32).clamp(110.0, 360.0);
    let val_w = (total - key_w).max(140.0);
    let mut remove = None;
    let len = rows.len();
    ui.push_id("multipart", |ui| {
        header_row(ui, &[("", 22.0), ("Key", key_w), ("Type", 70.0), ("Value", val_w)]);
        for (i, row) in rows.iter_mut().enumerate() {
            let last = i + 1 == len;
            ui.push_id(i, |ui| {
                ui.horizontal(|ui| {
                    if last {
                        ui.allocate_exact_size(egui::vec2(22.0, 22.0), Sense::hover());
                    } else {
                        changed |= ui.add_sized([22.0, 22.0], egui::Checkbox::without_text(&mut row.enabled)).changed();
                    }
                    changed |= var_text_edit(ui, &mut row.key, scope, if last { "Key" } else { "" }, key_w).changed();
                    egui::ComboBox::from_id_salt("ftype")
                        .width(64.0)
                        .selected_text(if row.is_file { "File" } else { "Text" })
                        .show_ui(ui, |ui| {
                            changed |= ui.selectable_value(&mut row.is_file, false, "Text").changed();
                            changed |= ui.selectable_value(&mut row.is_file, true, "File").changed();
                        });
                    if row.is_file {
                        let w = (val_w - 80.0).max(80.0);
                        changed |= ui
                            .add(egui::TextEdit::singleline(&mut row.value).hint_text("path/to/file").desired_width(w))
                            .changed();
                        if ui.button("Browse…").clicked() {
                            if let Some(path) = rfd::FileDialog::new().pick_file() {
                                row.value = path.display().to_string();
                                changed = true;
                            }
                        }
                    } else {
                        changed |=
                            var_text_edit(ui, &mut row.value, scope, if last { "Value" } else { "" }, val_w).changed();
                    }
                    if !last && ghost_button(ui, "×", "Remove").clicked() {
                        remove = Some(i);
                    }
                });
            });
        }
    });
    if let Some(i) = remove {
        rows.remove(i);
        changed = true;
    }
    changed
}

/// Removes the trailing blank row added by the editors (before saving).
pub fn trim_blank_rows(rows: &mut Vec<KeyValue>) {
    rows.retain(|r| !r.is_blank());
}

pub fn trim_blank_fields(rows: &mut Vec<FormField>) {
    rows.retain(|r| !r.key.is_empty() || !r.value.is_empty());
}

/// Empty-state block with a title and hint.
pub fn empty_state(ui: &mut Ui, p: &Palette, title: &str, hint: &str) {
    ui.vertical_centered(|ui| {
        ui.add_space(ui.available_height() * 0.25);
        ui.label(RichText::new(title).size(17.0).strong().color(p.text));
        ui.add_space(4.0);
        ui.label(RichText::new(hint).color(p.text_weak));
    });
}
