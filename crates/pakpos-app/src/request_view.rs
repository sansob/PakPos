//! Central workbench: tab strip, URL bar and request editor.

use crate::app::{Dialog, PakPosApp, ReqSection};
use crate::theme;
use crate::widgets;
use egui::{Margin, RichText, Sense, Stroke, Ui};
use pakpos_core::assertions::Operator;
use pakpos_core::model::{ApiKeyLocation, Assertion, Auth, Body, BodyKind, Extraction, RawLanguage, RequestKind};
use pakpos_core::vars::VarScope;
use pakpos_core::HttpMethod;

impl PakPosApp {
    pub fn workbench(&mut self, ui: &mut Ui) {
        let p = self.palette;
        egui::CentralPanel::default().frame(egui::Frame::new().fill(p.bg).inner_margin(Margin::ZERO)).show(ui, |ui| {
            self.tab_strip(ui);
            if self.tabs.is_empty() {
                return;
            }
            let screen_h = ui.ctx().content_rect().height();
            egui::Panel::bottom("response_panel")
                .resizable(true)
                .default_size((screen_h * 0.46).max(240.0))
                .min_size(120.0)
                .frame(
                    egui::Frame::new()
                        .fill(p.surface)
                        .inner_margin(Margin::symmetric(16, 10))
                        .stroke(Stroke::new(1.0, p.border)),
                )
                .show(ui, |ui| {
                    self.response_panel(ui);
                    // A resizable panel remembers the size its content used;
                    // claim the rest so an empty response doesn't shrink it.
                    ui.take_available_space();
                });
            egui::CentralPanel::default()
                .frame(egui::Frame::new().fill(p.surface).inner_margin(Margin::symmetric(16, 12)))
                .show(ui, |ui| self.request_editor(ui));
        });
    }

    fn tab_strip(&mut self, ui: &mut Ui) {
        let p = self.palette;
        let mut close = None;
        let mut select = None;
        egui::Frame::new().fill(p.bg).inner_margin(Margin { left: 8, right: 8, top: 6, bottom: 0 }).show(ui, |ui| {
            egui::ScrollArea::horizontal().id_salt("tabs").show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    for (i, tab) in self.tabs.iter().enumerate() {
                        let selected = i == self.active;
                        let fill = if selected { p.surface } else { p.bg };
                        let resp = egui::Frame::new()
                            .fill(fill)
                            .corner_radius(egui::CornerRadius { nw: 7, ne: 7, sw: 0, se: 0 })
                            .inner_margin(Margin::symmetric(10, 7))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 6.0;
                                    widgets::method_label(ui, &tab.request);
                                    let mut name = tab.request.name.clone();
                                    if name.chars().count() > 26 {
                                        name = format!("{}…", name.chars().take(26).collect::<String>());
                                    }
                                    let color = if selected { p.text } else { p.text_weak };
                                    ui.add(egui::Label::new(RichText::new(name).color(color)).selectable(false));
                                    if tab.dirty {
                                        ui.label(RichText::new("●").size(9.0).color(theme::ORANGE));
                                    }
                                    if ui
                                        .add(
                                            egui::Button::new(RichText::new("×").size(11.0).color(p.text_weak))
                                                .frame(false),
                                        )
                                        .on_hover_text("Close (Ctrl+W)")
                                        .clicked()
                                    {
                                        close = Some(i);
                                    }
                                });
                            })
                            .response;
                        if selected {
                            let r = resp.rect;
                            ui.painter().line_segment(
                                [r.left_top() + egui::vec2(6.0, 0.0), r.right_top() - egui::vec2(6.0, 0.0)],
                                Stroke::new(2.0, theme::ORANGE),
                            );
                        }
                        if ui.interact(resp.rect, egui::Id::new(("tab", tab.uid)), Sense::click()).clicked() {
                            select = Some(i);
                        }
                    }
                    if ui
                        .add(egui::Button::new(RichText::new(" + ").strong()).frame(false))
                        .on_hover_text("New tab (Ctrl+T)")
                        .clicked()
                    {
                        select = Some(usize::MAX);
                    }
                });
            });
        });
        if let Some(i) = select {
            if i == usize::MAX {
                self.new_blank_tab();
            } else {
                self.active = i;
            }
        }
        if let Some(i) = close {
            self.close_tab(i);
        }
    }

    fn request_editor(&mut self, ui: &mut Ui) {
        let p = self.palette;
        let idx = self.active;
        let scope = self.scope_for(&self.tabs[idx]);
        let inherited = self.inherited_auth(&self.tabs[idx]);
        let breadcrumb = self.tabs[idx].source.as_ref().and_then(|(c, _)| self.collection(c)).map(|c| c.name.clone());
        let mut send = false;
        let mut save = false;
        let mut snippet = false;
        let tokens = self.ws.tokens.clone();
        let mut oauth_action = None;
        let gql_key = scope.resolve(&self.tabs[idx].request.url);
        let gql_schema = self.graphql.schemas.get(&gql_key);
        let gql_loading = self.graphql.job.as_ref().is_some_and(|(k, _)| k == &gql_key);
        let mut gql_action = None;
        let mut ws_action = None;
        let mut script_action = None;
        let (script_trust, inherited_scripts, source_collection) = {
            let t = &self.tabs[idx];
            match t.source.as_ref().and_then(|(c, _)| self.collection(c)) {
                Some(c) => (
                    Some(self.ws.is_trusted(&c.id) || !c.has_scripts()),
                    c.script_chain(&t.request.id).len(),
                    Some(c.id.clone()),
                ),
                None => (None, 0, None),
            }
        };
        let oauth_job = self.oauth_job.as_ref();
        let tab = &mut self.tabs[idx];
        let mut changed = false;

        // Name + breadcrumb.
        ui.horizontal(|ui| {
            if let Some(c) = &breadcrumb {
                ui.label(RichText::new(format!("{c}  /")).color(p.text_weak));
            } else {
                ui.label(RichText::new("Unsaved  /").color(p.text_weak));
            }
            let name = egui::TextEdit::singleline(&mut tab.request.name)
                .frame(egui::Frame::NONE)
                .font(egui::TextStyle::Heading)
                .desired_width(ui.available_width() - 200.0);
            changed |= ui.add(name).changed();
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("</>").on_hover_text("Code snippet").clicked() {
                    snippet = true;
                }
                let save_label = if tab.dirty || tab.source.is_none() { "💾 Save" } else { "✔ Saved" };
                if ui.button(save_label).on_hover_text("Ctrl+S").clicked() {
                    save = true;
                }
            });
        });
        ui.add_space(6.0);

        // URL bar.
        egui::Frame::new()
            .fill(if p.dark { theme::INK } else { egui::Color32::WHITE })
            .stroke(Stroke::new(1.0, p.border))
            .corner_radius(8)
            .inner_margin(Margin::symmetric(6, 4))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let m = tab.request.method;
                    let _ = m;
                    let (badge, badge_color) = theme::badge(&tab.request);
                    let shown = if tab.request.kind.is_http() { tab.request.method.as_str() } else { badge };
                    egui::ComboBox::from_id_salt(("method", tab.uid))
                        .width(92.0)
                        .selected_text(RichText::new(shown).strong().monospace().color(badge_color))
                        .show_ui(ui, |ui| {
                            for method in HttpMethod::ALL {
                                let label = RichText::new(method.as_str())
                                    .strong()
                                    .monospace()
                                    .color(theme::method_color(method));
                                let selected = tab.request.kind.is_http() && tab.request.method == method;
                                if ui.selectable_label(selected, label).clicked() && !selected {
                                    tab.request.method = method;
                                    tab.request.kind = RequestKind::Http;
                                    changed = true;
                                }
                            }
                            ui.separator();
                            let ws = tab.request.kind == RequestKind::WebSocket;
                            if ui
                                .selectable_label(
                                    ws,
                                    RichText::new("WebSocket").strong().monospace().color(theme::TEAL),
                                )
                                .clicked()
                                && !ws
                            {
                                tab.request.kind = RequestKind::WebSocket;
                                if tab.section == ReqSection::Tests || tab.section == ReqSection::Extract {
                                    tab.section = ReqSection::Params;
                                }
                                changed = true;
                            }
                        });
                    let send_w = 96.0;
                    let url_w = ui.available_width() - send_w - 12.0;
                    let resp = widgets::var_text_edit(
                        ui,
                        &mut tab.url_edit,
                        &scope,
                        "Enter URL or paste a cURL command",
                        url_w,
                    );
                    if resp.changed() {
                        changed = true;
                        // Pasting a cURL command into the URL bar converts it.
                        if tab.url_edit.trim_start().starts_with("curl ") {
                            if let Ok(parsed) = pakpos_core::codegen::parse_curl(&tab.url_edit) {
                                let name = tab.request.name.clone();
                                let id = tab.request.id.clone();
                                tab.request = parsed;
                                tab.request.name = name;
                                tab.request.id = id;
                                tab.url_edit = compose_url(&tab.request);
                            }
                        } else {
                            apply_url_edit(&mut tab.request, &tab.url_edit);
                        }
                    }
                    if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        send = true;
                    }
                    if tab.request.kind == RequestKind::WebSocket {
                        let st = &tab.ws;
                        let label = if st.connected {
                            "Disconnect"
                        } else if st.connecting {
                            "Connecting…"
                        } else {
                            "Connect"
                        };
                        if widgets::primary_button(ui, label).clicked() {
                            ws_action = Some(if st.connected || st.connecting {
                                crate::ws_view::WsAction::Disconnect
                            } else {
                                crate::ws_view::WsAction::Connect
                            });
                        }
                        if send && !st.connected {
                            ws_action = Some(crate::ws_view::WsAction::Connect);
                        }
                        send = false;
                    } else {
                        let loading = matches!(tab.response, crate::app::ResponseState::Loading { .. });
                        if widgets::primary_button(ui, if loading { "Sending…" } else { "Send" })
                            .on_hover_text("Ctrl+Enter")
                            .clicked()
                        {
                            send = true;
                        }
                    }
                });
            });

        let missing = scope.unresolved(&tab.request.url);
        if !missing.is_empty() {
            ui.label(
                RichText::new(format!(
                    "⚠ Unresolved: {}",
                    missing.iter().map(|m| format!("{{{{{m}}}}}")).collect::<Vec<_>>().join(", ")
                ))
                .small()
                .color(theme::RED),
            );
        }
        ui.add_space(8.0);

        // Section tabs.
        let r = &tab.request;
        let body_label = match &r.body {
            Body::None => "Body".to_string(),
            b => format!("Body ({})", b.kind().label()),
        };
        let mut items = vec![
            (ReqSection::Params, widgets::counted("Params", r.params.iter().filter(|k| !k.is_blank()).count())),
            (ReqSection::Auth, "Auth".to_string()),
            (ReqSection::Headers, widgets::counted("Headers", r.headers.iter().filter(|k| !k.is_blank()).count())),
            (ReqSection::Body, body_label),
            (ReqSection::Tests, widgets::counted("Tests", r.assertions.len())),
            (ReqSection::Extract, widgets::counted("Extract", r.extractions.len())),
            (ReqSection::Scripts, if r.scripts.is_empty() { "Scripts".to_string() } else { "Scripts ●".to_string() }),
            (ReqSection::Docs, "Docs".to_string()),
        ];
        if r.kind == RequestKind::WebSocket {
            items.retain(|(s, _)| !matches!(s, ReqSection::Tests | ReqSection::Extract | ReqSection::Scripts));
            items[3].1 = "Message".to_string();
        }
        widgets::tab_bar(ui, &mut tab.section, &items);

        egui::ScrollArea::vertical().id_salt(("req_scroll", tab.uid)).auto_shrink([false, false]).show(ui, |ui| {
            match tab.section {
                ReqSection::Params => {
                    widgets::section_title(ui, "Query parameters");
                    if widgets::kv_table(ui, "params", &mut tab.request.params, &scope, "Parameter") {
                        changed = true;
                        tab.url_edit = compose_url(&tab.request);
                    }
                }
                ReqSection::Headers => {
                    widgets::section_title(ui, "Headers");
                    changed |= widgets::kv_table(ui, "headers", &mut tab.request.headers, &scope, "Header");
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Quick add:").small().color(p.text_weak));
                        for (k, v) in [
                            ("Accept", "application/json"),
                            ("Content-Type", "application/json"),
                            ("Cache-Control", "no-cache"),
                        ] {
                            if ui.small_button(format!("{k}: {v}")).clicked() {
                                widgets::trim_blank_rows(&mut tab.request.headers);
                                tab.request.headers.push(pakpos_core::KeyValue::new(k, v));
                                changed = true;
                            }
                        }
                    });
                }
                ReqSection::Auth => {
                    changed |= auth_editor(
                        ui,
                        &mut tab.request.auth,
                        inherited.as_ref(),
                        &scope,
                        &p,
                        &tokens,
                        oauth_job,
                        &mut oauth_action,
                    )
                }
                ReqSection::Body if tab.request.kind == RequestKind::WebSocket => {
                    changed |=
                        crate::ws_view::composer(ui, &mut tab.request.body, &tab.ws, tab.uid, &p, &mut ws_action);
                }
                ReqSection::Body => {
                    changed |= body_editor(
                        ui,
                        &mut tab.request.body,
                        &scope,
                        tab.uid,
                        gql_schema,
                        gql_loading,
                        &p,
                        &mut gql_action,
                    )
                }
                ReqSection::Tests => changed |= tests_editor(ui, &mut tab.request.assertions, &p),
                ReqSection::Extract => changed |= extract_editor(ui, &mut tab.request.extractions, &p),
                ReqSection::Scripts => {
                    changed |= crate::scripts_view::scripts_editor(
                        ui,
                        &mut tab.request.scripts,
                        tab.uid,
                        script_trust,
                        inherited_scripts,
                        &p,
                        &mut script_action,
                    );
                }
                ReqSection::Docs => {
                    widgets::section_title(ui, "Description (Markdown)");
                    changed |= ui
                        .add(
                            egui::TextEdit::multiline(&mut tab.request.description)
                                .desired_rows(10)
                                .desired_width(f32::INFINITY)
                                .hint_text("Describe what this request does, expected inputs, etc."),
                        )
                        .changed();
                }
            }
        });

        if changed {
            tab.dirty = true;
        }
        if let (Some(crate::scripts_view::ScriptAction::Trust), Some(cid)) = (script_action, source_collection) {
            self.trust_collection(&cid);
        }
        if let Some(a) = ws_action {
            let ctx = ui.ctx().clone();
            self.handle_ws_action(idx, a, &ctx);
        }
        match gql_action {
            Some(crate::graphql_view::GqlAction::FetchSchema) => {
                let ctx = ui.ctx().clone();
                self.fetch_graphql_schema(idx, &ctx);
            }
            Some(crate::graphql_view::GqlAction::Replace(q)) => {
                if let Body::GraphQl { query, .. } = &mut self.tabs[idx].request.body {
                    *query = q;
                    self.tabs[idx].dirty = true;
                }
            }
            None => {}
        }
        if let Some(a) = oauth_action {
            let ctx = ui.ctx().clone();
            self.handle_oauth_action(a, &ctx);
        }
        if send {
            let ctx = ui.ctx().clone();
            self.send(idx, &ctx);
        }
        if save {
            self.save_tab(idx);
        }
        if snippet {
            self.dialog = Some(Dialog::Snippet { lang: pakpos_core::codegen::SnippetLang::Curl });
        }
    }
}

/// URL shown in the URL bar: base URL plus enabled query params.
pub fn compose_url(req: &pakpos_core::Request) -> String {
    let q: Vec<String> = req
        .params
        .iter()
        .filter(|p| p.enabled && !p.is_blank())
        .map(|p| if p.value.is_empty() { p.key.clone() } else { format!("{}={}", p.key, p.value) })
        .collect();
    if q.is_empty() {
        req.url.clone()
    } else {
        format!("{}?{}", req.url, q.join("&"))
    }
}

/// Splits what the user typed into base URL + params (disabled params kept).
fn apply_url_edit(req: &mut pakpos_core::Request, text: &str) {
    let (base, query) = text.split_once('?').unwrap_or((text, ""));
    let parsed = query.split('&').filter(|p| !p.is_empty()).map(|p| match p.split_once('=') {
        Some((k, v)) => pakpos_core::KeyValue::new(k, v),
        None => pakpos_core::KeyValue::new(p, ""),
    });
    let disabled: Vec<_> = req.params.iter().filter(|p| !p.enabled && !p.is_blank()).cloned().collect();
    req.params = parsed.collect();
    req.params.extend(disabled);
    req.url = base.to_string();
}

/// The "Type" combo box shared by every auth kind.
fn auth_type_picker(ui: &mut Ui, changed: &mut bool, auth: &mut Auth) {
    ui.horizontal(|ui| {
        ui.label("Type");
        egui::ComboBox::from_id_salt("auth_type").width(190.0).selected_text(auth.label()).show_ui(ui, |ui| {
            for t in Auth::templates() {
                let selected = auth.same_variant(&t);
                if ui.selectable_label(selected, t.label()).clicked() && !selected {
                    *auth = t;
                    *changed = true;
                }
            }
        });
    });
}

#[allow(clippy::too_many_arguments)]
fn auth_editor(
    ui: &mut Ui,
    auth: &mut Auth,
    inherited: Option<&Auth>,
    scope: &VarScope,
    p: &crate::theme::Palette,
    tokens: &pakpos_core::oauth2::TokenStore,
    job: Option<&crate::oauth_view::OAuthJob>,
    oauth_action: &mut Option<crate::oauth_view::OAuthUiAction>,
) -> bool {
    let mut changed = false;
    if matches!(auth, Auth::OAuth2(_)) {
        auth_type_picker(ui, &mut changed, auth);
        if let Auth::OAuth2(cfg) = auth {
            ui.add_space(8.0);
            changed |= crate::oauth_view::oauth_form(ui, cfg, scope, p);
            *oauth_action = crate::oauth_view::token_panel(ui, cfg, scope, tokens, job, p);
        }
        return changed;
    }
    let parent_oauth = match (&*auth, inherited) {
        (Auth::Inherit, Some(Auth::OAuth2(parent))) => Some(parent.clone()),
        _ => None,
    };
    if let Some(parent) = parent_oauth {
        auth_type_picker(ui, &mut changed, auth);
        if matches!(auth, Auth::Inherit) {
            ui.add_space(8.0);
            ui.label(RichText::new("Uses OAuth 2.0 from the parent folder / collection.").color(p.text_weak));
            *oauth_action = crate::oauth_view::token_panel(ui, &parent, scope, tokens, job, p);
        }
        return changed;
    }
    ui.horizontal(|ui| {
        ui.label("Type");
        egui::ComboBox::from_id_salt("auth_type").width(190.0).selected_text(auth.label()).show_ui(ui, |ui| {
            for t in Auth::templates() {
                let selected = auth.same_variant(&t);
                if ui.selectable_label(selected, t.label()).clicked() && !selected {
                    *auth = t;
                    changed = true;
                }
            }
        });
    });
    ui.add_space(8.0);
    let w = 360.0;
    egui::Grid::new("auth_grid").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| match auth {
        Auth::Inherit => {
            let text = match inherited {
                Some(a) if !matches!(a, Auth::None | Auth::Inherit) => {
                    format!("Uses {} from the parent folder / collection.", a.label())
                }
                Some(_) => "The parent folder / collection has no auth.".to_string(),
                None => "Not in a collection: no auth will be sent.".to_string(),
            };
            ui.label(RichText::new(text).color(p.text_weak));
            ui.end_row();
        }
        Auth::None => {
            ui.label(RichText::new("This request does not use any authorization.").color(p.text_weak));
            ui.end_row();
        }
        Auth::OAuth2(_) => {} // handled above
        Auth::Basic { username, password } => {
            ui.label("Username");
            changed |= widgets::var_text_edit(ui, username, scope, "", w).changed();
            ui.end_row();
            ui.label("Password");
            changed |= ui.add(egui::TextEdit::singleline(password).password(true).desired_width(w)).changed();
            ui.end_row();
        }
        Auth::Bearer { token } => {
            ui.label("Token");
            changed |= widgets::var_text_edit(ui, token, scope, "{{token}}", w).changed();
            ui.end_row();
        }
        Auth::ApiKey { key, value, location } => {
            ui.label("Key");
            changed |= widgets::var_text_edit(ui, key, scope, "X-API-Key", w).changed();
            ui.end_row();
            ui.label("Value");
            changed |= widgets::var_text_edit(ui, value, scope, "", w).changed();
            ui.end_row();
            ui.label("Add to");
            ui.horizontal(|ui| {
                changed |= ui.radio_value(location, ApiKeyLocation::Header, "Header").changed();
                changed |= ui.radio_value(location, ApiKeyLocation::Query, "Query params").changed();
            });
            ui.end_row();
        }
    });
    changed
}

#[allow(clippy::too_many_arguments)]
fn body_editor(
    ui: &mut Ui,
    body: &mut Body,
    scope: &VarScope,
    uid: u64,
    gql_schema: Option<&Result<pakpos_core::graphql::Schema, String>>,
    gql_loading: bool,
    p: &crate::theme::Palette,
    gql_action: &mut Option<crate::graphql_view::GqlAction>,
) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        let current = body.kind();
        for kind in BodyKind::ALL {
            if ui.radio(current == kind, kind.label()).clicked() && current != kind {
                *body = body.convert_to(kind);
                changed = true;
            }
        }
        if let Body::Raw { language, content } = body {
            ui.separator();
            egui::ComboBox::from_id_salt("raw_lang").width(100.0).selected_text(language.label()).show_ui(ui, |ui| {
                for l in RawLanguage::ALL {
                    changed |= ui.selectable_value(language, l, l.label()).changed();
                }
            });
            if *language == RawLanguage::Json && ui.button("Beautify").clicked() {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(content) {
                    if let Ok(pretty) = serde_json::to_string_pretty(&v) {
                        *content = pretty;
                        changed = true;
                    }
                }
            }
        }
    });
    ui.add_space(6.0);
    match body {
        Body::None => {
            ui.label(RichText::new("This request does not have a body.").color(ui.visuals().weak_text_color()));
        }
        Body::Raw { content, language } => {
            let lang = match language {
                RawLanguage::Json => "json",
                RawLanguage::Xml | RawLanguage::Html => "html",
                RawLanguage::Javascript => "js",
                RawLanguage::Text => "txt",
            };
            let theme = egui_extras::syntax_highlighting::CodeTheme::from_style(ui.style());
            let mut layouter = |ui: &Ui, buf: &dyn egui::TextBuffer, wrap_width: f32| {
                let mut job =
                    egui_extras::syntax_highlighting::highlight(ui.ctx(), ui.style(), &theme, buf.as_str(), lang);
                job.wrap.max_width = wrap_width;
                ui.ctx().fonts_mut(|f| f.layout_job(job))
            };
            changed |= ui
                .add(
                    egui::TextEdit::multiline(content)
                        .id_salt(("body", uid))
                        .code_editor()
                        .desired_rows(14)
                        .desired_width(f32::INFINITY)
                        .hint_text("{ \"name\": \"{{username}}\" }")
                        .layouter(&mut layouter),
                )
                .changed();
            if *language == RawLanguage::Json && !content.trim().is_empty() {
                let resolved = scope.resolve(content);
                if let Err(e) = serde_json::from_str::<serde_json::Value>(&resolved) {
                    ui.label(RichText::new(format!("⚠ Invalid JSON: {e}")).small().color(theme::YELLOW));
                }
            }
        }
        Body::FormUrlEncoded { fields } => changed |= widgets::kv_table(ui, "urlencoded", fields, scope, "Field"),
        Body::Multipart { fields } => changed |= widgets::form_table(ui, fields, scope),
        Body::GraphQl { query, variables, operation_name } => {
            changed |= crate::graphql_view::editor(
                ui,
                query,
                variables,
                operation_name,
                uid,
                gql_schema,
                gql_loading,
                p,
                gql_action,
            );
        }
        Body::Binary { path } => {
            ui.horizontal(|ui| {
                changed |=
                    ui.add(egui::TextEdit::singleline(path).hint_text("path/to/file").desired_width(420.0)).changed();
                if ui.button("Browse…").clicked() {
                    if let Some(p) = rfd::FileDialog::new().pick_file() {
                        *path = p.display().to_string();
                        changed = true;
                    }
                }
            });
        }
    }
    changed
}

fn tests_editor(ui: &mut Ui, tests: &mut Vec<Assertion>, p: &crate::theme::Palette) -> bool {
    let mut changed = false;
    ui.label(
        RichText::new(
            "Declarative tests run after every response. Targets: status, time, size, body, header.<Name>, json.<path>",
        )
        .small()
        .color(p.text_weak),
    );
    ui.add_space(6.0);
    let mut remove = None;
    for (i, t) in tests.iter_mut().enumerate() {
        ui.push_id(("test", i), |ui| {
            ui.horizontal(|ui| {
                changed |= ui.checkbox(&mut t.enabled, "").changed();
                changed |= ui
                    .add(egui::TextEdit::singleline(&mut t.target).desired_width(220.0).hint_text("json.data.id"))
                    .changed();
                egui::ComboBox::from_id_salt("op").width(120.0).selected_text(t.op.label()).show_ui(ui, |ui| {
                    for op in Operator::ALL {
                        changed |= ui.selectable_value(&mut t.op, op, op.label()).changed();
                    }
                });
                if t.op.needs_expected() {
                    changed |= ui
                        .add(egui::TextEdit::singleline(&mut t.expected).desired_width(200.0).hint_text("expected"))
                        .changed();
                }
                if widgets::ghost_button(ui, "×", "Remove").clicked() {
                    remove = Some(i);
                }
            })
        });
    }
    if let Some(i) = remove {
        tests.remove(i);
        changed = true;
    }
    ui.add_space(6.0);
    ui.horizontal_wrapped(|ui| {
        if ui.button("+ Add test").clicked() {
            tests.push(Assertion { target: String::new(), expected: String::new(), ..Default::default() });
            changed = true;
        }
        ui.label(RichText::new("Presets:").small().color(p.text_weak));
        let presets: [(&str, Operator, &str, &str); 4] = [
            ("status", Operator::Equals, "200", "Status is 200"),
            ("time", Operator::LessThan, "1000", "Time < 1s"),
            ("header.Content-Type", Operator::Contains, "json", "Is JSON"),
            ("body", Operator::NotContains, "error", "No \"error\""),
        ];
        for (target, op, expected, label) in presets {
            if ui.small_button(label).clicked() {
                tests.push(Assertion { enabled: true, target: target.into(), op, expected: expected.into() });
                changed = true;
            }
        }
    });
    changed
}

fn extract_editor(ui: &mut Ui, ex: &mut Vec<Extraction>, p: &crate::theme::Palette) -> bool {
    let mut changed = false;
    ui.label(
        RichText::new("Save values from the response into the active environment, e.g. json.token → {{token}}. Great for login flows.")
            .small()
            .color(p.text_weak),
    );
    ui.add_space(6.0);
    let mut remove = None;
    for (i, e) in ex.iter_mut().enumerate() {
        ui.push_id(("extract", i), |ui| {
            ui.horizontal(|ui| {
                changed |= ui.checkbox(&mut e.enabled, "").changed();
                changed |= ui
                    .add(egui::TextEdit::singleline(&mut e.source).desired_width(240.0).hint_text("json.data.token"))
                    .changed();
                ui.label("→");
                changed |= ui
                    .add(egui::TextEdit::singleline(&mut e.variable).desired_width(160.0).hint_text("variable"))
                    .changed();
                if widgets::ghost_button(ui, "×", "Remove").clicked() {
                    remove = Some(i);
                }
            })
        });
    }
    if let Some(i) = remove {
        ex.remove(i);
        changed = true;
    }
    if ui.button("+ Add extraction").clicked() {
        ex.push(Extraction { enabled: true, ..Default::default() });
        changed = true;
    }
    changed
}
