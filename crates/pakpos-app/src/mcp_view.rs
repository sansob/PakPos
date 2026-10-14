//! MCP server requests: pick a tool, resource or prompt and call it.

use crate::app::{Outcome, PakPosApp, ResponseState};
use crate::theme::Palette;
use crate::widgets;
use egui::{RichText, Ui};
use pakpos_core::mcp_client::{self, McpClient, ServerInfo};
use pakpos_core::model::{Auth, Body, McpConfig, McpOperation, RawLanguage};
use pakpos_core::vars::VarScope;
use serde_json::Value;
use std::sync::mpsc::{self, Receiver};
use std::time::Instant;

type Listing = Result<(ServerInfo, Vec<Value>), String>;

/// Per-tab state: what the server offers for the current operation.
#[derive(Default)]
pub struct McpState {
    pub items: Vec<Value>,
    pub loaded: Option<McpOperation>,
    pub server: Option<ServerInfo>,
    pub job: Option<Receiver<Listing>>,
    pub error: Option<String>,
}

pub enum McpAction {
    Load,
}

fn list_key(op: McpOperation) -> &'static str {
    match op {
        McpOperation::ReadResource => "resources",
        McpOperation::GetPrompt => "prompts",
        _ => "tools",
    }
}

fn item_name(op: McpOperation, item: &Value) -> String {
    let key = if op == McpOperation::ReadResource { "uri" } else { "name" };
    item.get(key).and_then(Value::as_str).unwrap_or("").to_string()
}

/// Arguments template for a tool (from its schema) or a prompt (its arguments).
fn template(op: McpOperation, item: &Value) -> Option<String> {
    let v = match op {
        McpOperation::CallTool => mcp_client::arguments_template(item.get("inputSchema").unwrap_or(&Value::Null)),
        McpOperation::GetPrompt => {
            let mut m = serde_json::Map::new();
            for a in item.get("arguments").and_then(Value::as_array).cloned().unwrap_or_default() {
                if let Some(n) = a.get("name").and_then(Value::as_str) {
                    m.insert(n.to_string(), Value::String(String::new()));
                }
            }
            Value::Object(m)
        }
        _ => return None,
    };
    serde_json::to_string_pretty(&v).ok()
}

#[allow(clippy::too_many_arguments)]
pub fn editor(
    ui: &mut Ui,
    cfg: &mut McpConfig,
    body: &mut Body,
    target_url: &str,
    scope: &VarScope,
    state: &McpState,
    uid: u64,
    p: &Palette,
    action: &mut Option<McpAction>,
) -> bool {
    let mut changed = false;
    if !matches!(body, Body::Raw { .. }) {
        *body = Body::Raw { content: "{}".into(), language: RawLanguage::Json };
        changed = true;
    }
    ui.horizontal(|ui| {
        for op in McpOperation::ALL {
            if ui.selectable_value(&mut cfg.operation, op, op.label()).changed() {
                changed = true;
            }
        }
        ui.add_space(8.0);
        if state.job.is_some() {
            ui.add(egui::Spinner::new().color(crate::theme::ORANGE));
        } else if cfg.operation != McpOperation::ListTools
            && ui
                .button(crate::i18n::tr("⟳ Load"))
                .on_hover_text(crate::i18n::tr("Connect and list what the server offers"))
                .clicked()
        {
            *action = Some(McpAction::Load);
        }
    });
    if let Some(s) = &state.server {
        ui.label(
            RichText::new(crate::i18n::trf(
                "Server: {name} {version} · protocol {protocol}",
                &[("name", &s.name), ("version", &s.version), ("protocol", &s.protocol_version)],
            ))
            .small()
            .color(p.text_weak),
        );
    }
    if let Some(e) = &state.error {
        ui.label(RichText::new(e).small().color(crate::theme::RED));
    }
    ui.add_space(4.0);
    if cfg.operation != McpOperation::ListTools {
        let what = match cfg.operation {
            McpOperation::ReadResource => crate::i18n::tr("Resource URI"),
            McpOperation::GetPrompt => crate::i18n::tr("Prompt"),
            _ => crate::i18n::tr("Tool"),
        };
        ui.horizontal(|ui| {
            ui.label(what);
            changed |= ui
                .add(
                    egui::TextEdit::singleline(&mut cfg.target)
                        .min_size(egui::vec2(0.0, crate::theme::CONTROL_HEIGHT))
                        .vertical_align(egui::Align::Center)
                        .desired_width(320.0),
                )
                .changed();
            let loaded = state.loaded == Some(cfg.operation);
            if loaded && !state.items.is_empty() {
                egui::ComboBox::from_id_salt(("mcp_items", uid)).width(260.0).selected_text("Pick…").show_ui(
                    ui,
                    |ui| {
                        for item in &state.items {
                            let name = item_name(cfg.operation, item);
                            let desc = item.get("description").and_then(Value::as_str).unwrap_or("");
                            let resp = ui.selectable_label(cfg.target == name, &name).on_hover_text(desc);
                            if resp.clicked() {
                                cfg.target = name;
                                if let (Some(t), Body::Raw { content, .. }) =
                                    (template(cfg.operation, item), &mut *body)
                                {
                                    *content = t;
                                }
                                changed = true;
                            }
                        }
                    },
                );
            }
        });
        if let Some(item) = state
            .items
            .iter()
            .find(|i| state.loaded == Some(cfg.operation) && item_name(cfg.operation, i) == cfg.target)
        {
            if let Some(d) = item.get("description").and_then(Value::as_str) {
                ui.label(RichText::new(d).small().color(p.text_weak));
            }
        }
    }
    if !mcp_client::is_http(&scope.resolve(target_url)) {
        egui::CollapsingHeader::new(
            RichText::new(crate::i18n::trf(
                "Environment for the server process ({n})",
                &[("n", &cfg.env.iter().filter(|k| !k.is_blank()).count().to_string())],
            ))
            .small(),
        )
        .id_salt(("mcp_env", uid))
        .show(ui, |ui| {
            changed |= widgets::kv_table(ui, "mcp_env", &mut cfg.env, scope, "VARIABLE");
        });
    }
    if matches!(cfg.operation, McpOperation::CallTool | McpOperation::GetPrompt) {
        ui.add_space(4.0);
        widgets::section_title(ui, crate::i18n::tr("Arguments (JSON)"));
        let Body::Raw { content, .. } = body else { return changed };
        let theme_ = egui_extras::syntax_highlighting::CodeTheme::from_style(ui.style());
        let mut layouter = |ui: &Ui, buf: &dyn egui::TextBuffer, wrap_width: f32| {
            let mut job =
                egui_extras::syntax_highlighting::highlight(ui.ctx(), ui.style(), &theme_, buf.as_str(), "json");
            job.wrap.max_width = wrap_width;
            ui.ctx().fonts_mut(|f| f.layout_job(job))
        };
        changed |= ui
            .add(
                egui::TextEdit::multiline(content)
                    .id_salt(("mcp_args", uid))
                    .code_editor()
                    .desired_rows(8)
                    .desired_width(f32::INFINITY)
                    .layouter(&mut layouter),
            )
            .changed();
    }
    ui.label(
        RichText::new(crate::i18n::tr(
            "The URL is an http(s):// endpoint (Streamable HTTP) or a command line that starts the server (stdio).",
        ))
        .small()
        .color(p.text_weak),
    );
    changed
}

impl PakPosApp {
    /// May this tab start a stdio server? Saved requests need a trusted collection.
    fn mcp_allowed(&self, idx: usize, target: &str) -> bool {
        mcp_client::is_http(target) || self.tabs[idx].source.as_ref().is_none_or(|(c, _)| self.ws.is_trusted(c))
    }

    pub fn mcp_load(&mut self, idx: usize, ctx: &egui::Context) {
        let Some(tab) = self.tabs.get(idx) else { return };
        let scope = self.scope_for(tab);
        let fallback = self.inherited_auth(tab);
        let target = scope.resolve(tab.request.url.trim());
        let cfg = tab.request.mcp.clone().unwrap_or_default();
        if !self.mcp_allowed(idx, &target) {
            self.tabs[idx].mcp.error = Some(crate::i18n::tr("This collection is not trusted: a stdio server is a program it would start. Trust it in the Scripts tab first.").into());
            return;
        }
        let mut meta = tab.request.clone();
        meta.body = Body::None;
        let headers = if mcp_client::is_http(&target) {
            pakpos_core::http::prepare(&meta, &scope, fallback.as_ref()).map(|p| p.headers).unwrap_or_default()
        } else {
            Vec::new()
        };
        let env: Vec<(String, String)> = cfg
            .env
            .iter()
            .filter(|kv| kv.enabled && !kv.key.trim().is_empty())
            .map(|kv| (scope.resolve(&kv.key), scope.resolve(&kv.value)))
            .collect();
        let settings = self.ws.settings.http.clone();
        let op = cfg.operation;
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = McpClient::connect(&target, &headers, &env, &settings)
                .and_then(|mut c| c.list(list_key(op)).map(|items| (c.server.clone(), items)))
                .map_err(|e| e.to_string());
            let _ = tx.send(result);
            ctx.request_repaint();
        });
        let st = &mut self.tabs[idx].mcp;
        st.job = Some(rx);
        st.error = None;
        st.loaded = Some(op);
    }

    pub fn poll_mcp(&mut self) {
        for tab in &mut self.tabs {
            let st = &mut tab.mcp;
            let Some(rx) = &st.job else { continue };
            if let Ok(result) = rx.try_recv() {
                st.job = None;
                match result {
                    Ok((server, items)) => {
                        st.server = Some(server);
                        st.items = items;
                    }
                    Err(e) => {
                        st.error = Some(e);
                        st.items.clear();
                    }
                }
            }
        }
    }

    /// Sends an MCP request: the result becomes the response (tests and extractions apply).
    pub fn mcp_send(&mut self, idx: usize, ctx: &egui::Context) {
        let Some(tab) = self.tabs.get(idx) else { return };
        let scope = self.scope_for(tab);
        let fallback = self.inherited_auth(tab);
        let target = scope.resolve(tab.request.url.trim());
        if !self.mcp_allowed(idx, &target) {
            self.tabs[idx].response = ResponseState::Error(
                crate::i18n::tr("This collection is not trusted: a stdio MCP server is a program it would start. Trust the collection (Scripts tab) first.").into(),
            );
            return;
        }
        let mut req = tab.request.clone();
        crate::app::clean_request(&mut req);
        let settings = self.ws.settings.http.clone();
        let mut tokens = self.ws.tokens.clone();
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        let mut scope = scope;
        std::thread::spawn(move || {
            let effective = match &req.auth {
                Auth::Inherit => fallback.clone().unwrap_or(Auth::None),
                a => a.clone(),
            };
            let new_token = match pakpos_core::oauth2::ensure_token(&effective, &mut scope, &mut tokens, &settings) {
                Ok(t) => t,
                Err(e) => {
                    let _ = tx.send(Err(e.to_string()));
                    ctx.request_repaint();
                    return;
                }
            };
            let (prepared, response) = mcp_client::execute(&req, &scope, fallback.as_ref(), &settings);
            let pretty = response.pretty_body();
            let tests = pakpos_core::assertions::run_all(&response, &req.assertions, |s| scope.resolve(s));
            let extracted = pakpos_core::assertions::extract_all(&response, &req.extractions);
            let json = crate::app::json_of(&response);
            let _ = tx.send(Ok(Outcome {
                prepared,
                response,
                pretty,
                tests,
                extracted,
                new_token,
                logs: Vec::new(),
                script_changes: Default::default(),
                json,
            }));
            ctx.request_repaint();
        });
        self.tabs[idx].response = ResponseState::Loading { started: Instant::now(), rx };
    }
}
