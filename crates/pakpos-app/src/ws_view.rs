//! WebSocket connections in the desktop app.

use crate::app::PakPosApp;
use crate::theme::{self, Palette};
use crate::widgets;
use egui::{RichText, Ui};
use pakpos_core::model::{Auth, Body, RawLanguage};
use pakpos_core::ws::{self, Direction, WsEvent, WsHandle};

pub struct LogEntry {
    /// `None` for connection/system events.
    pub direction: Option<Direction>,
    pub text: String,
    pub at: chrono::DateTime<chrono::Utc>,
    pub error: bool,
    /// Event name / topic / SSE event type.
    pub label: Option<String>,
}

/// Per-tab WebSocket state.
#[derive(Default)]
pub struct WsState {
    pub handle: Option<WsHandle>,
    pub connected: bool,
    pub connecting: bool,
    pub log: Vec<LogEntry>,
    pub filter: String,
}

impl WsState {
    pub(crate) fn system(&mut self, text: impl Into<String>, error: bool) {
        self.log.push(LogEntry { direction: None, text: text.into(), at: chrono::Utc::now(), error, label: None });
    }
}

pub enum WsAction {
    Connect,
    Disconnect,
    Send(String),
    Emit { event: String, data: String },
    Publish { topic: String, payload: String, qos: u8, retain: bool },
    Subscribe { topic: String, qos: u8 },
    Unsubscribe(String),
}

/// Composer for every realtime kind: WebSocket messages, Socket.IO events,
/// MQTT topics and publishing; SSE is receive-only.
#[allow(clippy::too_many_arguments)]
pub fn realtime_composer(
    ui: &mut Ui,
    kind: pakpos_core::model::RequestKind,
    cfg: &mut Option<pakpos_core::model::RealtimeConfig>,
    body: &mut Body,
    state: &WsState,
    uid: u64,
    p: &Palette,
    action: &mut Option<WsAction>,
) -> bool {
    use pakpos_core::model::RequestKind;
    let field = |ui: &mut Ui, v: &mut String, hint: &str, w: f32| {
        ui.add(
            egui::TextEdit::singleline(v)
                .hint_text(hint)
                .min_size(egui::vec2(0.0, crate::theme::CONTROL_HEIGHT))
                .vertical_align(egui::Align::Center)
                .desired_width(w),
        )
        .changed()
    };
    match kind {
        RequestKind::Sse => {
            widgets::section_title(ui, crate::i18n::tr("Server-Sent Events"));
            ui.label(
                RichText::new(
                    crate::i18n::tr("Receive-only: Connect opens the event stream (Accept: text/event-stream) with this request's params, headers and auth. Events appear below with their type."),
                )
                .color(p.text_weak),
            );
            false
        }
        RequestKind::SocketIo => {
            let c = cfg.get_or_insert_with(Default::default);
            let mut changed = false;
            egui::Grid::new(("sio_grid", uid)).num_columns(2).spacing([10.0, 8.0]).show(ui, |ui| {
                ui.label(crate::i18n::tr("Event"));
                changed |= field(ui, &mut c.event, "message", 260.0);
                ui.end_row();
                ui.label(crate::i18n::tr("Auth"));
                changed |= field(ui, &mut c.auth, "{ \"token\": \"{{token}}\" } (optional)", 360.0);
                ui.end_row();
                ui.label(crate::i18n::tr("Path"));
                changed |= field(ui, &mut c.path, "/socket.io/", 260.0);
                ui.end_row();
            });
            ui.label(
                RichText::new(crate::i18n::tr("The URL's path is the namespace, e.g. http://localhost:3000/admin."))
                    .small()
                    .color(p.text_weak),
            );
            ui.add_space(4.0);
            let event = if c.event.trim().is_empty() { "message".to_string() } else { c.event.trim().to_string() };
            let mut send = None;
            changed |= composer(ui, body, state, uid, p, &mut send);
            if let Some(WsAction::Send(data)) = send {
                *action = Some(WsAction::Emit { event, data });
            }
            changed
        }
        RequestKind::Mqtt => {
            let c = cfg.get_or_insert_with(Default::default);
            let mut changed = false;
            widgets::section_title(ui, crate::i18n::tr("Subscriptions"));
            let mut remove = None;
            for (i, t) in c.subscriptions.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    changed |= ui.checkbox(&mut t.enabled, "").changed();
                    changed |= field(ui, &mut t.topic, "sensors/+/temperature", 300.0);
                    egui::ComboBox::from_id_salt(("mqtt_sub_qos", uid, i))
                        .width(70.0)
                        .selected_text(format!("QoS {}", t.qos))
                        .show_ui(ui, |ui| {
                            for q in 0..=2u8 {
                                changed |= ui.selectable_value(&mut t.qos, q, format!("QoS {q}")).changed();
                            }
                        });
                    if state.connected && !t.topic.trim().is_empty() {
                        if ui.button(crate::i18n::tr("Subscribe")).clicked() {
                            *action = Some(WsAction::Subscribe { topic: t.topic.trim().to_string(), qos: t.qos });
                        }
                        if ui.button(crate::i18n::tr("Unsubscribe")).clicked() {
                            *action = Some(WsAction::Unsubscribe(t.topic.trim().to_string()));
                        }
                    }
                    if widgets::ghost_button(ui, "×", crate::i18n::tr("Remove")).clicked() {
                        remove = Some(i);
                    }
                });
            }
            if let Some(i) = remove {
                c.subscriptions.remove(i);
                changed = true;
            }
            if ui.button(crate::i18n::tr("+ Topic")).clicked() {
                c.subscriptions.push(pakpos_core::model::MqttTopic { enabled: true, ..Default::default() });
                changed = true;
            }
            ui.label(
                RichText::new(crate::i18n::tr(
                    "Enabled topics are subscribed when you connect; + and # are wildcards.",
                ))
                .small()
                .color(p.text_weak),
            );
            ui.add_space(8.0);
            widgets::section_title(ui, crate::i18n::tr("Publish"));
            ui.horizontal(|ui| {
                ui.label(crate::i18n::tr("Topic"));
                changed |= field(ui, &mut c.topic, "sensors/kantor-1/temperature", 300.0);
                egui::ComboBox::from_id_salt(("mqtt_pub_qos", uid))
                    .width(70.0)
                    .selected_text(format!("QoS {}", c.qos))
                    .show_ui(ui, |ui| {
                        for q in 0..=2u8 {
                            changed |= ui.selectable_value(&mut c.qos, q, format!("QoS {q}")).changed();
                        }
                    });
                changed |= ui.checkbox(&mut c.retain, crate::i18n::tr("Retain")).changed();
            });
            egui::CollapsingHeader::new(RichText::new(crate::i18n::tr("Connection")).small())
                .id_salt(("mqtt_conn", uid))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(crate::i18n::tr("Client ID"));
                        changed |= field(ui, &mut c.client_id, "random", 200.0);
                        ui.label(crate::i18n::tr("Keep alive"));
                        changed |=
                            ui.add(egui::DragValue::new(&mut c.keep_alive_secs).range(5..=3600).suffix(" s")).changed();
                        changed |= ui.checkbox(&mut c.clean_session, crate::i18n::tr("Clean session")).changed();
                    });
                    ui.label(
                        RichText::new(crate::i18n::tr("User name and password come from the Auth tab (Basic Auth)."))
                            .small()
                            .color(p.text_weak),
                    );
                });
            let (topic, qos, retain) = (c.topic.trim().to_string(), c.qos, c.retain);
            let mut send = None;
            changed |= composer(ui, body, state, uid, p, &mut send);
            if let Some(WsAction::Send(payload)) = send {
                *action = Some(WsAction::Publish { topic, payload, qos, retain });
            }
            changed
        }
        _ => composer(ui, body, state, uid, p, action),
    }
}

/// Message composer shown in the "Message" section.
pub fn composer(
    ui: &mut Ui,
    body: &mut Body,
    state: &WsState,
    uid: u64,
    p: &Palette,
    action: &mut Option<WsAction>,
) -> bool {
    let mut changed = false;
    if !matches!(body, Body::Raw { .. }) {
        *body = Body::Raw { content: String::new(), language: RawLanguage::Json };
        changed = true;
    }
    let Body::Raw { content, language } = body else { return changed };
    ui.horizontal(|ui| {
        widgets::section_title(ui, crate::i18n::tr("Message"));
        egui::ComboBox::from_id_salt("ws_lang").width(90.0).selected_text(language.label()).show_ui(ui, |ui| {
            for l in [RawLanguage::Json, RawLanguage::Text] {
                changed |= ui.selectable_value(language, l, l.label()).changed();
            }
        });
    });
    let theme_ = egui_extras::syntax_highlighting::CodeTheme::from_style(ui.style());
    let lang = if *language == RawLanguage::Json { "json" } else { "txt" };
    let mut layouter = |ui: &Ui, buf: &dyn egui::TextBuffer, wrap_width: f32| {
        let mut job = egui_extras::syntax_highlighting::highlight(ui.ctx(), ui.style(), &theme_, buf.as_str(), lang);
        job.wrap.max_width = wrap_width;
        ui.ctx().fonts_mut(|f| f.layout_job(job))
    };
    let resp = crate::widgets::code_editor(ui, content, |e| {
        e.id_salt(("ws_msg", uid))
            .desired_rows(8)
            .desired_width(f32::INFINITY)
            .hint_text(crate::i18n::tr("{ \"type\": \"subscribe\", \"channel\": \"orders\" }"))
            .layouter(&mut layouter)
    });
    changed |= resp.changed();
    let send_shortcut = resp.has_focus() && ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Enter));
    ui.horizontal(|ui| {
        let enabled = state.connected && !content.trim().is_empty();
        if ui
            .add_enabled(enabled, egui::Button::new(crate::i18n::tr("Send message")))
            .on_hover_text(crate::i18n::tr("Ctrl+Enter in the editor"))
            .clicked()
            || (enabled && send_shortcut)
        {
            *action = Some(WsAction::Send(content.clone()));
        }
        if !state.connected {
            ui.label(
                RichText::new(crate::i18n::tr("Connect first (the button next to the URL)."))
                    .small()
                    .color(p.text_weak),
            );
        } else {
            ui.label(
                RichText::new(crate::i18n::tr("{{variables}} are resolved before sending.")).small().color(p.text_weak),
            );
        }
    });
    changed
}

/// Message log shown in the response panel.
pub fn log_view(ui: &mut Ui, state: &mut WsState, uid: u64, p: &Palette) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(crate::i18n::tr("Messages")).strong().color(p.text_weak));
        ui.add_space(8.0);
        if state.connected {
            widgets::pill(ui, crate::i18n::tr("Connected"), theme::GREEN, theme::GREEN.gamma_multiply(0.16));
        } else if state.connecting {
            ui.add(egui::Spinner::new().color(theme::ORANGE));
        } else {
            widgets::pill(ui, crate::i18n::tr("Disconnected"), p.text_weak, p.surface_alt);
        }
        let (sent, recv) = state.log.iter().fold((0, 0), |(s, r), e| match e.direction {
            Some(Direction::Sent) => (s + 1, r),
            Some(Direction::Received) => (s, r + 1),
            None => (s, r),
        });
        widgets::pill(ui, format!("↑ {sent}  ↓ {recv}"), p.text, p.surface_alt);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button(crate::i18n::tr("Clear")).clicked() {
                state.log.clear();
            }
            ui.add(
                egui::TextEdit::singleline(&mut state.filter)
                    .min_size(egui::vec2(0.0, crate::theme::CONTROL_HEIGHT))
                    .vertical_align(egui::Align::Center)
                    .hint_text(crate::i18n::tr("🔍 Filter"))
                    .desired_width(160.0),
            );
        });
    });
    ui.add_space(4.0);
    if state.log.is_empty() {
        widgets::empty_state(
            ui,
            p,
            crate::i18n::tr("No messages yet"),
            crate::i18n::tr("Connect, then send a message from the Message tab."),
        );
        return;
    }
    let filter = state.filter.to_lowercase();
    egui::ScrollArea::vertical().id_salt(("ws_log", uid)).auto_shrink([false, false]).stick_to_bottom(true).show(
        ui,
        |ui| {
            for e in state.log.iter().filter(|e| {
                filter.is_empty()
                    || e.text.to_lowercase().contains(&filter)
                    || e.label.as_deref().is_some_and(|l| l.to_lowercase().contains(&filter))
            }) {
                let time = e.at.with_timezone(&chrono::Local).format("%H:%M:%S%.3f").to_string();
                ui.horizontal_top(|ui| {
                    let (arrow, color) = match e.direction {
                        Some(Direction::Sent) => ("↑", theme::ORANGE),
                        Some(Direction::Received) => ("↓", theme::GREEN),
                        None if e.error => ("!", theme::RED),
                        None => ("•", p.text_weak),
                    };
                    ui.label(RichText::new(arrow).strong().color(color));
                    ui.label(RichText::new(time).small().monospace().color(p.text_weak));
                    if let Some(l) = &e.label {
                        widgets::pill(ui, l, theme::TEAL, theme::TEAL.gamma_multiply(0.16));
                    }
                    let text = if e.direction.is_some() {
                        // Pretty-print JSON messages, truncate huge ones.
                        let pretty = serde_json::from_str::<serde_json::Value>(&e.text)
                            .ok()
                            .and_then(|v| serde_json::to_string_pretty(&v).ok())
                            .unwrap_or_else(|| e.text.clone());
                        if pretty.len() > 20_000 {
                            format!("{}…", &pretty[..pretty.floor_char_boundary(20_000)])
                        } else {
                            pretty
                        }
                    } else {
                        e.text.clone()
                    };
                    let rich = RichText::new(text).monospace().color(if e.error { theme::RED } else { p.text });
                    let resp = ui
                        .add(egui::Label::new(rich).wrap().sense(egui::Sense::click()))
                        .on_hover_text(crate::i18n::tr("Click to copy"));
                    if resp.clicked() {
                        ui.ctx().copy_text(e.text.clone());
                    }
                });
                ui.separator();
            }
        },
    );
}

impl PakPosApp {
    pub fn handle_ws_action(&mut self, idx: usize, action: WsAction, ctx: &egui::Context) {
        match action {
            WsAction::Connect => self.ws_connect(idx, ctx),
            WsAction::Disconnect => {
                if let Some(h) = &self.tabs[idx].ws.handle {
                    h.close();
                }
            }
            WsAction::Send(text) => {
                let scope = self.scope_for(&self.tabs[idx]);
                let resolved = scope.resolve(&text);
                if let Some(h) = &self.tabs[idx].ws.handle {
                    h.send(resolved);
                }
            }
            other => {
                let scope = self.scope_for(&self.tabs[idx]);
                let r = |s: &str| scope.resolve(s);
                let cmd = match other {
                    WsAction::Emit { event, data } => ws::WsCommand::Emit { event: r(&event), data: r(&data) },
                    WsAction::Publish { topic, payload, qos, retain } => {
                        if topic.is_empty() {
                            self.tabs[idx].ws.system(crate::i18n::tr("Pick a topic to publish to"), true);
                            return;
                        }
                        ws::WsCommand::Publish { topic: r(&topic), payload: r(&payload), qos, retain }
                    }
                    WsAction::Subscribe { topic, qos } => ws::WsCommand::Subscribe { topic: r(&topic), qos },
                    WsAction::Unsubscribe(topic) => ws::WsCommand::Unsubscribe(r(&topic)),
                    _ => return,
                };
                if let Some(h) = &self.tabs[idx].ws.handle {
                    let _ = h.tx.send(cmd);
                }
            }
        }
    }

    pub fn ws_connect(&mut self, idx: usize, ctx: &egui::Context) {
        let Some(tab) = self.tabs.get(idx) else { return };
        if tab.ws.handle.is_some() {
            return;
        }
        let mut scope = self.scope_for(tab);
        let fallback = self.inherited_auth(tab);
        let mut req = tab.request.clone();
        crate::app::clean_request(&mut req);
        // The composer draft is not an upgrade body.
        req.body = Body::None;
        let effective = match &req.auth {
            Auth::Inherit => fallback.clone().unwrap_or(Auth::None),
            a => a.clone(),
        };
        let mut tokens = self.ws.tokens.clone();
        let result = pakpos_core::oauth2::ensure_token(&effective, &mut scope, &mut tokens, &self.ws.settings.http)
            .map_err(|e| e.to_string())
            .and_then(|_| pakpos_core::http::prepare(&req, &scope, fallback.as_ref()).map_err(|e| e.to_string()));
        let kind = req.kind;
        let cfg = req.realtime.clone().unwrap_or_default();
        let settings = self.ws.settings.http.clone();
        let (user, pass) = match &effective {
            Auth::Basic { username, password } => (Some(scope.resolve(username)), Some(scope.resolve(password))),
            _ => (None, None),
        };
        let resolved_url = scope.resolve(req.url.trim());
        let state = &mut self.tabs[idx].ws;
        match result {
            Ok(prepared) => {
                let ctx = ctx.clone();
                let notify = move || ctx.request_repaint();
                use pakpos_core::model::RequestKind;
                let handle = match kind {
                    RequestKind::Sse => {
                        state.system(format!("Opening event stream {}", prepared.url), false);
                        pakpos_core::sse::connect(&prepared, &settings, notify)
                    }
                    RequestKind::SocketIo => {
                        state.system(format!("Connecting to Socket.IO at {}", prepared.url), false);
                        pakpos_core::socketio::connect(
                            &prepared,
                            &scope.resolve(&cfg.path),
                            &scope.resolve(&cfg.auth),
                            notify,
                        )
                    }
                    RequestKind::Mqtt => {
                        state.system(format!("Connecting to MQTT broker {resolved_url}"), false);
                        let options = pakpos_core::mqtt::MqttOptions {
                            url: resolved_url.clone(),
                            client_id: scope.resolve(&cfg.client_id),
                            // Basic auth with an empty user name means "no credentials".
                            username: user.filter(|u| !u.is_empty()),
                            password: pass.filter(|p| !p.is_empty()),
                            keep_alive_secs: cfg.keep_alive_secs,
                            clean_session: cfg.clean_session,
                            subscriptions: cfg
                                .subscriptions
                                .iter()
                                .filter(|t| t.enabled && !t.topic.trim().is_empty())
                                .map(|t| (scope.resolve(t.topic.trim()), t.qos))
                                .collect(),
                            settings,
                        };
                        pakpos_core::mqtt::connect(options, notify)
                    }
                    _ => {
                        state.system(format!("Connecting to {}", ws::to_ws_url(&prepared.url)), false);
                        ws::connect(&prepared, notify)
                    }
                };
                state.handle = Some(handle);
                state.connecting = true;
            }
            Err(e) => state.system(e, true),
        }
        if tokens != self.ws.tokens {
            self.ws.tokens = tokens;
            let _ = self.ws.save_tokens();
        }
    }

    pub fn poll_ws(&mut self) {
        for tab in &mut self.tabs {
            let state = &mut tab.ws;
            let Some(handle) = &state.handle else { continue };
            let mut events = Vec::new();
            while let Ok(e) = handle.rx.try_recv() {
                events.push(e);
            }
            for e in events {
                match e {
                    WsEvent::Connected { status, .. } => {
                        state.connected = true;
                        state.connecting = false;
                        state.system(format!("Connected (HTTP {status})"), false);
                    }
                    WsEvent::Message { direction, text, binary, at, label } => {
                        let text = if binary { format!("[binary] {text}") } else { text };

                        state.log.push(LogEntry { direction: Some(direction), text, at, error: false, label });
                    }
                    WsEvent::Error(e) => state.system(e, true),
                    WsEvent::Info(t) => state.system(t, false),
                    WsEvent::Closed { reason } => {
                        state.connected = false;
                        state.connecting = false;
                        state.handle = None;
                        state.system(format!("Disconnected: {reason}"), false);
                        break;
                    }
                }
            }
            // Keep memory bounded on chatty sockets.
            if state.log.len() > 5000 {
                let drop = state.log.len() - 5000;
                state.log.drain(..drop);
            }
        }
    }
}
