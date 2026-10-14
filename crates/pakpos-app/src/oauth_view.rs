//! OAuth 2.0 editor and token handling in the desktop app.

use crate::app::PakPosApp;
use crate::theme::{self, Palette};
use crate::widgets;
use egui::{RichText, Ui};
use pakpos_core::model::{ClientAuthMethod, OAuth2Config, OAuth2Grant};
use pakpos_core::oauth2::{self, OAuthToken, TokenStore};
use pakpos_core::vars::VarScope;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

/// What the user asked for in the OAuth panel.
pub enum OAuthUiAction {
    Fetch(Box<OAuth2Config>),
    Clear(String),
}

pub enum OAuthMsg {
    /// Authorization URL opened in the browser (shown in case it didn't open).
    Url(String),
    Done(Result<(String, OAuthToken), String>),
}

/// A token request running in the background.
pub struct OAuthJob {
    pub key: String,
    pub rx: Receiver<OAuthMsg>,
    pub started: Instant,
    pub auth_url: Option<String>,
}

/// Editable OAuth 2.0 settings.
pub fn oauth_form(ui: &mut Ui, cfg: &mut OAuth2Config, scope: &VarScope, p: &Palette) -> bool {
    let mut changed = false;
    let w = 380.0;
    egui::Grid::new("oauth_grid").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
        ui.label("Grant type");
        egui::ComboBox::from_id_salt("oauth_grant").width(200.0).selected_text(cfg.grant.label()).show_ui(ui, |ui| {
            for g in OAuth2Grant::ALL {
                changed |= ui.selectable_value(&mut cfg.grant, g, g.label()).changed();
            }
        });
        ui.end_row();
        if cfg.grant == OAuth2Grant::AuthorizationCode {
            ui.label("Authorization URL");
            changed |=
                widgets::var_text_edit(ui, &mut cfg.auth_url, scope, "https://id.example.com/authorize", w).changed();
            ui.end_row();
        }
        ui.label("Token URL");
        changed |=
            widgets::var_text_edit(ui, &mut cfg.token_url, scope, "https://id.example.com/oauth/token", w).changed();
        ui.end_row();
        ui.label("Client ID");
        changed |= widgets::var_text_edit(ui, &mut cfg.client_id, scope, "{{client_id}}", w).changed();
        ui.end_row();
        ui.label("Client secret");
        changed |=
            widgets::var_text_edit(ui, &mut cfg.client_secret, scope, "optional for public clients", w).changed();
        ui.end_row();
        if cfg.grant == OAuth2Grant::Password {
            ui.label("Username");
            changed |= widgets::var_text_edit(ui, &mut cfg.username, scope, "", w).changed();
            ui.end_row();
            ui.label("Password");
            changed |= ui
                .add(
                    egui::TextEdit::singleline(&mut cfg.password)
                        .min_size(egui::vec2(0.0, crate::theme::CONTROL_HEIGHT))
                        .vertical_align(egui::Align::Center)
                        .password(true)
                        .desired_width(w),
                )
                .changed();
            ui.end_row();
        }
        ui.label("Scope");
        changed |= widgets::var_text_edit(ui, &mut cfg.scope, scope, "openid profile read:items", w).changed();
        ui.end_row();
        ui.label("Audience");
        changed |= widgets::var_text_edit(ui, &mut cfg.audience, scope, "optional (Auth0 and others)", w).changed();
        ui.end_row();
        if cfg.grant == OAuth2Grant::AuthorizationCode {
            ui.label("Redirect URI");
            changed |=
                widgets::var_text_edit(ui, &mut cfg.redirect_uri, scope, "http://127.0.0.1:7777/callback", w).changed();
            ui.end_row();
            ui.label("PKCE (S256)");
            changed |= ui.checkbox(&mut cfg.use_pkce, "recommended").changed();
            ui.end_row();
        }
        ui.label("Client authentication");
        ui.horizontal(|ui| {
            changed |= ui.radio_value(&mut cfg.client_auth, ClientAuthMethod::Header, "Basic header").changed();
            changed |= ui.radio_value(&mut cfg.client_auth, ClientAuthMethod::Body, "In body").changed();
        });
        ui.end_row();
        ui.label("Header prefix");
        changed |= ui
            .add(
                egui::TextEdit::singleline(&mut cfg.header_prefix)
                    .min_size(egui::vec2(0.0, crate::theme::CONTROL_HEIGHT))
                    .vertical_align(egui::Align::Center)
                    .desired_width(120.0),
            )
            .changed();
        ui.end_row();
    });
    if cfg.grant == OAuth2Grant::AuthorizationCode {
        ui.label(
            RichText::new(
                "Register the redirect URI with your provider. PakPos listens on it locally while you log in.",
            )
            .small()
            .color(p.text_weak),
        );
    }
    changed
}

/// Token status + "Get new access token" / "Clear".
pub fn token_panel(
    ui: &mut Ui,
    cfg: &OAuth2Config,
    scope: &VarScope,
    tokens: &TokenStore,
    job: Option<&OAuthJob>,
    p: &Palette,
) -> Option<OAuthUiAction> {
    let resolved = cfg.resolved(scope);
    let key = resolved.cache_key();
    let mut action = None;
    ui.add_space(8.0);
    egui::Frame::new().fill(p.surface_alt).corner_radius(8).inner_margin(12).show(ui, |ui| {
        ui.set_width(ui.available_width());
        let busy = job.is_some_and(|j| j.key == key);
        ui.horizontal(|ui| {
            match tokens.get(&key) {
                Some(t) => {
                    let (label, c) =
                        if t.is_fresh() { ("Token ready", theme::GREEN) } else { ("Token expired", theme::YELLOW) };
                    widgets::pill(ui, label, c, c.gamma_multiply(0.16));
                    let shown: String = t.access_token.chars().take(12).collect();
                    ui.label(RichText::new(format!("{shown}…")).monospace().color(p.text_weak));
                    if let Some(s) = t.expires_in() {
                        let txt = if s > 0 { format!("expires in {}m {}s", s / 60, s % 60) } else { "expired".into() };
                        ui.label(RichText::new(txt).small().color(p.text_weak));
                    }
                    if t.refresh_token.is_some() {
                        ui.label(RichText::new("· refreshable").small().color(p.text_weak));
                    }
                }
                None => {
                    widgets::pill(ui, "No token", p.text_weak, p.surface);
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if tokens.contains_key(&key) && ui.button("Clear").clicked() {
                    action = Some(OAuthUiAction::Clear(key.clone()));
                }
                if busy {
                    ui.add(egui::Spinner::new().color(theme::ORANGE));
                } else if widgets::primary_button(ui, "Get new access token").clicked() {
                    action = Some(OAuthUiAction::Fetch(Box::new(resolved.clone())));
                }
            });
        });
        if let Some(j) = job.filter(|j| j.key == key) {
            if let Some(url) = &j.auth_url {
                ui.add_space(4.0);
                ui.label(
                    RichText::new("Complete the login in your browser. If it didn't open:").small().color(p.text_weak),
                );
                ui.horizontal(|ui| {
                    ui.hyperlink_to(RichText::new("open the login page").small(), url);
                    if ui.button("Copy link").clicked() {
                        ui.ctx().copy_text(url.clone());
                    }
                });
            }
        }
        if !busy {
            let hint = if cfg.grant.is_interactive() {
                "Tokens are stored locally in tokens.json, never in the collection file."
            } else {
                "Fetched automatically when you Send (and refreshed when expired). Stored locally in tokens.json."
            };
            ui.label(RichText::new(hint).small().color(p.text_weak));
        }
    });
    action
}

impl PakPosApp {
    pub fn handle_oauth_action(&mut self, action: OAuthUiAction, ctx: &egui::Context) {
        match action {
            OAuthUiAction::Clear(key) => {
                self.ws.tokens.remove(&key);
                if let Err(e) = self.ws.save_tokens() {
                    self.toast_error(e.to_string());
                }
            }
            OAuthUiAction::Fetch(cfg) => {
                let key = cfg.cache_key();
                let settings = self.ws.settings.http.clone();
                let (tx, rx) = mpsc::channel();
                let ctx = ctx.clone();
                std::thread::spawn(move || {
                    let result = if cfg.grant.is_interactive() {
                        oauth2::begin_authorization(&cfg).and_then(|pending| {
                            let _ = tx.send(OAuthMsg::Url(pending.url.clone()));
                            ctx.request_repaint();
                            let _ = webbrowser::open(&pending.url);
                            pending.finish(&cfg, &settings, Duration::from_secs(300))
                        })
                    } else {
                        oauth2::fetch_token(&cfg, &settings)
                    };
                    let _ = tx.send(OAuthMsg::Done(result.map(|t| (cfg.cache_key(), t)).map_err(|e| e.to_string())));
                    ctx.request_repaint();
                });
                self.oauth_job = Some(OAuthJob { key, rx, started: Instant::now(), auth_url: None });
            }
        }
    }

    pub fn poll_oauth(&mut self) {
        let Some(job) = &mut self.oauth_job else { return };
        let mut done = None;
        while let Ok(msg) = job.rx.try_recv() {
            match msg {
                OAuthMsg::Url(u) => job.auth_url = Some(u),
                OAuthMsg::Done(r) => done = Some(r),
            }
        }
        if let Some(result) = done {
            self.oauth_job = None;
            match result {
                Ok((key, token)) => {
                    self.ws.tokens.insert(key, token);
                    match self.ws.save_tokens() {
                        Ok(()) => self.toast("Access token received"),
                        Err(e) => self.toast_error(e.to_string()),
                    }
                }
                Err(e) => self.toast_error(e),
            }
        } else if job.started.elapsed() > Duration::from_secs(310) {
            self.oauth_job = None;
        }
    }
}
