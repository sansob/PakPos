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
    fn system(&mut self, text: impl Into<String>, error: bool) {
        self.log.push(LogEntry { direction: None, text: text.into(), at: chrono::Utc::now(), error });
    }
}

pub enum WsAction {
    Connect,
    Disconnect,
    Send(String),
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
        widgets::section_title(ui, "Message");
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
    let resp = ui.add(
        egui::TextEdit::multiline(content)
            .id_salt(("ws_msg", uid))
            .code_editor()
            .desired_rows(8)
            .desired_width(f32::INFINITY)
            .hint_text("{ \"type\": \"subscribe\", \"channel\": \"orders\" }")
            .layouter(&mut layouter),
    );
    changed |= resp.changed();
    let send_shortcut = resp.has_focus() && ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Enter));
    ui.horizontal(|ui| {
        let enabled = state.connected && !content.trim().is_empty();
        if ui
            .add_enabled(enabled, egui::Button::new("Send message"))
            .on_hover_text("Ctrl+Enter in the editor")
            .clicked()
            || (enabled && send_shortcut)
        {
            *action = Some(WsAction::Send(content.clone()));
        }
        if !state.connected {
            ui.label(RichText::new("Connect first (the button next to the URL).").small().color(p.text_weak));
        } else {
            ui.label(RichText::new("{{variables}} are resolved before sending.").small().color(p.text_weak));
        }
    });
    changed
}

/// Message log shown in the response panel.
pub fn log_view(ui: &mut Ui, state: &mut WsState, uid: u64, p: &Palette) {
    ui.horizontal(|ui| {
        ui.label(RichText::new("Messages").strong().color(p.text_weak));
        ui.add_space(8.0);
        if state.connected {
            widgets::pill(ui, "Connected", theme::GREEN, theme::GREEN.gamma_multiply(0.16));
        } else if state.connecting {
            ui.add(egui::Spinner::new().color(theme::ORANGE));
        } else {
            widgets::pill(ui, "Disconnected", p.text_weak, p.surface_alt);
        }
        let (sent, recv) = state.log.iter().fold((0, 0), |(s, r), e| match e.direction {
            Some(Direction::Sent) => (s + 1, r),
            Some(Direction::Received) => (s, r + 1),
            None => (s, r),
        });
        widgets::pill(ui, format!("↑ {sent}  ↓ {recv}"), p.text, p.surface_alt);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Clear").clicked() {
                state.log.clear();
            }
            ui.add(egui::TextEdit::singleline(&mut state.filter).hint_text("🔍 Filter").desired_width(160.0));
        });
    });
    ui.add_space(4.0);
    if state.log.is_empty() {
        widgets::empty_state(ui, p, "No messages yet", "Connect, then send a message from the Message tab.");
        return;
    }
    let filter = state.filter.to_lowercase();
    egui::ScrollArea::vertical().id_salt(("ws_log", uid)).auto_shrink([false, false]).stick_to_bottom(true).show(
        ui,
        |ui| {
            for e in state.log.iter().filter(|e| filter.is_empty() || e.text.to_lowercase().contains(&filter)) {
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
                        .on_hover_text("Click to copy");
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
        let state = &mut self.tabs[idx].ws;
        match result {
            Ok(prepared) => {
                state.system(format!("Connecting to {}", ws::to_ws_url(&prepared.url)), false);
                let ctx = ctx.clone();
                state.handle = Some(ws::connect(&prepared, move || ctx.request_repaint()));
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
                    WsEvent::Message { direction, text, binary, at } => {
                        let text = if binary { format!("[binary] {text}") } else { text };
                        state.log.push(LogEntry { direction: Some(direction), text, at, error: false });
                    }
                    WsEvent::Error(e) => state.system(e, true),
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
