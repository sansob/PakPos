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
        if tab.request.kind == pakpos_core::model::RequestKind::WebSocket {
            crate::ws_view::log_view(ui, &mut tab.ws, tab.uid, &p);
            return;
        }
        let mut copy: Option<String> = None;
        let mut cancel = false;
        let mut save_to_file: Option<Vec<u8>> = None;

        match &tab.response {
            ResponseState::Empty => {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Response").strong().color(p.text_weak));
                });
                widgets::empty_state(
                    ui,
                    &p,
                    "Ready when you are",
                    "Enter a URL and press Send (Ctrl+Enter). Paste a cURL command into the URL bar to import it.",
                );
            }
            ResponseState::Loading { started, .. } => {
                let elapsed = started.elapsed().as_millis();
                ui.vertical_centered(|ui| {
                    ui.add_space(ui.available_height() * 0.3);
                    ui.add(egui::Spinner::new().size(28.0).color(theme::ORANGE));
                    ui.add_space(6.0);
                    ui.label(RichText::new(format!("Sending request… {elapsed} ms")).color(p.text_weak));
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                });
            }
            ResponseState::Error(e) => {
                ui.label(RichText::new("Could not send request").strong().color(theme::RED));
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
                    RichText::new("Check the URL, your network/proxy settings, or disable TLS verification in Settings for self-signed certificates.")
                        .small()
                        .color(p.text_weak),
                );
            }
            ResponseState::Done(outcome) => {
                let resp = &outcome.response;
                let passed = outcome.tests.iter().filter(|t| t.passed).count();
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Response").strong().color(p.text_weak));
                    ui.add_space(10.0);
                    let sc = theme::status_color(resp.status);
                    widgets::pill(ui, format!("{} {}", resp.status, resp.status_text), sc, sc.gamma_multiply(0.16));
                    widgets::pill(ui, format!("{} ms", resp.elapsed_ms), p.text, p.surface_alt);
                    widgets::pill(ui, format_size(resp.size_bytes()), p.text, p.surface_alt);
                    if !outcome.tests.is_empty() {
                        let all = passed == outcome.tests.len();
                        let c = if all { theme::GREEN } else { theme::RED };
                        widgets::pill(ui, format!("Tests {passed}/{}", outcome.tests.len()), c, c.gamma_multiply(0.16));
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("⬇ Save").on_hover_text("Save response body to a file").clicked() {
                            save_to_file = Some(resp.body.clone());
                        }
                        if ui.button("📋 Copy").on_hover_text("Copy response body").clicked() {
                            copy = Some(outcome.pretty.clone());
                        }
                    });
                });
                ui.add_space(6.0);
                let items = [
                    (RespSection::Body, "Body".to_string()),
                    (RespSection::Headers, widgets::counted("Headers", resp.headers.len())),
                    (
                        RespSection::Tests,
                        if outcome.tests.is_empty() {
                            "Tests".into()
                        } else {
                            format!("Tests ({passed}/{})", outcome.tests.len())
                        },
                    ),
                    (RespSection::Console, widgets::counted("Console", outcome.logs.len())),
                    (RespSection::Request, "Request".to_string()),
                ];
                widgets::tab_bar(ui, &mut tab.resp_section, &items);

                match tab.resp_section {
                    RespSection::Body => {
                        let is_json = resp.is_json();
                        if is_json {
                            ui.horizontal(|ui| {
                                ui.selectable_value(&mut tab.raw_view, false, "Pretty");
                                ui.selectable_value(&mut tab.raw_view, true, "Raw");
                            });
                        }
                        let text: String =
                            if tab.raw_view || !is_json { resp_text(resp) } else { outcome.pretty.clone() };
                        egui::ScrollArea::both().id_salt(("resp_body", tab.uid)).auto_shrink([false, false]).show(
                            ui,
                            |ui| {
                                let lang = guess_lang(resp.header("content-type").unwrap_or(""), is_json);
                                let mut view = text.as_str();
                                if text.len() < 300_000 && lang != "txt" {
                                    let theme = egui_extras::syntax_highlighting::CodeTheme::from_style(ui.style());
                                    let mut layouter = |ui: &Ui, buf: &dyn egui::TextBuffer, wrap_width: f32| {
                                        let mut job = egui_extras::syntax_highlighting::highlight(
                                            ui.ctx(),
                                            ui.style(),
                                            &theme,
                                            buf.as_str(),
                                            lang,
                                        );
                                        job.wrap.max_width = wrap_width;
                                        ui.ctx().fonts_mut(|f| f.layout_job(job))
                                    };
                                    ui.add(
                                        egui::TextEdit::multiline(&mut view)
                                            .code_editor()
                                            .frame(egui::Frame::NONE)
                                            .desired_width(f32::INFINITY)
                                            .layouter(&mut layouter),
                                    );
                                } else {
                                    ui.add(
                                        egui::TextEdit::multiline(&mut view)
                                            .code_editor()
                                            .frame(egui::Frame::NONE)
                                            .desired_width(f32::INFINITY),
                                    );
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
                                RichText::new("No tests for this request. Add some in the Tests tab above.")
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
                                    widgets::section_title(ui, "Extracted variables");
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
                                    RichText::new(format!("Final URL: {}  ·  {}", resp.url, resp.http_version))
                                        .small()
                                        .color(p.text_weak),
                                );
                                ui.add_space(6.0);
                                widgets::section_title(ui, "Request headers");
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
                                    widgets::section_title(ui, "Request body");
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
        if let Some(text) = copy {
            ui.ctx().copy_text(text);
            self.toast("Copied response to clipboard");
        }
        if let Some(bytes) = save_to_file {
            if let Some(path) = rfd::FileDialog::new().set_file_name("response.txt").save_file() {
                match std::fs::write(&path, bytes) {
                    Ok(()) => self.toast(format!("Saved {}", path.display())),
                    Err(e) => self.toast_error(e.to_string()),
                }
            }
        }
    }
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
