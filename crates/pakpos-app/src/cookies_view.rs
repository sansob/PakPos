//! Cookie jar viewer / editor.

use crate::app::PakPosApp;
use crate::dialogs::{modal_frame, title};
use crate::{theme, widgets};
use egui::{Id, RichText};
use pakpos_core::cookies::StoredCookie;

/// State of the cookies dialog.
#[derive(Default)]
pub struct CookiesDialog {
    pub filter: String,
    pub new_domain: String,
    pub new_name: String,
    pub new_value: String,
}

fn expiry_label(c: &StoredCookie) -> String {
    match c.expires {
        None => "session".into(),
        Some(t) => chrono::DateTime::from_timestamp(t, 0)
            .map(|d| d.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M").to_string())
            .unwrap_or_default(),
    }
}

impl PakPosApp {
    /// Returns `false` when the dialog is closed.
    pub fn cookies_dialog(&mut self, ctx: &egui::Context, st: &mut CookiesDialog) -> bool {
        let p = self.palette;
        let mut keep = true;
        let mut changed = false;
        self.sync_cookies();
        let r = egui::Modal::new(Id::new("cookies")).frame(modal_frame(&p)).show(ctx, |ui| {
            ui.set_width(760.0);
            ui.horizontal(|ui| {
                title(ui, "Cookies");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if !self.ws.cookies.cookies.is_empty() && ui.button("Clear all").clicked() {
                        self.ws.cookies.cookies.clear();
                        changed = true;
                    }
                    ui.add(
                        egui::TextEdit::singleline(&mut st.filter)
                            .hint_text("Filter by domain or name")
                            .min_size(egui::vec2(0.0, theme::CONTROL_HEIGHT))
                            .vertical_align(egui::Align::Center)
                            .desired_width(220.0),
                    );
                });
            });
            ui.label(
                RichText::new(
                    "Cookies set by servers are stored here and sent with matching requests (domain, path, Secure).",
                )
                .small()
                .color(p.text_weak),
            );
            ui.add_space(6.0);
            let filter = st.filter.to_lowercase();
            let mut remove = None;
            egui::ScrollArea::vertical().max_height(380.0).auto_shrink([false, true]).show(ui, |ui| {
                if self.ws.cookies.cookies.is_empty() {
                    ui.add_space(20.0);
                    ui.vertical_centered(|ui| {
                        ui.label(
                            RichText::new("No cookies yet. Send a request to a server that sets one.")
                                .color(p.text_weak),
                        );
                    });
                    ui.add_space(20.0);
                    return;
                }
                for domain in self.ws.cookies.domains() {
                    let in_domain: Vec<usize> = self
                        .ws
                        .cookies
                        .cookies
                        .iter()
                        .enumerate()
                        .filter(|(_, c)| c.domain == domain)
                        .filter(|(_, c)| {
                            filter.is_empty() || c.domain.contains(&filter) || c.name.to_lowercase().contains(&filter)
                        })
                        .map(|(i, _)| i)
                        .collect();
                    if in_domain.is_empty() {
                        continue;
                    }
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&domain).strong());
                        widgets::pill(ui, in_domain.len().to_string(), p.text_weak, p.surface_alt);
                    });
                    egui::Grid::new(("cookie_grid", &domain)).num_columns(6).spacing([10.0, 6.0]).striped(true).show(
                        ui,
                        |ui| {
                            for i in in_domain {
                                let c = &mut self.ws.cookies.cookies[i];
                                ui.label(RichText::new(&c.name).monospace().color(theme::ORANGE));
                                changed |= ui
                                    .add(
                                        egui::TextEdit::singleline(&mut c.value)
                                            .font(egui::TextStyle::Monospace)
                                            .min_size(egui::vec2(260.0, theme::CONTROL_HEIGHT))
                                            .vertical_align(egui::Align::Center)
                                            .desired_width(260.0),
                                    )
                                    .changed();
                                ui.label(RichText::new(&c.path).monospace().small().color(p.text_weak));
                                ui.label(RichText::new(expiry_label(c)).small().color(p.text_weak));
                                let mut flags = Vec::new();
                                if c.secure {
                                    flags.push("Secure");
                                }
                                if c.http_only {
                                    flags.push("HttpOnly");
                                }
                                if c.host_only {
                                    flags.push("host-only");
                                }
                                ui.label(RichText::new(flags.join(" · ")).small().color(p.text_weak));
                                if ui.button("×").on_hover_text("Delete cookie").clicked() {
                                    remove = Some(i);
                                }
                                ui.end_row();
                            }
                        },
                    );
                }
            });
            if let Some(i) = remove {
                self.ws.cookies.cookies.remove(i);
                changed = true;
            }
            ui.separator();
            ui.label(RichText::new("ADD A COOKIE").small().strong().color(p.text_weak));
            ui.horizontal(|ui| {
                let field = |ui: &mut egui::Ui, v: &mut String, hint: &str, w: f32| {
                    ui.add(
                        egui::TextEdit::singleline(v)
                            .hint_text(hint)
                            .min_size(egui::vec2(0.0, theme::CONTROL_HEIGHT))
                            .vertical_align(egui::Align::Center)
                            .desired_width(w),
                    );
                };
                field(ui, &mut st.new_domain, "api.example.com", 200.0);
                field(ui, &mut st.new_name, "name", 140.0);
                field(ui, &mut st.new_value, "value", 220.0);
                let ready = !st.new_domain.trim().is_empty() && !st.new_name.trim().is_empty();
                if ui.add_enabled(ready, egui::Button::new("Add")).clicked() {
                    changed |= self.ws.cookies.upsert(StoredCookie {
                        name: st.new_name.trim().to_string(),
                        value: st.new_value.clone(),
                        domain: st.new_domain.trim().trim_start_matches('.').to_ascii_lowercase(),
                        path: "/".into(),
                        host_only: false,
                        ..Default::default()
                    });
                    st.new_name.clear();
                    st.new_value.clear();
                }
            });
            ui.add_space(10.0);
            if widgets::primary_button(ui, "Done").clicked() {
                keep = false;
            }
        });
        if changed {
            let cookies = self.ws.cookies.clone();
            self.cookie_jar.with(|j| *j = cookies);
            let _ = self.ws.save_cookies();
        }
        if r.should_close() {
            keep = false;
        }
        keep
    }
}
