//! gRPC requests in the desktop app.

use crate::app::{Outcome, PakPosApp, ResponseState};
use crate::theme::Palette;
use crate::widgets;
use egui::{RichText, Ui};
use pakpos_core::grpc::{self, MethodInfo};
use pakpos_core::http::{self, PreparedBody, Response};
use pakpos_core::model::{Auth, Body, GrpcConfig, RawLanguage};
use prost_reflect::DescriptorPool;
use std::sync::mpsc::{self, Receiver};

/// Per-tab gRPC state: the loaded service descriptions.
#[derive(Default)]
pub struct GrpcState {
    pub pool: Option<DescriptorPool>,
    pub methods: Vec<MethodInfo>,
    /// What the pool was loaded from, to know when it is stale.
    pub source_key: String,
    pub job: Option<Receiver<Result<DescriptorPool, String>>>,
    pub error: Option<String>,
}

pub enum GrpcAction {
    Load,
    Template,
    AddProto,
}

fn source_key(url: &str, cfg: &GrpcConfig) -> String {
    format!("{url}|{}|{}", cfg.proto_files.join(","), cfg.import_paths.join(","))
}

/// Method picker, proto files and the JSON message editor.
#[allow(clippy::too_many_arguments)]
pub fn editor(
    ui: &mut Ui,
    cfg: &mut GrpcConfig,
    body: &mut Body,
    state: &GrpcState,
    uid: u64,
    p: &Palette,
    action: &mut Option<GrpcAction>,
) -> bool {
    let mut changed = false;
    if !matches!(body, Body::Raw { .. }) {
        *body = Body::Raw { content: "{}".into(), language: RawLanguage::Json };
        changed = true;
    }
    ui.horizontal(|ui| {
        ui.label("Method");
        let selected =
            if cfg.method.is_empty() { "Load methods, then pick one".to_string() } else { cfg.method.clone() };
        egui::ComboBox::from_id_salt(("grpc_method", uid)).width(380.0).selected_text(selected).show_ui(ui, |ui| {
            if state.methods.is_empty() {
                ui.label(RichText::new("No methods loaded yet").color(p.text_weak));
            }
            for m in &state.methods {
                let label = format!("{}  ·  {}", m.full_name, m.kind_label());
                if ui.selectable_label(cfg.method == m.full_name, label).clicked() && cfg.method != m.full_name {
                    cfg.method = m.full_name.clone();
                    changed = true;
                    *action = Some(GrpcAction::Template);
                }
            }
        });
        if state.job.is_some() {
            ui.add(egui::Spinner::new().color(crate::theme::ORANGE));
        } else if ui
            .button("⟳ Load methods")
            .on_hover_text(if cfg.proto_files.is_empty() {
                "Ask the server (gRPC reflection)"
            } else {
                "Compile the .proto files"
            })
            .clicked()
        {
            *action = Some(GrpcAction::Load);
        }
    });
    if let Some(e) = &state.error {
        ui.label(RichText::new(e).small().color(crate::theme::RED));
    }
    if let Some(m) = state.methods.iter().find(|m| m.full_name == cfg.method) {
        ui.label(RichText::new(format!("{} → {}  ({})", m.input, m.output, m.kind_label())).small().color(p.text_weak));
    }
    ui.add_space(4.0);
    egui::CollapsingHeader::new(RichText::new(if cfg.proto_files.is_empty() {
        "Schema: server reflection".to_string()
    } else {
        format!("Schema: {} .proto file(s)", cfg.proto_files.len())
    }))
    .id_salt(("grpc_schema", uid))
    .show(ui, |ui| {
        ui.label(
            RichText::new("Leave empty to describe the service with gRPC server reflection, or add the .proto files.")
                .small()
                .color(p.text_weak),
        );
        let mut remove = None;
        for (i, f) in cfg.proto_files.iter().enumerate() {
            ui.horizontal(|ui| {
                ui.label(RichText::new(f).monospace().small());
                if widgets::ghost_button(ui, "×", "Remove").clicked() {
                    remove = Some(i);
                }
            });
        }
        if let Some(i) = remove {
            cfg.proto_files.remove(i);
            changed = true;
        }
        ui.horizontal(|ui| {
            if ui.button("Add .proto…").clicked() {
                *action = Some(GrpcAction::AddProto);
            }
            ui.label(RichText::new("Import paths").small().color(p.text_weak));
            let mut paths = cfg.import_paths.join(";");
            if ui
                .add(
                    egui::TextEdit::singleline(&mut paths)
                        .min_size(egui::vec2(0.0, crate::theme::CONTROL_HEIGHT))
                        .vertical_align(egui::Align::Center)
                        .hint_text("dir1;dir2 (optional)")
                        .desired_width(240.0),
                )
                .changed()
            {
                cfg.import_paths =
                    paths.split(';').map(str::trim).filter(|s| !s.is_empty()).map(str::to_string).collect();
                changed = true;
            }
        });
    });
    ui.add_space(6.0);
    let Body::Raw { content, .. } = body else { return changed };
    ui.horizontal(|ui| {
        widgets::section_title(ui, "Message (JSON)");
        if ui.button("Generate example").clicked() {
            *action = Some(GrpcAction::Template);
        }
        let streaming = state.methods.iter().find(|m| m.full_name == cfg.method).is_some_and(|m| m.client_streaming);
        if streaming {
            ui.label(RichText::new("client streaming: send a JSON array of messages").small().color(p.text_weak));
        }
    });
    let theme_ = egui_extras::syntax_highlighting::CodeTheme::from_style(ui.style());
    let mut layouter = |ui: &Ui, buf: &dyn egui::TextBuffer, wrap_width: f32| {
        let mut job = egui_extras::syntax_highlighting::highlight(ui.ctx(), ui.style(), &theme_, buf.as_str(), "json");
        job.wrap.max_width = wrap_width;
        ui.ctx().fonts_mut(|f| f.layout_job(job))
    };
    changed |= ui
        .add(
            egui::TextEdit::multiline(content)
                .id_salt(("grpc_msg", uid))
                .code_editor()
                .desired_rows(10)
                .desired_width(f32::INFINITY)
                .layouter(&mut layouter),
        )
        .changed();
    changed
}

/// gRPC status → the generic response shape, so the Body / Headers / Tests views work.
fn to_response(r: &grpc::GrpcResponse, url: &str) -> Response {
    let mut headers = r.headers.clone();
    headers.extend(r.trailers.iter().map(|(k, v)| (format!("trailer: {k}"), v.clone())));
    headers.push(("grpc-status".into(), r.code.to_string()));
    if !r.status_message.is_empty() {
        headers.push(("grpc-message".into(), r.status_message.clone()));
    }
    let body = if r.messages.is_empty() { String::new() } else { r.body() };
    Response {
        status: r.code as u16,
        status_text: r.code_name.clone(),
        headers,
        body: body.into_bytes(),
        elapsed_ms: r.elapsed_ms,
        url: url.to_string(),
        http_version: "gRPC".into(),
    }
}

impl PakPosApp {
    pub fn handle_grpc_action(&mut self, idx: usize, action: GrpcAction, ctx: &egui::Context) {
        match action {
            GrpcAction::Load => self.grpc_load(idx, ctx),
            GrpcAction::AddProto => {
                if let Some(files) = rfd::FileDialog::new().add_filter("Protocol Buffers", &["proto"]).pick_files() {
                    let tab = &mut self.tabs[idx];
                    let cfg = tab.request.grpc.get_or_insert_with(Default::default);
                    for f in files {
                        let f = f.display().to_string();
                        if !cfg.proto_files.contains(&f) {
                            cfg.proto_files.push(f);
                        }
                    }
                    tab.dirty = true;
                    self.grpc_load(idx, ctx);
                }
            }
            GrpcAction::Template => {
                let tab = &mut self.tabs[idx];
                let method = tab.request.grpc.as_ref().map(|g| g.method.clone()).unwrap_or_default();
                if let Some(pool) = &tab.grpc.pool {
                    match grpc::request_template(pool, &method) {
                        Ok(t) => {
                            tab.request.body = Body::Raw { content: t, language: RawLanguage::Json };
                            tab.dirty = true;
                        }
                        Err(e) => tab.grpc.error = Some(e.to_string()),
                    }
                }
            }
        }
    }

    /// Loads service descriptions (reflection or .proto files) in the background.
    pub fn grpc_load(&mut self, idx: usize, ctx: &egui::Context) {
        let Some(tab) = self.tabs.get(idx) else { return };
        let scope = self.scope_for(tab);
        let url = scope.resolve(&tab.request.url);
        let cfg = tab.request.grpc.clone().unwrap_or_default();
        let settings = self.ws.settings.http.clone();
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(grpc::load_pool(&cfg, &url, &settings).map_err(|e| e.to_string()));
            ctx.request_repaint();
        });
        let key =
            source_key(&scope.resolve(&tab.request.url), tab.request.grpc.as_ref().unwrap_or(&GrpcConfig::default()));
        let st = &mut self.tabs[idx].grpc;
        st.job = Some(rx);
        st.error = None;
        st.source_key = key;
    }

    pub fn poll_grpc(&mut self) {
        for tab in &mut self.tabs {
            let st = &mut tab.grpc;
            let Some(rx) = &st.job else { continue };
            if let Ok(result) = rx.try_recv() {
                st.job = None;
                match result {
                    Ok(pool) => {
                        st.methods = grpc::methods(&pool);
                        st.pool = Some(pool);
                        st.error =
                            if st.methods.is_empty() { Some("The server exposes no services.".into()) } else { None };
                        let cfg = tab.request.grpc.get_or_insert_with(Default::default);
                        if cfg.method.is_empty() || !st.methods.iter().any(|m| m.full_name == cfg.method) {
                            if let Some(first) = st.methods.first() {
                                cfg.method = first.full_name.clone();
                            }
                        }
                    }
                    Err(e) => st.error = Some(e),
                }
            }
        }
    }

    /// Sends a gRPC request (loading descriptors first if needed).
    pub fn grpc_send(&mut self, idx: usize, ctx: &egui::Context) {
        let Some(tab) = self.tabs.get(idx) else { return };
        let mut scope = self.scope_for(tab);
        let fallback = self.inherited_auth(tab);
        let settings = self.ws.settings.http.clone();
        let mut tokens = self.ws.tokens.clone();
        let mut req = tab.request.clone();
        crate::app::clean_request(&mut req);
        let cfg = req.grpc.clone().unwrap_or_default();
        let key = source_key(&scope.resolve(&req.url), &cfg);
        let cached = tab.grpc.pool.clone().filter(|_| tab.grpc.source_key == key);
        let message = match &req.body {
            Body::Raw { content, .. } => scope.resolve(content),
            _ => "{}".into(),
        };
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = (|| -> Result<Outcome, String> {
                let effective = match &req.auth {
                    Auth::Inherit => fallback.clone().unwrap_or(Auth::None),
                    a => a.clone(),
                };
                let new_token = pakpos_core::oauth2::ensure_token(&effective, &mut scope, &mut tokens, &settings)
                    .map_err(|e| e.to_string())?;
                // Headers + auth become metadata; the body is sent separately.
                let mut meta_req = req.clone();
                meta_req.body = Body::None;
                let prepared = http::prepare(&meta_req, &scope, fallback.as_ref()).map_err(|e| e.to_string())?;
                let pool = match cached {
                    Some(p) => p,
                    None => grpc::load_pool(&cfg, &prepared.url, &settings).map_err(|e| e.to_string())?,
                };
                if cfg.method.is_empty() {
                    return Err("Pick a method first (Message tab → Load methods).".into());
                }
                let r = grpc::call(&prepared.url, &pool, &cfg.method, &message, &prepared.headers, &settings, 500)
                    .map_err(|e| e.to_string())?;
                let response = to_response(&r, &prepared.url);
                let pretty = response.pretty_body();
                let tests = pakpos_core::assertions::run_all(&response, &req.assertions, |s| scope.resolve(s));
                let extracted = pakpos_core::assertions::extract_all(&response, &req.extractions);
                let mut prepared = prepared;
                prepared.method = "gRPC".into();
                prepared.url = format!("{} {}", prepared.url, cfg.method);
                prepared.body = PreparedBody::Text(message.clone());
                Ok(Outcome {
                    prepared,
                    response,
                    pretty,
                    tests,
                    extracted,
                    new_token,
                    logs: Vec::new(),
                    script_changes: Default::default(),
                })
            })();
            let _ = tx.send(result);
            ctx.request_repaint();
        });
        if let Some(tab) = self.tabs.get_mut(idx) {
            tab.response = ResponseState::Loading { started: std::time::Instant::now(), rx };
        }
    }
}
