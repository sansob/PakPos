//! Modal dialogs and the collection runner window.

use crate::app::{clean_request, DeleteTarget, Dialog, PakPosApp, RenameTarget};
use crate::theme;
use crate::widgets;
use egui::{Id, RichText};
use pakpos_core::codegen::{self, SnippetLang};
use pakpos_core::model::Item;
use pakpos_core::storage::Theme;
use pakpos_core::Collection;

pub(crate) fn modal_frame(p: &theme::Palette) -> egui::Frame {
    egui::Frame::new().fill(p.surface).stroke(egui::Stroke::new(1.0, p.border)).corner_radius(12).inner_margin(20)
}

pub(crate) fn title(ui: &mut egui::Ui, text: &str) {
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
                    title(ui, crate::i18n::tr("Import cURL"));
                    ui.label(
                        RichText::new(crate::i18n::tr(crate::i18n::tr(
                            "Paste a command from your terminal or a browser's “Copy as cURL”.",
                        )))
                        .color(p.text_weak),
                    );
                    ui.add_space(6.0);
                    ui.add(
                        egui::TextEdit::multiline(text)
                            .code_editor()
                            .desired_rows(10)
                            .desired_width(f32::INFINITY)
                            .hint_text(crate::i18n::tr(
                                "curl https://api.example.com/users -H 'Accept: application/json'",
                            )),
                    );
                    if let Some(e) = error {
                        ui.label(RichText::new(e.as_str()).color(theme::RED));
                    }
                    ui.add_space(8.0);
                    let mut result = None;
                    ui.horizontal(|ui| {
                        if widgets::primary_button(ui, crate::i18n::tr("Import")).clicked() {
                            result = Some(codegen::parse_curl(text));
                        }
                        if ui.button(crate::i18n::tr("Cancel")).clicked() {
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
                        self.toast(crate::i18n::tr("cURL imported into a new tab"));
                    }
                    Some(Err(e)) => *error = Some(e.to_string()),
                    None => {}
                }
            }
            Dialog::SaveRequest { tab, collection, new_collection, name } => {
                let r = egui::Modal::new(Id::new("save_request")).frame(modal_frame(&p)).show(ctx, |ui| {
                    ui.set_width(440.0);
                    title(ui, crate::i18n::tr("Save request"));
                    ui.label(crate::i18n::tr("Name"));
                    ui.add(
                        egui::TextEdit::singleline(name)
                            .min_size(egui::vec2(0.0, crate::theme::CONTROL_HEIGHT))
                            .vertical_align(egui::Align::Center)
                            .desired_width(f32::INFINITY),
                    );
                    ui.add_space(8.0);
                    ui.label(crate::i18n::tr("Collection"));
                    egui::ScrollArea::vertical().max_height(180.0).show(ui, |ui| {
                        for c in &self.ws.collections {
                            ui.radio_value(collection, Some(c.id.clone()), &c.name);
                        }
                        ui.radio_value(collection, None, crate::i18n::tr("New collection…"));
                    });
                    if collection.is_none() {
                        ui.add(
                            egui::TextEdit::singleline(new_collection)
                                .min_size(egui::vec2(0.0, crate::theme::CONTROL_HEIGHT))
                                .vertical_align(egui::Align::Center)
                                .hint_text(crate::i18n::tr("Collection name"))
                                .desired_width(f32::INFINITY),
                        );
                    }
                    ui.add_space(10.0);
                    let mut save = false;
                    ui.horizontal(|ui| {
                        save = widgets::primary_button(ui, crate::i18n::tr("Save")).clicked();
                        if ui.button(crate::i18n::tr("Cancel")).clicked() {
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
                            let n = if new_collection.trim().is_empty() {
                                crate::i18n::tr("My collection")
                            } else {
                                new_collection.trim()
                            };
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
                    req.name = if name.trim().is_empty() {
                        crate::i18n::tr("Untitled request").into()
                    } else {
                        name.trim().to_string()
                    };
                    clean_request(&mut req);
                    t.request.id = req.id.clone();
                    t.request.name = req.name.clone();
                    t.source = Some((cid.clone(), req.id.clone()));
                    t.dirty = false;
                    if let Some(c) = self.collection_mut(&cid) {
                        c.items.push(Item::Request(req));
                    }
                    self.persist_collection(&cid);
                    self.toast(crate::i18n::tr("Request saved"));
                    keep = false;
                }
            }
            Dialog::NewCollection { name } => {
                let r = egui::Modal::new(Id::new("new_collection")).frame(modal_frame(&p)).show(ctx, |ui| {
                    ui.set_width(380.0);
                    title(ui, crate::i18n::tr("New collection"));
                    let resp = ui.add(
                        egui::TextEdit::singleline(name)
                            .min_size(egui::vec2(0.0, crate::theme::CONTROL_HEIGHT))
                            .vertical_align(egui::Align::Center)
                            .hint_text(crate::i18n::tr("e.g. Payments API"))
                            .desired_width(f32::INFINITY),
                    );
                    resp.request_focus();
                    ui.add_space(10.0);
                    let mut ok = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    ui.horizontal(|ui| {
                        ok |= widgets::primary_button(ui, crate::i18n::tr("Create")).clicked();
                        if ui.button(crate::i18n::tr("Cancel")).clicked() {
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
                    title(ui, crate::i18n::tr("Rename"));
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
                        ok |= widgets::primary_button(ui, crate::i18n::tr("Rename")).clicked();
                        if ui.button(crate::i18n::tr("Cancel")).clicked() {
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
            Dialog::CloseTabs { uids } => {
                let unsaved: Vec<(usize, String, bool)> = self
                    .tabs
                    .iter()
                    .enumerate()
                    .filter(|(_, t)| t.dirty && uids.contains(&t.uid))
                    .map(|(i, t)| (i, t.request.name.clone(), t.source.is_some()))
                    .collect();
                let all_saveable = unsaved.iter().all(|(_, _, s)| *s);
                let r = egui::Modal::new(Id::new("close_tabs")).frame(modal_frame(&p)).show(ctx, |ui| {
                    ui.set_width(420.0);
                    title(ui, crate::i18n::tr("Unsaved changes"));
                    ui.label(crate::i18n::trf(
                        "{n} tab(s) have changes that are not saved:",
                        &[("n", &unsaved.len().to_string())],
                    ));
                    for (_, name, _) in unsaved.iter().take(8) {
                        ui.label(RichText::new(format!("• {name}")).color(p.text_weak));
                    }
                    if unsaved.len() > 8 {
                        ui.label(RichText::new("…").color(p.text_weak));
                    }
                    ui.add_space(12.0);
                    let mut choice = None;
                    ui.horizontal(|ui| {
                        if all_saveable && widgets::primary_button(ui, crate::i18n::tr("Save and close")).clicked() {
                            choice = Some(true);
                        }
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new(crate::i18n::tr("Close without saving"))
                                        .color(egui::Color32::WHITE)
                                        .strong(),
                                )
                                .fill(theme::RED),
                            )
                            .clicked()
                        {
                            choice = Some(false);
                        }
                        if ui.button(crate::i18n::tr("Cancel")).clicked() {
                            keep = false;
                        }
                    });
                    choice
                });
                if r.should_close() {
                    keep = false;
                }
                if let Some(save) = r.inner {
                    if save {
                        for (i, _, _) in &unsaved {
                            self.save_tab(*i);
                        }
                    }
                    let uids = uids.clone();
                    self.close_tabs(&uids);
                    keep = false;
                }
            }
            Dialog::ConfirmDelete { target, label } => {
                let r = egui::Modal::new(Id::new("confirm_delete")).frame(modal_frame(&p)).show(ctx, |ui| {
                    ui.set_width(380.0);
                    title(ui, crate::i18n::tr("Delete?"));
                    ui.label(crate::i18n::trf("“{label}” will be deleted permanently.", &[("label", label)]));
                    ui.add_space(12.0);
                    let mut ok = false;
                    ui.horizontal(|ui| {
                        ok = ui
                            .add(
                                egui::Button::new(
                                    RichText::new(crate::i18n::tr("Delete")).color(egui::Color32::WHITE).strong(),
                                )
                                .fill(theme::RED),
                            )
                            .clicked();
                        if ui.button(crate::i18n::tr("Cancel")).clicked() {
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
                    title(ui, crate::i18n::tr("Settings"));
                    let mut changed = false;
                    let s = &mut self.ws.settings;
                    egui::Grid::new("settings_grid").num_columns(2).spacing([16.0, 10.0]).show(ui, |ui| {
                        ui.label(crate::i18n::tr("Theme"));
                        ui.horizontal(|ui| {
                            changed |=
                                ui.selectable_value(&mut s.theme, Theme::Dark, crate::i18n::tr("🌙 Dark")).changed();
                            changed |=
                                ui.selectable_value(&mut s.theme, Theme::Light, crate::i18n::tr("☀ Light")).changed();
                        });
                        ui.end_row();
                        ui.label(crate::i18n::tr("Language"));
                        let current = crate::i18n::current();
                        let mut lang = current;
                        egui::ComboBox::from_id_salt("language").selected_text(lang.name()).show_ui(ui, |ui| {
                            for l in crate::i18n::Lang::ALL {
                                ui.selectable_value(&mut lang, l, l.name());
                            }
                        });
                        if lang != current {
                            crate::i18n::set(lang);
                            s.language = Some(lang.code().to_string());
                            changed = true;
                        }
                        ui.end_row();
                        ui.label(crate::i18n::tr("Font size"));
                        changed |= ui.add(egui::Slider::new(&mut s.font_scale_percent, 80..=160).suffix("%")).changed();
                        ui.end_row();
                        ui.label(crate::i18n::tr("Request timeout"));
                        changed |= ui
                            .add(egui::DragValue::new(&mut s.http.timeout_secs).range(1..=600).suffix(" s"))
                            .changed();
                        ui.end_row();
                        ui.label(crate::i18n::tr("Follow redirects"));
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
                        ui.label(crate::i18n::tr("Verify TLS certificates"));
                        changed |= ui.checkbox(&mut s.http.verify_tls, "").changed();
                        ui.end_row();
                        ui.label(crate::i18n::tr("Proxy"));
                        changed |= ui
                            .add(
                                egui::TextEdit::singleline(&mut s.http.proxy)
                                    .min_size(egui::vec2(0.0, crate::theme::CONTROL_HEIGHT))
                                    .vertical_align(egui::Align::Center)
                                    .hint_text(crate::i18n::tr("http://127.0.0.1:8080"))
                                    .desired_width(260.0),
                            )
                            .changed();
                        ui.end_row();
                        ui.label(crate::i18n::tr("User-Agent"));
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
                            RichText::new(crate::i18n::tr(crate::i18n::tr(
                                "⚠ TLS verification is off: only use this for local development.",
                            )))
                            .small()
                            .color(theme::YELLOW),
                        );
                    }
                    ui.add_space(8.0);
                    changed |= client_certs_editor(ui, &mut s.http.client_certs, &p);
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new(crate::i18n::trf(
                            "Workspace folder: {path}",
                            &[("path", &self.ws.root.display().to_string())],
                        ))
                        .small()
                        .color(p.text_weak),
                    );
                    ui.label(
                        RichText::new(crate::i18n::tr(crate::i18n::tr(
                            "Set PAKPOS_HOME to use another folder (e.g. a git repo shared with your team).",
                        )))
                        .small()
                        .color(p.text_weak),
                    );
                    ui.add_space(10.0);
                    if widgets::primary_button(ui, crate::i18n::tr("Done")).clicked() {
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
                    title(ui, crate::i18n::tr("Code snippet"));
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
                    ui.label(RichText::new(crate::i18n::tr("Variables are resolved with the active environment. Secrets are included — share with care.")).small().color(p.text_weak));
                    ui.add_space(8.0);
                    let mut copy = false;
                    ui.horizontal(|ui| {
                        copy = widgets::primary_button(ui, crate::i18n::tr("Copy")).clicked();
                        if ui.button(crate::i18n::tr("Close")).clicked() {
                            keep = false;
                        }
                    });
                    copy
                });
                if r.inner {
                    ctx.copy_text(code);
                    self.toast(crate::i18n::tr("Snippet copied"));
                }
                if r.should_close() {
                    keep = false;
                }
            }
            Dialog::Cookies(st) => {
                keep = self.cookies_dialog(ctx, st);
            }
            Dialog::NewWorkspace { name, path, error } => {
                if self.new_workspace_dialog(ctx, name, path, error) {
                    keep = false;
                }
            }
            Dialog::About => {
                let r = egui::Modal::new(Id::new("about")).frame(modal_frame(&p)).show(ctx, |ui| {
                    ui.set_width(420.0);
                    ui.vertical_centered(|ui| {
                        ui.add(
                            egui::Image::new(egui::include_image!("../../../assets/icon-256.png"))
                                .fit_to_exact_size(egui::vec2(96.0, 96.0)),
                        );
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
                        ui.label(crate::i18n::trf("Version {v}", &[("v", pakpos_core::VERSION)]));
                        ui.add_space(6.0);
                        ui.label(crate::i18n::tr("A fast, native, open-source API client written in Rust."));
                        ui.hyperlink_to("github.com/sansob/PakPos", "https://github.com/sansob/PakPos");
                        ui.label(
                            RichText::new(crate::i18n::tr("Licensed under MIT or Apache-2.0"))
                                .small()
                                .color(p.text_weak),
                        );
                        ui.add_space(10.0);
                        if ui.button(crate::i18n::tr("Close")).clicked() {
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
        egui::Window::new(crate::i18n::tr("Environments"))
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
                        widgets::section_title(ui, crate::i18n::tr("Scopes"));
                        if ui.selectable_label(selected.is_none(), crate::i18n::tr("🌐 Globals")).clicked() {
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
                        if ui.button(crate::i18n::tr("+ New environment")).clicked() {
                            new_env = true;
                        }
                    });
                    ui.separator();
                    ui.vertical(|ui| {
                        let scope = pakpos_core::vars::VarScope::new();
                        match selected.clone() {
                            None => {
                                ui.label(RichText::new(crate::i18n::tr("Globals")).strong().size(16.0));
                                ui.label(
                                    RichText::new(crate::i18n::tr("Available in every request, lowest priority."))
                                        .small()
                                        .color(p.text_weak),
                                );
                                ui.add_space(6.0);
                                changed |= widgets::kv_table(
                                    ui,
                                    "globals",
                                    &mut self.ws.globals,
                                    &scope,
                                    crate::i18n::tr("Variable"),
                                );
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
                                            && ui.button(crate::i18n::tr("Set active")).clicked()
                                        {
                                            self.actions
                                                .push(crate::app::Action::SetActiveEnvironment(Some(env.id.clone())));
                                        }
                                    });
                                    ui.add_space(6.0);
                                    egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                                        changed |= widgets::kv_table(
                                            ui,
                                            "env_vars",
                                            &mut env.variables,
                                            &scope,
                                            crate::i18n::tr("Variable"),
                                        );
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
            let env = pakpos_core::Environment::new(crate::i18n::trf(
                "Environment {n}",
                &[("n", &(self.ws.environments.len() + 1).to_string())],
            ));
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
        let mut network = self.ws.settings.network_collections.iter().any(|c| c == id);
        let network_before = network;
        let Some(idx) = self.ws.collections.iter().position(|c| c.id == id) else { return false };
        let mut open = true;
        let mut changed = false;
        let mut trust = None;
        let title = crate::i18n::trf("Collection — {name}", &[("name", &self.ws.collections[idx].name)]);
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
                        (CollectionTab::General, crate::i18n::tr("General").into()),
                        (CollectionTab::Variables, widgets::counted("Variables", c.variables.iter().filter(|v| !v.is_blank()).count())),
                        (CollectionTab::Scripts, if c.scripts.is_empty() { crate::i18n::tr("Scripts").into() } else { format!("{} ●", crate::i18n::tr("Scripts")) }),
                    ],
                );
                egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| match tab {
                    CollectionTab::General => {
                        ui.label(crate::i18n::tr("Name"));
                        changed |= ui.add(egui::TextEdit::singleline(&mut c.name).min_size(egui::vec2(0.0, crate::theme::CONTROL_HEIGHT)).vertical_align(egui::Align::Center).desired_width(f32::INFINITY)).changed();
                        ui.add_space(6.0);
                        ui.label(crate::i18n::tr("Description (Markdown)"));
                        changed |= ui
                            .add(egui::TextEdit::multiline(&mut c.description).desired_rows(8).desired_width(f32::INFINITY))
                            .changed();
                        ui.add_space(6.0);
                        ui.label(RichText::new(crate::i18n::trf("{n} requests · id {id}", &[("n", &c.request_count().to_string()), ("id", &c.id)])).small().color(p.text_weak));
                    }
                    CollectionTab::Variables => {
                        ui.label(
                            RichText::new(crate::i18n::tr("Collection variables are shared by every request here and overridden by the active environment."))
                                .small()
                                .color(p.text_weak),
                        );
                        ui.add_space(4.0);
                        changed |= widgets::kv_table(ui, "coll_vars", &mut c.variables, &pakpos_core::vars::VarScope::new(), crate::i18n::tr("Variable"));
                    }
                    CollectionTab::Scripts => {
                        ui.label(RichText::new(crate::i18n::tr("These run around every request in the collection, before the request's own scripts.")).small().color(p.text_weak));
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
                        ui.add_space(8.0);
                        ui.add_enabled_ui(trusted, |ui| {
                            ui.checkbox(&mut network, crate::i18n::tr("Allow network access for scripts (pm.sendRequest)"))
                                .on_disabled_hover_text(crate::i18n::tr("Trust the collection first"));
                        });
                        ui.label(
                            RichText::new(
                                crate::i18n::tr("Lets this collection's scripts send their own HTTP requests (e.g. to fetch a token). Stored in your settings, not in the collection file."),
                            )
                            .small()
                            .color(p.text_weak),
                        );
                    }
                });
            });
        if let Some(cid) = trust {
            self.trust_collection(&cid);
        }
        if network != network_before {
            let list = &mut self.ws.settings.network_collections;
            list.retain(|c| c != id);
            if network {
                list.push(id.to_string());
            }
            let _ = self.ws.save_settings();
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

    /// Options before a run: iterations, data file, delay, stop on failure.
    pub fn runner_setup_window(&mut self, ctx: &egui::Context) {
        let p = self.palette;
        let Some(setup) = &mut self.runner_setup else { return };
        let mut open = true;
        let mut start = false;
        egui::Window::new(crate::i18n::trf("Run — {name}", &[("name", &setup.collection)]))
            .id(Id::new("runner_setup"))
            .open(&mut open)
            .default_width(520.0)
            .pivot(egui::Align2::CENTER_CENTER)
            .default_pos(ctx.content_rect().center())
            .collapsible(false)
            .resizable(false)
            .frame(modal_frame(&p))
            .show(ctx, |ui| {
                egui::Grid::new("runner_setup_grid").num_columns(2).spacing([12.0, 10.0]).show(ui, |ui| {
                    ui.label(crate::i18n::tr("Data file"));
                    ui.horizontal(|ui| {
                        let name = setup
                            .data_path
                            .as_deref()
                            .map(|p| {
                                std::path::Path::new(p)
                                    .file_name()
                                    .map(|f| f.to_string_lossy().into_owned())
                                    .unwrap_or_default()
                            })
                            .unwrap_or_else(|| "none".into());
                        ui.label(RichText::new(name).monospace());
                        if ui.button(crate::i18n::tr("Choose CSV / JSON…")).clicked() {
                            if let Some(path) = rfd::FileDialog::new().add_filter("Data", &["csv", "json"]).pick_file()
                            {
                                setup.load_data(path.display().to_string());
                            }
                        }
                        if setup.data_path.is_some()
                            && ui.button("×").on_hover_text(crate::i18n::tr("No data file")).clicked()
                        {
                            setup.data_path = None;
                            setup.data.clear();
                            setup.data_error = None;
                        }
                    });
                    ui.end_row();
                    ui.label(crate::i18n::tr("Iterations"));
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::DragValue::new(&mut setup.iterations)
                                .range(0..=10_000)
                                .custom_formatter(|n, _| if n < 1.0 { "auto".into() } else { format!("{n}") })
                                .custom_parser(|s| if s.trim() == "auto" { Some(0.0) } else { s.trim().parse().ok() }),
                        );
                        let effective = match (setup.iterations, setup.data.len()) {
                            (0, 0) => crate::i18n::tr("1 run").to_string(),
                            (0, n) => crate::i18n::trf("{n} (one per data row)", &[("n", &n.to_string())]),
                            (n, _) => format!("{n}"),
                        };
                        ui.label(RichText::new(effective).small().color(p.text_weak));
                    });
                    ui.end_row();
                    ui.label(crate::i18n::tr("Delay"));
                    ui.horizontal(|ui| {
                        ui.add(egui::DragValue::new(&mut setup.delay_ms).range(0..=60_000).suffix(" ms"));
                        ui.label(RichText::new(crate::i18n::tr("between requests")).small().color(p.text_weak));
                    });
                    ui.end_row();
                    ui.label("");
                    ui.checkbox(&mut setup.stop_on_failure, crate::i18n::tr("Stop at the first failure"));
                    ui.end_row();
                });
                if let Some(e) = &setup.data_error {
                    ui.label(RichText::new(e).small().color(theme::RED));
                }
                if !setup.data.is_empty() {
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new(crate::i18n::trf(
                            "DATA PREVIEW · {n} rows · columns are {{variables}}",
                            &[("n", &setup.data.len().to_string())],
                        ))
                        .small()
                        .strong()
                        .color(p.text_weak),
                    );
                    egui::ScrollArea::both().max_height(150.0).show(ui, |ui| {
                        egui::Grid::new("data_preview").striped(true).spacing([14.0, 4.0]).show(ui, |ui| {
                            for (k, _) in &setup.data[0] {
                                ui.label(RichText::new(k).strong().monospace());
                            }
                            ui.end_row();
                            for row in setup.data.iter().take(8) {
                                for (_, v) in row {
                                    ui.label(RichText::new(v).monospace());
                                }
                                ui.end_row();
                            }
                        });
                    });
                }
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    start = widgets::primary_button(ui, crate::i18n::tr("▶ Run")).clicked();
                });
            });
        if start {
            let setup = self.runner_setup.take().expect("open");
            self.start_runner_with(setup, ctx);
        } else if !open {
            self.runner_setup = None;
        }
    }

    pub fn runner_window(&mut self, ctx: &egui::Context) {
        let p = self.palette;
        let mut rerun = None;
        let Some(r) = &mut self.runner else { return };
        let mut open = r.open;
        egui::Window::new(crate::i18n::trf("Runner — {name}", &[("name", &r.collection)]))
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
                        if rep.iterations > 1 {
                            widgets::pill(
                                ui,
                                crate::i18n::trf("{n} iterations", &[("n", &rep.iterations.to_string())]),
                                p.text,
                                p.surface_alt,
                            );
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button(crate::i18n::tr("⟳ Run again")).clicked() {
                                rerun = Some(r.setup.clone());
                            }
                        });
                    } else {
                        ui.add(egui::Spinner::new().color(theme::ORANGE));
                        ui.label(crate::i18n::trf(
                            "{done}/{total} requests",
                            &[("done", &finished.to_string()), ("total", &r.total.to_string())],
                        ));
                    }
                });
                ui.add(
                    egui::ProgressBar::new(if r.total == 0 { 1.0 } else { finished as f32 / r.total as f32 })
                        .fill(theme::ORANGE)
                        .desired_height(6.0),
                );
                ui.add_space(6.0);
                let many = r.progress.iter().any(|x| x.iteration > 0);
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    let mut last = usize::MAX;
                    for rep in &r.progress {
                        if many && rep.iteration != last {
                            last = rep.iteration;
                            ui.add_space(4.0);
                            ui.label(
                                RichText::new(crate::i18n::trf(
                                    "ITERATION {n}",
                                    &[("n", &(rep.iteration + 1).to_string())],
                                ))
                                .small()
                                .strong()
                                .color(p.text_weak),
                            );
                        }
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
                        ui.label(crate::i18n::tr("Collection has no requests."));
                    }
                });
            });
        r.open = open;
        if !open && r.report.is_some() {
            self.runner = None;
        }
        if let Some(setup) = rerun {
            self.start_runner_with(setup, ctx);
        }
    }
}

/// Client certificates (mTLS) per host, in the settings dialog.
fn client_certs_editor(ui: &mut egui::Ui, certs: &mut Vec<pakpos_core::http::ClientCert>, p: &theme::Palette) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label(RichText::new(crate::i18n::tr("CLIENT CERTIFICATES (mTLS)")).small().strong().color(p.text_weak));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button(crate::i18n::tr("+ Add")).clicked() {
                certs.push(pakpos_core::http::ClientCert { enabled: true, ..Default::default() });
                changed = true;
            }
        });
    });
    if certs.is_empty() {
        ui.label(
            RichText::new(crate::i18n::tr(
                "PEM certificate + key per host, used for HTTPS and gRPC (e.g. *.internal.example.com).",
            ))
            .small()
            .color(p.text_weak),
        );
    }
    let mut remove = None;
    let field = |ui: &mut egui::Ui, value: &mut String, hint: &str, width: f32| {
        ui.add(
            egui::TextEdit::singleline(value)
                .hint_text(hint)
                .min_size(egui::vec2(0.0, crate::theme::CONTROL_HEIGHT))
                .vertical_align(egui::Align::Center)
                .desired_width(width),
        )
        .changed()
    };
    for (i, cert) in certs.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            changed |= ui.checkbox(&mut cert.enabled, "").changed();
            changed |= field(ui, &mut cert.host, crate::i18n::tr("host or *.domain"), 130.0);
            changed |= field(ui, &mut cert.cert_path, "cert.pem", 120.0);
            changed |= field(ui, &mut cert.key_path, crate::i18n::tr("key.pem (optional)"), 120.0);
            changed |= field(ui, &mut cert.ca_path, crate::i18n::tr("CA (optional)"), 100.0);
            if ui.button("×").on_hover_text(crate::i18n::tr("Remove")).clicked() {
                remove = Some(i);
            }
        });
    }
    if let Some(i) = remove {
        certs.remove(i);
        changed = true;
    }
    changed
}
