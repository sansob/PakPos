//! GraphQL body editor with a schema explorer.

use crate::app::PakPosApp;
use crate::theme::{self, Palette};
use crate::widgets;
use egui::{RichText, Ui};
use pakpos_core::graphql::{self, Schema};
use pakpos_core::model::{Auth, Body, HttpMethod};
use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver};

pub enum GqlAction {
    FetchSchema,
    Replace(String),
}

/// Introspection results by resolved URL.
#[derive(Default)]
pub struct GqlState {
    pub schemas: HashMap<String, Result<Schema, String>>,
    pub job: Option<(String, Receiver<Result<Schema, String>>)>,
}

fn code_layouter<'a>(
    lang: &'a str,
) -> impl FnMut(&Ui, &dyn egui::TextBuffer, f32) -> std::sync::Arc<egui::Galley> + 'a {
    move |ui: &Ui, buf: &dyn egui::TextBuffer, wrap_width: f32| {
        let theme = egui_extras::syntax_highlighting::CodeTheme::from_style(ui.style());
        let mut job = egui_extras::syntax_highlighting::highlight(ui.ctx(), ui.style(), &theme, buf.as_str(), lang);
        job.wrap.max_width = wrap_width;
        ui.ctx().fonts_mut(|f| f.layout_job(job))
    }
}

/// Query + variables editors and the explorer. Returns whether the body changed.
#[allow(clippy::too_many_arguments)]
pub fn editor(
    ui: &mut Ui,
    query: &mut String,
    variables: &mut String,
    operation_name: &mut String,
    uid: u64,
    schema: Option<&Result<Schema, String>>,
    loading: bool,
    p: &Palette,
    action: &mut Option<GqlAction>,
) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        if loading {
            ui.add(egui::Spinner::new().color(theme::ORANGE));
            ui.label(RichText::new("Fetching schema…").color(p.text_weak));
        } else if ui.button("⟳ Fetch schema").on_hover_text("Run an introspection query against the URL").clicked() {
            *action = Some(GqlAction::FetchSchema);
        }
        ui.label(RichText::new("Operation name").small().color(p.text_weak));
        changed |=
            ui.add(egui::TextEdit::singleline(operation_name).hint_text("optional").desired_width(140.0)).changed();
    });
    ui.add_space(4.0);
    let total = ui.available_width();
    let explorer_w = if schema.is_some() { (total * 0.3).clamp(200.0, 340.0) } else { 0.0 };
    let editors_w = total - explorer_w - if schema.is_some() { 12.0 } else { 0.0 };
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width(editors_w);
            widgets::section_title(ui, "Query");
            let mut lay = code_layouter("js");
            changed |= ui
                .add(
                    egui::TextEdit::multiline(query)
                        .id_salt(("gql_query", uid))
                        .code_editor()
                        .desired_rows(12)
                        .desired_width(f32::INFINITY)
                        .hint_text("query { me { id name } }")
                        .layouter(&mut lay),
                )
                .changed();
            ui.add_space(4.0);
            widgets::section_title(ui, "Variables (JSON)");
            let mut lay = code_layouter("json");
            changed |= ui
                .add(
                    egui::TextEdit::multiline(variables)
                        .id_salt(("gql_vars", uid))
                        .code_editor()
                        .desired_rows(4)
                        .desired_width(f32::INFINITY)
                        .hint_text("{ \"id\": \"{{user_id}}\" }")
                        .layouter(&mut lay),
                )
                .changed();
            if !variables.trim().is_empty() && !variables.contains("{{") {
                if let Err(e) = serde_json::from_str::<serde_json::Value>(variables) {
                    ui.label(
                        RichText::new(format!("⚠ Variables are not valid JSON: {e}")).small().color(theme::YELLOW),
                    );
                }
            }
        });
        if let Some(schema) = schema {
            ui.separator();
            ui.vertical(|ui| {
                ui.set_width(explorer_w);
                widgets::section_title(ui, "Schema explorer");
                match schema {
                    Err(e) => {
                        ui.label(RichText::new(e).small().color(theme::RED));
                    }
                    Ok(s) => {
                        egui::ScrollArea::vertical().id_salt(("gql_explorer", uid)).max_height(320.0).show(ui, |ui| {
                            for (title, op, fields) in [
                                ("Queries", "query", &s.queries),
                                ("Mutations", "mutation", &s.mutations),
                                ("Subscriptions", "subscription", &s.subscriptions),
                            ] {
                                if fields.is_empty() {
                                    continue;
                                }
                                egui::CollapsingHeader::new(RichText::new(title).strong())
                                    .id_salt((title, uid))
                                    .default_open(op == "query")
                                    .show(ui, |ui| {
                                        for f in fields.iter() {
                                            let args = if f.args.is_empty() {
                                                String::new()
                                            } else {
                                                format!(
                                                    "({})",
                                                    f.args
                                                        .iter()
                                                        .map(|(n, t)| format!("{n}: {t}"))
                                                        .collect::<Vec<_>>()
                                                        .join(", ")
                                                )
                                            };
                                            let resp = ui
                                                .add(
                                                    egui::Label::new(
                                                        RichText::new(format!("{}{args}: {}", f.name, f.ty))
                                                            .monospace()
                                                            .size(12.0),
                                                    )
                                                    .sense(egui::Sense::click()),
                                                )
                                                .on_hover_text(if f.description.is_empty() {
                                                    "Click to insert a starter query".to_string()
                                                } else {
                                                    format!("{}\n\nClick to insert a starter query", f.description)
                                                });
                                            if resp.hovered() {
                                                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                                            }
                                            if resp.clicked() {
                                                *action = Some(GqlAction::Replace(graphql::skeleton(s, op, f)));
                                            }
                                        }
                                    });
                            }
                            ui.label(
                                RichText::new(format!("{} object types", s.types.len())).small().color(p.text_weak),
                            );
                        });
                    }
                }
            });
        }
    });
    changed
}

impl PakPosApp {
    /// Sends an introspection query using the tab's URL, headers and auth.
    pub fn fetch_graphql_schema(&mut self, idx: usize, ctx: &egui::Context) {
        let Some(tab) = self.tabs.get(idx) else { return };
        let mut scope = self.scope_for(tab);
        let fallback = self.inherited_auth(tab);
        let settings = self.ws.settings.http.clone();
        let mut tokens = self.ws.tokens.clone();
        let mut req = tab.request.clone();
        crate::app::clean_request(&mut req);
        req.method = HttpMethod::Post;
        req.body = Body::GraphQl {
            query: graphql::INTROSPECTION_QUERY.into(),
            variables: String::new(),
            operation_name: "PakPosIntrospection".into(),
        };
        let key = scope.resolve(&req.url);
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let effective = match &req.auth {
                Auth::Inherit => fallback.clone().unwrap_or(Auth::None),
                a => a.clone(),
            };
            let result = pakpos_core::oauth2::ensure_token(&effective, &mut scope, &mut tokens, &settings)
                .map_err(|e| e.to_string())
                .and_then(|_| {
                    pakpos_core::http::execute(&req, &scope, fallback.as_ref(), &settings).map_err(|e| e.to_string())
                })
                .and_then(|(_, resp)| {
                    let json = resp.json().ok_or_else(|| format!("HTTP {}: response is not JSON", resp.status))?;
                    graphql::parse_schema(&json)
                });
            let _ = tx.send(result);
            ctx.request_repaint();
        });
        self.graphql.job = Some((key, rx));
    }

    pub fn poll_graphql(&mut self) {
        let Some((key, rx)) = &self.graphql.job else { return };
        if let Ok(result) = rx.try_recv() {
            if let Err(e) = &result {
                self.toasts.push(crate::app::Toast {
                    text: e.clone(),
                    error: true,
                    until: std::time::Instant::now() + std::time::Duration::from_secs(6),
                });
            }
            self.graphql.schemas.insert(key.clone(), result);
            self.graphql.job = None;
        }
    }
}
