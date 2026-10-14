//! Several workspaces: the header menu to switch, create or open one.

use crate::app::{Dialog, PakPosApp, SavedSession, SavedTab};
use crate::theme;
use egui::RichText;
use pakpos_core::storage::{self, Workspace};
use std::path::{Path, PathBuf};

impl PakPosApp {
    /// The open tabs, to restore later.
    pub fn current_session(&self) -> SavedSession {
        SavedSession {
            tabs: self
                .tabs
                .iter()
                .map(|t| SavedTab { request: t.request.clone(), source: t.source.clone(), dirty: t.dirty })
                .collect(),
            active: self.active,
            sidebar_width: None,
        }
    }

    pub fn restore_session(&mut self, session: SavedSession) {
        self.tabs.clear();
        for t in session.tabs {
            let idx = self.push_tab(t.request, t.source);
            self.tabs[idx].dirty = t.dirty;
        }
        self.active = session.active.min(self.tabs.len().saturating_sub(1));
    }

    /// The name shown for the current workspace.
    pub fn workspace_name(&self) -> String {
        self.registry
            .entries(&self.ws.root)
            .into_iter()
            .find(|e| storage::same_path(&e.path, &self.ws.root))
            .map(|e| e.name)
            .unwrap_or_default()
    }

    /// Saves what is pending, keeps this workspace's tabs, and opens `root`.
    pub fn switch_workspace(&mut self, root: PathBuf) {
        if storage::same_path(&root, &self.ws.root) {
            return;
        }
        if self.env_dirty.is_some() {
            self.persist_environments();
            self.env_dirty = None;
        }
        if let Some((id, _)) = self.collection_dirty.take() {
            self.persist_collection(&id);
        }
        self.sync_cookies();
        let key = self.ws.root.display().to_string();
        let session = self.current_session();
        self.sessions.insert(key, session);

        let (ws, errors) = Workspace::load(&root);
        self.ws = ws;
        self.cookie_jar = pakpos_core::cookies::SharedJar::new(self.ws.cookies.clone());
        self.mock = None;
        self.git = Default::default();
        self.runner = None;
        self.runner_setup = None;
        self.dialog = None;
        self.open_nodes = self.ws.collections.iter().map(|c| c.id.clone()).collect();
        if let Some(code) = &self.ws.settings.language {
            crate::i18n::set(crate::i18n::Lang::from_code(code));
        }
        match self.sessions.remove(&root.display().to_string()) {
            Some(s) => self.restore_session(s),
            None => self.tabs.clear(),
        }
        if self.tabs.is_empty() {
            self.new_blank_tab();
        }
        for e in errors {
            self.toast_error(e.to_string());
        }
        self.registry.active = Some(root);
        if let Err(e) = self.registry.save() {
            self.toast_error(e.to_string());
        }
        let name = self.workspace_name();
        self.toast(crate::i18n::trf("Workspace: {name}", &[("name", &name)]));
    }

    /// Adds a folder as a workspace and opens it.
    pub fn open_workspace_folder(&mut self, path: PathBuf, name: Option<String>) {
        let name = name.unwrap_or_else(|| {
            path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.display().to_string())
        });
        self.registry.add(&name, &path);
        self.switch_workspace(path);
    }

    /// The workspace button in the header.
    pub fn workspace_menu(&mut self, ui: &mut egui::Ui) {
        let p = self.palette;
        let current = self.ws.root.clone();
        let label = format!("{} ▾", self.workspace_name());
        let button = ui.add(
            egui::Button::new(RichText::new(label).color(p.header_text))
                .fill(p.header_button)
                .stroke(egui::Stroke::NONE),
        );
        let button = button
            .on_hover_text(crate::i18n::trf("Workspace folder: {path}", &[("path", &current.display().to_string())]));
        let mut switch_to = None;
        let mut forget = None;
        egui::Popup::menu(&button).show(|ui| {
            ui.set_min_width(260.0);
            ui.label(RichText::new(crate::i18n::tr("WORKSPACES")).small().strong().color(p.text_weak));
            for e in self.registry.entries(&current) {
                let is_current = storage::same_path(&e.path, &current);
                ui.horizontal(|ui| {
                    let text = if is_current { format!("✔ {}", e.name) } else { format!("    {}", e.name) };
                    if ui
                        .add(egui::Button::new(text).frame(false))
                        .on_hover_text(e.path.display().to_string())
                        .clicked()
                        && !is_current
                    {
                        switch_to = Some(e.path.clone());
                        ui.close();
                    }
                    let removable = !is_current && !storage::same_path(&e.path, &storage::data_dir());
                    if removable {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .add(egui::Button::new(RichText::new("×").color(p.text_weak)).frame(false))
                                .on_hover_text(crate::i18n::tr("Remove from the list (the folder is kept)"))
                                .clicked()
                            {
                                forget = Some(e.path.clone());
                            }
                        });
                    }
                });
            }
            ui.separator();
            if ui.button(crate::i18n::tr("New workspace…")).clicked() {
                self.dialog = Some(Dialog::NewWorkspace { name: String::new(), path: String::new(), error: None });
                ui.close();
            }
            if ui.button(crate::i18n::tr("Open folder as workspace…")).clicked() {
                ui.close();
                if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                    switch_to = None;
                    self.open_workspace_folder(dir, None);
                }
            }
        });
        if let Some(path) = forget {
            self.registry.remove(&path);
            let _ = self.registry.save();
        }
        if let Some(path) = switch_to {
            self.switch_workspace(path);
        }
    }

    /// Body of the "New workspace" dialog; true when it should close.
    pub fn new_workspace_dialog(
        &mut self,
        ctx: &egui::Context,
        name: &mut String,
        path: &mut String,
        error: &mut Option<String>,
    ) -> bool {
        let p = self.palette;
        let suggested = suggested_folder(name);
        let r =
            egui::Modal::new(egui::Id::new("new_workspace")).frame(crate::dialogs::modal_frame(&p)).show(ctx, |ui| {
                ui.set_width(520.0);
                crate::dialogs::title(ui, crate::i18n::tr("New workspace"));
                ui.label(
                    RichText::new(crate::i18n::tr(
                        "A workspace is a folder with its own collections, environments, history and settings.",
                    ))
                    .color(p.text_weak),
                );
                ui.add_space(8.0);
                ui.label(crate::i18n::tr("Name"));
                let name_edit = ui.add(
                    egui::TextEdit::singleline(name)
                        .hint_text(crate::i18n::tr("e.g. Payments team"))
                        .min_size(egui::vec2(0.0, theme::CONTROL_HEIGHT))
                        .vertical_align(egui::Align::Center)
                        .desired_width(f32::INFINITY),
                );
                if ui.memory(|m| m.focused().is_none()) {
                    name_edit.request_focus();
                }
                ui.add_space(6.0);
                ui.label(crate::i18n::tr("Folder"));
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(path)
                            .hint_text(suggested.display().to_string())
                            .min_size(egui::vec2(0.0, theme::CONTROL_HEIGHT))
                            .vertical_align(egui::Align::Center)
                            .desired_width(400.0),
                    );
                    if ui.button(crate::i18n::tr("Browse…")).clicked() {
                        if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                            *path = dir.display().to_string();
                        }
                    }
                });
                ui.label(
                    RichText::new(crate::i18n::tr(
                        "Tip: pick a folder inside a git repository to share it with your team.",
                    ))
                    .small()
                    .color(p.text_weak),
                );
                if let Some(e) = error {
                    ui.label(RichText::new(e.as_str()).small().color(theme::RED));
                }
                ui.add_space(10.0);
                let mut create = false;
                let mut cancel = false;
                ui.horizontal(|ui| {
                    create = crate::widgets::primary_button(ui, crate::i18n::tr("Create")).clicked();
                    cancel = ui.button(crate::i18n::tr("Cancel")).clicked();
                });
                (create, cancel)
            });
        let (create, cancel) = r.inner;
        if cancel || r.should_close() {
            return true;
        }
        if !create {
            return false;
        }
        let name_ = name.trim();
        if name_.is_empty() {
            *error = Some(crate::i18n::tr("Give the workspace a name.").to_string());
            return false;
        }
        let folder = if path.trim().is_empty() { suggested } else { PathBuf::from(path.trim()) };
        // New workspaces start with this one's look and HTTP settings, but no trust.
        let mut settings = self.ws.settings.clone();
        settings.trusted_collections.clear();
        settings.network_collections.clear();
        settings.active_environment = None;
        match storage::create_workspace(&folder, &settings) {
            Ok(()) => {
                self.open_workspace_folder(folder, Some(name_.to_string()));
                true
            }
            Err(e) => {
                *error = Some(e.to_string());
                false
            }
        }
    }
}

/// `<Documents>/PakPos/<name>` (or next to the default workspace).
fn suggested_folder(name: &str) -> PathBuf {
    let slug: String = name
        .trim()
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '-' })
        .collect::<String>()
        .trim_matches('-')
        .to_string();
    let slug = if slug.is_empty() { "workspace".to_string() } else { slug };
    let base = directories::UserDirs::new()
        .and_then(|d| d.document_dir().map(Path::to_path_buf))
        .map(|d| d.join("PakPos"))
        .unwrap_or_else(|| storage::data_dir().join("workspaces"));
    base.join(slug)
}
