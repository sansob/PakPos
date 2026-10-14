//! Examples editor and the in-app mock server.

use crate::app::PakPosApp;
use crate::theme::{self, Palette};
use crate::widgets;
use egui::{Id, RichText, Ui};
use pakpos_core::mock::{self, Hit, MockOptions, MockServer};
use pakpos_core::model::Example;

/// A running mock server for one collection.
pub struct MockState {
    pub collection: String,
    pub server: MockServer,
    pub hits: Vec<Hit>,
    pub open: bool,
}

/// Saved examples of a request. Returns whether anything changed.
pub fn examples_editor(ui: &mut Ui, examples: &mut Vec<Example>, uid: u64, p: &Palette) -> bool {
    let mut changed = false;
    ui.label(
        RichText::new("Saved responses. Use “Save as example” in the response panel, then serve them with the collection's mock server.")
            .small()
            .color(p.text_weak),
    );
    ui.add_space(6.0);
    if examples.is_empty() {
        ui.label(RichText::new("No examples yet.").color(p.text_weak));
    }
    let mut remove = None;
    for (i, ex) in examples.iter_mut().enumerate() {
        egui::Frame::new().fill(p.surface_alt).corner_radius(8).inner_margin(10).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let c = theme::status_color(ex.status);
                widgets::pill(ui, ex.status.to_string(), c, c.gamma_multiply(0.16));
                changed |= ui
                    .add(
                        egui::TextEdit::singleline(&mut ex.name)
                            .min_size(egui::vec2(0.0, crate::theme::CONTROL_HEIGHT))
                            .vertical_align(egui::Align::Center)
                            .desired_width(260.0),
                    )
                    .changed();
                ui.label(RichText::new("status").small().color(p.text_weak));
                changed |= ui.add(egui::DragValue::new(&mut ex.status).range(100..=599)).changed();
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if widgets::ghost_button(ui, "🗑", "Delete example").clicked() {
                        remove = Some(i);
                    }
                });
            });
            egui::CollapsingHeader::new(RichText::new("Body & headers").small()).id_salt(("ex", uid, i)).show(
                ui,
                |ui| {
                    changed |= widgets::kv_table(
                        ui,
                        &format!("exh{uid}{i}"),
                        &mut ex.headers,
                        &pakpos_core::vars::VarScope::new(),
                        "Header",
                    );
                    changed |= ui
                        .add(
                            egui::TextEdit::multiline(&mut ex.body)
                                .id_salt(("exb", uid, i))
                                .code_editor()
                                .desired_rows(6)
                                .desired_width(f32::INFINITY),
                        )
                        .changed();
                },
            );
        });
        ui.add_space(4.0);
    }
    if let Some(i) = remove {
        examples.remove(i);
        changed = true;
    }
    changed
}

impl PakPosApp {
    pub fn start_mock(&mut self, collection_id: &str) {
        let Some(c) = self.collection(collection_id).cloned() else { return };
        // Try the conventional port first, then any free one.
        let server = mock::start(&c, MockOptions { addr: ([127, 0, 0, 1], 8000).into(), ..Default::default() })
            .or_else(|_| mock::start(&c, MockOptions { addr: ([127, 0, 0, 1], 0).into(), ..Default::default() }));
        match server {
            Ok(server) => {
                let n = server.routes().len();
                self.toast(format!("Mock server for “{}” on http://{} ({n} routes)", c.name, server.addr));
                self.mock = Some(MockState { collection: c.id.clone(), server, hits: Vec::new(), open: true });
            }
            Err(e) => self.toast_error(format!("Cannot start mock server: {e}")),
        }
    }

    pub fn stop_mock(&mut self) {
        if let Some(m) = self.mock.take() {
            m.server.stop();
            self.toast("Mock server stopped");
        }
    }

    /// Picks up new hits and keeps routes in sync with the collection.
    pub fn poll_mock(&mut self) {
        let Some(m) = &mut self.mock else { return };
        m.hits.extend(m.server.hits.try_iter());
        if m.hits.len() > 500 {
            let drop = m.hits.len() - 500;
            m.hits.drain(..drop);
        }
    }

    pub fn reload_mock(&self, collection_id: &str) {
        if let (Some(m), Some(c)) = (&self.mock, self.collection(collection_id)) {
            if m.collection == collection_id {
                m.server.reload(c);
            }
        }
    }

    pub fn mock_window(&mut self, ctx: &egui::Context) {
        let p = self.palette;
        let name =
            self.mock.as_ref().and_then(|m| self.collection(&m.collection)).map(|c| c.name.clone()).unwrap_or_default();
        let Some(m) = &mut self.mock else { return };
        let mut open = m.open;
        let mut stop = false;
        egui::Window::new(format!("Mock server — {name}"))
            .id(Id::new("mock_window"))
            .open(&mut open)
            .default_size([620.0, 460.0])
            .pivot(egui::Align2::CENTER_CENTER)
            .default_pos(ctx.content_rect().center())
            .collapsible(false)
            .frame(
                egui::Frame::new()
                    .fill(p.surface)
                    .stroke(egui::Stroke::new(1.0, p.border))
                    .corner_radius(12)
                    .inner_margin(20),
            )
            .show(ctx, |ui| {
                let url = format!("http://{}", m.server.addr);
                ui.horizontal(|ui| {
                    widgets::pill(ui, "Running", theme::GREEN, theme::GREEN.gamma_multiply(0.16));
                    ui.label(RichText::new(&url).monospace().strong());
                    if ui.button("Copy URL").clicked() {
                        ui.ctx().copy_text(url.clone());
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button(RichText::new("Stop").color(theme::RED)).clicked() {
                            stop = true;
                        }
                    });
                });
                ui.add_space(6.0);
                widgets::section_title(ui, "Routes");
                let routes = m.server.routes();
                if routes.is_empty() {
                    ui.label(
                        RichText::new("No examples yet. Send a request, then “Save as example”.").color(p.text_weak),
                    );
                }
                for r in &routes {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&r.pattern).monospace());
                        let ex: Vec<String> = r.examples.iter().map(|e| format!("{} {}", e.status, e.name)).collect();
                        ui.label(RichText::new(ex.join(" · ")).small().color(p.text_weak));
                    });
                }
                ui.label(
                    RichText::new(
                        "Choose another example with the x-mock-response-name or x-mock-response-code request header.",
                    )
                    .small()
                    .color(p.text_weak),
                );
                ui.add_space(6.0);
                widgets::section_title(ui, &format!("Requests ({})", m.hits.len()));
                egui::ScrollArea::vertical().auto_shrink([false, false]).stick_to_bottom(true).show(ui, |ui| {
                    for h in &m.hits {
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(h.at.with_timezone(&chrono::Local).format("%H:%M:%S").to_string())
                                    .small()
                                    .monospace()
                                    .color(p.text_weak),
                            );
                            ui.label(RichText::new(h.status.to_string()).color(theme::status_color(h.status)).strong());
                            ui.label(RichText::new(format!("{} {}", h.method, h.path)).monospace());
                            ui.label(
                                RichText::new(h.matched.clone().unwrap_or_else(|| "no match".into()))
                                    .small()
                                    .color(p.text_weak),
                            );
                        });
                    }
                });
            });
        m.open = open;
        if stop {
            self.stop_mock();
        }
    }
}
