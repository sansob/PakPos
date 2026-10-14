//! Modal dialogs and the collection runner window.

use crate::app::{clean_request, DeleteTarget, Dialog, PakPosApp, RenameTarget};
use crate::theme;
use crate::widgets;
use egui::{Id, RichText};
use pakpos_core::codegen::{self, SnippetLang};
use pakpos_core::model::Item;
use pakpos_core::storage::Theme;
use pakpos_core::Collection;

fn modal_frame(p: &theme::Palette) -> egui::Frame {
    egui::Frame::new().fill(p.surface).stroke(egui::Stroke::new(1.0, p.border)).corner_radius(12).inner_margin(20)
}

fn title(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).heading().strong());
    ui.add_space(8.0);
}

impl PakPosApp {
    pub fn dialogs(&mut self, ctx: &egui::Context) {
        let Some(mut dialog) = self.dialog.take() else { return };
        let p = self.palette;
        let mut keep = true;

        match &mut dialog {
            Dialog::ImportCurl { text, error } => {
                let r = egui::Modal::new(Id::new("import_curl")).frame(modal_frame(&p)).show(ctx, |ui| {
                    ui.set_width(620.0);
                    title(ui, "Import cURL");
                    ui.label(
                        RichText::new("Paste a command from your terminal or a browser's “Copy as cURL”.")
                            .color(p.text_weak),
                    );
                    ui.add_space(6.0);
                    ui.add(
                        egui::TextEdit::multiline(text)
                            .code_editor()
                            .desired_rows(10)
                            .desired_width(f32::INFINITY)
                            .hint_text("curl https://api.example.com/users -H 'Accept: application/json'"),
                    );
                    if let Some(e) = error {
                        ui.label(RichText::new(e.as_str()).color(theme::RED));
                    }
                    ui.add_space(8.0);
                    let mut result = None;
                    ui.horizontal(|ui| {
                        if widgets::primary_button(ui, "Import").clicked() {
                            result = Some(codegen::parse_curl(text));
                        }
                        if ui.button("Cancel").clicked() {
                            keep = false;
                        }
                    });
                    result
                });
                if r.should_close() {
                    keep = false;
                }
                match r.inner {
                    Some(Ok(req)) => {
                        self.active = self.push_tab(req, None);
                        self.tabs[self.active].dirty = true;
                        keep = false;
                        self.toast("cURL imported into a new tab");
                    }
                    Some(Err(e)) => *error = Some(e.to_string()),
                    None => {}
                }
            }
            Dialog::SaveRequest { tab, collection, new_collection, name } => {
                let r = egui::Modal::new(Id::new("save_request")).frame(modal_frame(&p)).show(ctx, |ui| {
                    ui.set_width(440.0);
                    title(ui, "Save request");
                    ui.label("Name");
                    ui.add(
                        egui::TextEdit::singleline(name)
                            .min_size(egui::vec2(0.0, crate::theme::CONTROL_HEIGHT))
                            .vertical_align(egui::Align::Center)
                            .desired_width(f32::INFINITY),
                    );
                    ui.add_space(8.0);
                    ui.label("Collection");
                    egui::ScrollArea::vertical().max_height(180.0).show(ui, |ui| {
                        for c in &self.ws.collections {
                            ui.radio_value(collection, Some(c.id.clone()), &c.name);
                        }
                        ui.radio_value(collection, None, "New collection…");
                    });
                    if collection.is_none() {
                        ui.add(
                            egui::TextEdit::singleline(new_collection)
                                .min_size(egui::vec2(0.0, crate::theme::CONTROL_HEIGHT))
                                .vertical_align(egui::Align::Center)
                                .hint_text("Collection name")
                                .desired_width(f32::INFINITY),
                        );
                    }
                    ui.add_space(10.0);
                    let mut save = false;
                    ui.horizontal(|ui| {
                        save = widgets::primary_button(ui, "Save").clicked();
                        if ui.button("Cancel").clicked() {
                            keep = false;
                        }
                    });
                    save
                });
                if r.should_close() {
                    keep = false;
                }
                if r.inner && *tab < self.tabs.len() {
                    let cid = match collection.clone() {
                        Some(id) => id,
                        None => {
                            let n =
                                if new_collection.trim().is_empty() { "My collection" } else { new_collection.trim() };
                            let c = Collection::new(n);
                            let id = c.id.clone();
                            self.open_nodes.insert(id.clone());
                            self.ws.collections.push(c);
                            // Created here, so its scripts are the user's own.
                            self.ws.settings.trusted_collections.push(id.clone());
                            self.persist_settings();
                            id
                        }
                    };
                    let t = &mut self.tabs[*tab];
                    let mut req = t.request.clone();
                    req.id = pakpos_core::model::new_id();
                    req.name = if name.trim().is_empty() { "Untitled request".into() } else { name.trim().to_string() };
                    clean_request(&mut req);
                    t.request.id = req.id.clone();
                    t.request.name = req.name.clone();
                    t.source = Some((cid.clone(), req.id.clone()));
                    t.dirty = false;
                    if let Some(c) = self.collection_mut(&cid) {
                        c.items.push(Item::Request(req));
                    }
                    self.persist_collection(&cid);
                    self.toast("Request saved");
                    keep = false;
                }
            }
            Dialog::NewCollection { name } => {
                let r = egui::Modal::new(Id::new("new_collection")).frame(modal_frame(&p)).show(ctx, |ui| {
                    ui.set_width(380.0);
                    title(ui, "New collection");
                    let resp = ui.add(
                        egui::TextEdit::singleline(name)
                            .min_size(egui::vec2(0.0, crate::theme::CONTROL_HEIGHT))
                            .vertical_align(egui::Align::Center)
                            .hint_text("e.g. Payments API")
                            .desired_width(f32::INFINITY),
                    );
                    resp.request_focus();
                    ui.add_space(10.0);
                    let mut ok = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    ui.horizontal(|ui| {
                        ok |= widgets::primary_button(ui, "Create").clicked();
                        if ui.button("Cancel").clicked() {
                            keep = false;
                        }
                    });
                    ok
                });
                if r.should_close() {
                    keep = false;
                }
                if r.inner && !name.trim().is_empty() {
                    let c = Collection::new(name.trim());
                    self.open_nodes.insert(c.id.clone());
                    let id = c.id.clone();
                    self.ws.collections.push(c);
                    self.persist_collection(&id);
                    self.ws.settings.trusted_collections.push(id.clone());
                    self.persist_settings();
                    keep = false;
                }
            }
            Dialog::Rename { target, name } => {
                let r = egui::Modal::new(Id::new("rename")).frame(modal_frame(&p)).show(ctx, |ui| {
                    ui.set_width(380.0);
                    title(ui, "Rename");
                    let resp = ui.add(
                        egui::TextEdit::singleline(name)
                            .min_size(egui::vec2(0.0, crate::theme::CONTROL_HEIGHT))
                            .vertical_align(egui::Align::Center)
                            .desired_width(f32::INFINITY),
                    );
                    resp.request_focus();
                    ui.add_space(10.0);
                    let mut ok = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    ui.horizontal(|ui| {
                        ok |= widgets::primary_button(ui, "Rename").clicked();
                        if ui.button("Cancel").clicked() {
                            keep = false;
                        }
                    });
                    ok
                });
                if r.should_close() {
                    keep = false;
                }
                if r.inner && !name.trim().is_empty() {
                    self.rename(target.clone(), name.trim().to_string());
                    keep = false;
                }
            }
            Dialog::ConfirmDelete { target, label } => {
                let r = egui::Modal::new(Id::new("confirm_delete")).frame(modal_frame(&p)).show(ctx, |ui| {
                    ui.set_width(380.0);
                    title(ui, "Delete?");
                    ui.label(format!("“{label}” will be deleted permanently."));
                    ui.add_space(12.0);
                    let mut ok = false;
                    ui.horizontal(|ui| {
                        ok = ui
                            .add(
                                egui::Button::new(RichText::new("Delete").color(egui::Color32::WHITE).strong())
                                    .fill(theme::RED),
                            )
                            .clicked();
                        if ui.button("Cancel").clicked() {
                            keep = false;
                        }
                    });
                    ok
                });
                if r.should_close() {
                    keep = false;
                }
                if r.inner {
                    self.delete(target.clone());
                    keep = false;
                }
            }
            Dialog::CollectionSettings { id, tab } => {
                keep = self.collection_window(ctx, id, tab);
            }
            Dialog::Environments { selected } => {
                keep = self.environments_window(ctx, selected);
            }
            Dialog::Settings => {
                let r = egui::Modal::new(Id::new("settings")).frame(modal_frame(&p)).show(ctx, |ui| {
                    ui.set_width(480.0);
                    title(ui, "Settings");
                    let mut changed = false;
                    let s = &mut self.ws.settings;
                    egui::Grid::new("settings_grid").num_columns(2).spacing([16.0, 10.0]).show(ui, |ui| {
                        ui.label("Theme");
                        ui.horizontal(|ui| {
                            changed |= ui.selectable_value(&mut s.theme, Theme::Dark, "🌙 Dark").changed();
                            changed |= ui.selectable_value(&mut s.theme, Theme::Light, "☀ Light").changed();
                        });
                        ui.end_row();
                        ui.label("Font size");
                        changed |= ui.add(egui::Slider::new(&mut s.font_scale_percent, 80..=160).suffix("%")).changed();
                        ui.end_row();
                        ui.label("Request timeout");
                        changed |= ui
                            .add(egui::DragValue::new(&mut s.http.timeout_secs).range(1..=600).suffix(" s"))
                            .changed();
                        ui.end_row();
                        ui.label("Follow redirects");
                        ui.horizontal(|ui| {
                            changed |= ui.checkbox(&mut s.http.follow_redirects, "").changed();
                            changed |= ui
                                .add_enabled(
                                    s.http.follow_redirects,
                                    egui::DragValue::new(&mut s.http.max_redirects).range(0..=50).prefix("max "),
                                )
                                .changed();
                        });
                        ui.end_row();
                        ui.label("Verify TLS certificates");
                        changed |= ui.checkbox(&mut s.http.verify_tls, "").changed();
                        ui.end_row();
                        ui.label("Proxy");
                        changed |= ui
                            .add(
                                egui::TextEdit::singleline(&mut s.http.proxy)
                                    .min_size(egui::vec2(0.0, crate::theme::CONTROL_HEIGHT))
                                    .vertical_align(egui::Align::Center)
                                    .hint_text("http://127.0.0.1:8080")
                                    .desired_width(260.0),
                            )
                            .changed();
                        ui.end_row();
                        ui.label("User-Agent");
                        changed |= ui
                            .add(
                                egui::TextEdit::singleline(&mut s.http.user_agent)
                                    .min_size(egui::vec2(0.0, crate::theme::CONTROL_HEIGHT))
                                    .vertical_align(egui::Align::Center)
                                    .desired_width(260.0),
                            )
                            .changed();
                        ui.end_row();
                    });
                    if !s.http.verify_tls {
                        ui.label(
                            RichText::new("⚠ TLS verification is off: only use this for local development.")
                                .small()
                                .color(theme::YELLOW),
                        );
                    }
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new(format!("Workspace folder: {}", self.ws.root.display()))
                            .small()
                            .color(p.text_weak),
                    );
                    ui.label(
                        RichText::new("Set PAKPOS_HOME to use another folder (e.g. a git repo shared with your team).")
                            .small()
                            .color(p.text_weak),
                    );
                    ui.add_space(10.0);
                    if widgets::primary_button(ui, "Done").clicked() {
                        keep = false;
                    }
                    changed
                });
                if r.inner {
                    self.persist_settings();
                }
                if r.should_close() {
                    keep = false;
                }
            }
            Dialog::Snippet { lang } => {
                let tab = &self.tabs[self.active];
                let scope = self.scope_for(tab);
                let inherited = self.inherited_auth(tab);
                let mut req = tab.request.clone();
                clean_request(&mut req);
                let code = match pakpos_core::http::prepare(&req, &scope, inherited.as_ref()) {
                    Ok(prep) => codegen::snippet(*lang, &prep),
                    Err(e) => format!("# {e}"),
                };
                let r = egui::Modal::new(Id::new("snippet")).frame(modal_frame(&p)).show(ctx, |ui| {
                    ui.set_width(680.0);
                    title(ui, "Code snippet");
                    ui.horizontal(|ui| {
                        for l in SnippetLang::ALL {
                            ui.selectable_value(lang, l, l.label());
                        }
                    });
                    ui.add_space(6.0);
                    let theme = egui_extras::syntax_highlighting::CodeTheme::from_style(ui.style());
                    let syntax = match lang {
                        SnippetLang::Python => "py",
                        SnippetLang::JavaScript => "js",
                        _ => "sh",
                    };
                    egui::ScrollArea::vertical().max_height(380.0).show(ui, |ui| {
                        egui::Frame::new().fill(ui.visuals().code_bg_color).corner_radius(8).inner_margin(10).show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            egui_extras::syntax_highlighting::code_view_ui(ui, &theme, &code, syntax);
                        });
                    });
                    ui.label(RichText::new("Variables are resolved with the active environment. Secrets are included — share with care.").small().color(p.text_weak));
                    ui.add_space(8.0);
                    let mut copy = false;
                    ui.horizontal(|ui| {
                        copy = widgets::primary_button(ui, "Copy").clicked();
                        if ui.button("Close").clicked() {
                            keep = false;
                        }
                    });
                    copy
                });
                if r.inner {
                    ctx.copy_text(code);
                    self.toast("Snippet copied");
                }
                if r.should_close() {
                    keep = false;
                }
            }
            Dialog::About => {
                let r = egui::Modal::new(Id::new("about")).frame(modal_frame(&p)).show(ctx, |ui| {
                    ui.set_width(420.0);
                    ui.vertical_centered(|ui| {
                        ui.horizontal(|ui| {
                            ui.add_space(130.0);
                            ui.label(RichText::new("Pak").size(30.0).strong().color(theme::ORANGE));
                            ui.add_space(-8.0);
                            ui.label(RichText::new("Pos").size(30.0).strong().color(if p.dark {
                                egui::Color32::WHITE
                            } else {
                                theme::NAVY_700
                            }));
                        });
                        ui.label(format!("Version {}", pakpos_core::VERSION));
                        ui.add_space(6.0);
                        ui.label("A fast, native, open-source API client written in Rust.");
                        ui.hyperlink_to("github.com/sansob/PakPos", "https://github.com/sansob/PakPos");
                        ui.label(RichText::new("Licensed under MIT or Apache-2.0").small().color(p.text_weak));
                        ui.add_space(10.0);
                        if ui.button("Close").clicked() {
                            keep = false;
                        }
                    });
                });
                if r.should_close() {
                    keep = false;
                }
            }
        }

        if keep && self.dialog.is_none() {
            self.dialog = Some(dialog);
        }
    }

    /// Environment + globals editor. Returns false when closed.
    fn environments_window(&mut self, ctx: &egui::Context, selected: &mut Option<String>) -> bool {
        let p = self.palette;
        let mut open = true;
        let mut changed = false;
        let mut new_env = false;
        egui::Window::new("Environments")
            .id(Id::new("env_window"))
            .open(&mut open)
            .default_size([760.0, 460.0])
            .pivot(egui::Align2::CENTER_CENTER)
            .default_pos(ctx.content_rect().center())
            .collapsible(false)
            .frame(modal_frame(&p))
            .show(ctx, |ui| {
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(180.0);
                        widgets::section_title(ui, "Scopes");
                        if ui.selectable_label(selected.is_none(), "🌐 Globals").clicked() {
                            *selected = None;
                        }
                        ui.separator();
                        for e in &self.ws.environments {
                            let active = self.ws.settings.active_environment.as_deref() == Some(e.id.as_str());
                            let label = if active { format!("● {}", e.name) } else { e.name.clone() };
                            if ui.selectable_label(selected.as_deref() == Some(e.id.as_str()), label).clicked() {
                                *selected = Some(e.id.clone());
                            }
                        }
                        ui.add_space(6.0);
                        if ui.button("+ New environment").clicked() {
                            new_env = true;
                        }
                    });
                    ui.separator();
                    ui.vertical(|ui| {
                        let scope = pakpos_core::vars::VarScope::new();
                        match selected.clone() {
                            None => {
                                ui.label(RichText::new("Globals").strong().size(16.0));
                                ui.label(
                                    RichText::new("Available in every request, lowest priority.")
                                        .small()
                                        .color(p.text_weak),
                                );
                                ui.add_space(6.0);
                                changed |= widgets::kv_table(ui, "globals", &mut self.ws.globals, &scope, "Variable");
                            }
                            Some(id) => {
                                let active_id = self.ws.settings.active_environment.clone();
                                if let Some(env) = self.ws.environments.iter_mut().find(|e| e.id == id) {
                                    ui.horizontal(|ui| {
                                        changed |= ui
                                            .add(
                                                egui::TextEdit::singleline(&mut env.name)
                                                    .min_size(egui::vec2(0.0, crate::theme::CONTROL_HEIGHT))
                                                    .vertical_align(egui::Align::Center)
                                                    .font(egui::TextStyle::Heading)
                                                    .desired_width(260.0),
                                            )
                                            .changed();
                                        if active_id.as_deref() != Some(env.id.as_str())
                                            && ui.button("Set active").clicked()
                                        {
                                            self.actions
                                                .push(crate::app::Action::SetActiveEnvironment(Some(env.id.clone())));
                                        }
                                    });
                                    ui.add_space(6.0);
                                    egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                                        changed |=
                                            widgets::kv_table(ui, "env_vars", &mut env.variables, &scope, "Variable");
                                    });
                                } else {
                                    *selected = None;
                                }
                            }
                        }
                    });
                });
            });
        if new_env {
            let env = pakpos_core::Environment::new(format!("Environment {}", self.ws.environments.len() + 1));
            *selected = Some(env.id.clone());
            self.ws.environments.push(env);
            changed = true;
        }
        if changed {
            self.env_dirty = Some(std::time::Instant::now());
        }
        if !open {
            for e in &mut self.ws.environments {
                widgets::trim_blank_rows(&mut e.variables);
            }
            widgets::trim_blank_rows(&mut self.ws.globals);
            self.persist_environments();
        }
        open
    }

    /// Collection description, variables and scripts. Returns false when closed.
    fn collection_window(&mut self, ctx: &egui::Context, id: &str, tab: &mut crate::app::CollectionTab) -> bool {
        use crate::app::CollectionTab;
        let p = self.palette;
        let trusted = self.ws.is_trusted(id);
        let Some(idx) = self.ws.collections.iter().position(|c| c.id == id) else { return false };
        let mut open = true;
        let mut changed = false;
        let mut trust = None;
        let title = format!("Collection — {}", self.ws.collections[idx].name);
        egui::Window::new(title)
            .id(Id::new("collection_window"))
            .open(&mut open)
            .default_size([720.0, 520.0])
            .pivot(egui::Align2::CENTER_CENTER)
            .default_pos(ctx.content_rect().center())
            .collapsible(false)
            .frame(modal_frame(&p))
            .show(ctx, |ui| {
                let c = &mut self.ws.collections[idx];
                widgets::tab_bar(
                    ui,
                    tab,
                    &[
                        (CollectionTab::General, "General".into()),
                        (CollectionTab::Variables, widgets::counted("Variables", c.variables.iter().filter(|v| !v.is_blank()).count())),
                        (CollectionTab::Scripts, if c.scripts.is_empty() { "Scripts".into() } else { "Scripts ●".into() }),
                    ],
                );
                egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| match tab {
                    CollectionTab::General => {
                        ui.label("Name");
                        changed |= ui.add(egui::TextEdit::singleline(&mut c.name).min_size(egui::vec2(0.0, crate::theme::CONTROL_HEIGHT)).vertical_align(egui::Align::Center).desired_width(f32::INFINITY)).changed();
                        ui.add_space(6.0);
                        ui.label("Description (Markdown)");
                        changed |= ui
                            .add(egui::TextEdit::multiline(&mut c.description).desired_rows(8).desired_width(f32::INFINITY))
                            .changed();
                        ui.add_space(6.0);
                        ui.label(RichText::new(format!("{} requests · id {}", c.request_count(), c.id)).small().color(p.text_weak));
                    }
                    CollectionTab::Variables => {
                        ui.label(
                            RichText::new("Collection variables are shared by every request here and overridden by the active environment.")
                                .small()
                                .color(p.text_weak),
                        );
                        ui.add_space(4.0);
                        changed |= widgets::kv_table(ui, "coll_vars", &mut c.variables, &pakpos_core::vars::VarScope::new(), "Variable");
                    }
                    CollectionTab::Scripts => {
                        ui.label(RichText::new("These run around every request in the collection, before the request's own scripts.").small().color(p.text_weak));
                        ui.add_space(4.0);
                        let mut action = None;
                        let trust_state = Some(trusted || !c.has_scripts());
                        changed |= crate::scripts_view::scripts_editor(
                            ui,
                            &mut c.scripts,
                            0,
                            trust_state,
                            0,
                            &p,
                            &mut action,
                        );
                        if action.is_some() {
                            trust = Some(c.id.clone());
                        }
                    }
                });
            });
        if let Some(cid) = trust {
            self.trust_collection(&cid);
        }
        if changed {
            self.collection_dirty = Some((id.to_string(), std::time::Instant::now()));
        }
        if !open {
            widgets::trim_blank_rows(&mut self.ws.collections[idx].variables);
            self.persist_collection(id);
            self.collection_dirty = None;
        }
        open
    }

    pub fn rename(&mut self, target: RenameTarget, name: String) {
        match target {
            RenameTarget::Collection(id) => {
                if let Some(c) = self.collection_mut(&id) {
                    c.name = name;
                }
                self.persist_collection(&id);
            }
            RenameTarget::Item { collection, item } => {
                if let Some(c) = self.collection_mut(&collection) {
                    if let Some(r) = c.find_request_mut(&item) {
                        r.name = name.clone();
                    } else if let Some(f) = c.find_folder_mut(&item) {
                        f.name = name.clone();
                    }
                }
                for t in &mut self.tabs {
                    if t.source.as_ref().is_some_and(|(c, r)| c == &collection && r == &item) {
                        t.request.name = name.clone();
                    }
                }
                self.persist_collection(&collection);
            }
            RenameTarget::Environment(id) => {
                if let Some(e) = self.ws.environments.iter_mut().find(|e| e.id == id) {
                    e.name = name;
                }
                self.persist_environments();
            }
        }
    }

    pub fn delete(&mut self, target: DeleteTarget) {
        match target {
            DeleteTarget::Collection(id) => {
                self.ws.collections.retain(|c| c.id != id);
                if let Err(e) = self.ws.delete_collection_file(&id) {
                    self.toast_error(e.to_string());
                }
                for t in &mut self.tabs {
                    if t.source.as_ref().is_some_and(|(c, _)| c == &id) {
                        t.source = None;
                        t.dirty = true;
                    }
                }
            }
            DeleteTarget::Item { collection, item } => {
                if let Some(c) = self.collection_mut(&collection) {
                    c.remove_item(&item);
                }
                for t in &mut self.tabs {
                    if t.source.as_ref().is_some_and(|(c, r)| c == &collection && r == &item) {
                        t.source = None;
                        t.dirty = true;
                    }
                }
                self.persist_collection(&collection);
            }
            DeleteTarget::Environment(id) => {
                self.ws.environments.retain(|e| e.id != id);
                if self.ws.settings.active_environment.as_deref() == Some(id.as_str()) {
                    self.ws.settings.active_environment = None;
                    self.persist_settings();
                }
                if let Err(e) = self.ws.delete_environment_file(&id) {
                    self.toast_error(e.to_string());
                }
            }
        }
    }

    pub fn runner_window(&mut self, ctx: &egui::Context) {
        let p = self.palette;
        let Some(r) = &mut self.runner else { return };
        let mut open = r.open;
        egui::Window::new(format!("Runner — {}", r.collection))
            .id(Id::new("runner"))
            .open(&mut open)
            .default_size([640.0, 540.0])
            .pivot(egui::Align2::CENTER_CENTER)
            .default_pos(ctx.content_rect().center())
            .collapsible(false)
            .frame(modal_frame(&p))
            .show(ctx, |ui| {
                let done = r.report.is_some();
                let finished = r.progress.len();
                ui.horizontal(|ui| {
                    if let Some(env) = &r.environment {
                        widgets::pill(ui, format!("env: {env}"), p.text, p.surface_alt);
                    }
                    if let Some(rep) = &r.report {
                        let (passed, total) = rep.assertion_counts();
                        let c = if rep.passed() { theme::GREEN } else { theme::RED };
                        widgets::pill(ui, if rep.passed() { "PASSED" } else { "FAILED" }, c, c.gamma_multiply(0.16));
                        widgets::pill(ui, format!("tests {passed}/{total}"), p.text, p.surface_alt);
                        widgets::pill(ui, format!("{} ms", rep.total_ms), p.text, p.surface_alt);
                    } else {
                        ui.add(egui::Spinner::new().color(theme::ORANGE));
                        ui.label(format!("{finished}/{} requests", r.total));
                    }
                });
                ui.add(
                    egui::ProgressBar::new(if r.total == 0 { 1.0 } else { finished as f32 / r.total as f32 })
                        .fill(theme::ORANGE)
                        .desired_height(6.0),
                );
                ui.add_space(6.0);
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    for rep in &r.progress {
                        ui.horizontal(|ui| {
                            let (mark, c) = if rep.passed() { ("✔", theme::GREEN) } else { ("✖", theme::RED) };
                            ui.label(RichText::new(mark).color(c).strong());
                            ui.label(RichText::new(&rep.method).monospace().small().color(p.text_weak));
                            ui.label(RichText::new(&rep.name).strong());
                            if let Some(s) = rep.status {
                                ui.label(RichText::new(s.to_string()).color(theme::status_color(s)));
                            }
                            if let Some(ms) = rep.elapsed_ms {
                                ui.label(RichText::new(format!("{ms} ms")).small().color(p.text_weak));
                            }
                        });
                        if let Some(e) = &rep.error {
                            ui.label(RichText::new(format!("    {e}")).small().color(theme::RED));
                        }
                        for a in &rep.assertions {
                            let c = if a.passed { theme::GREEN } else { theme::RED };
                            ui.label(
                                RichText::new(format!(
                                    "      {} {}{}",
                                    if a.passed { "✔" } else { "✖" },
                                    a.name,
                                    if a.passed { String::new() } else { format!(" — {}", a.message) }
                                ))
                                .small()
                                .color(c),
                            );
                        }
                    }
                    if done && r.progress.is_empty() {
                        ui.label("Collection has no requests.");
                    }
                });
            });
        r.open = open;
        if !open && r.report.is_some() {
            self.runner = None;
        }
    }
}
