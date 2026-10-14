//! Left sidebar: collections tree, environments and history.

use crate::app::{Action, DeleteTarget, Dialog, PakPosApp, RenameTarget, SidebarView};
use crate::theme::{self, Palette};
use crate::widgets;
use egui::{Align2, FontId, Margin, RichText, Sense, Stroke, Ui};
use pakpos_core::model::Item;

struct RowStyle<'a> {
    depth: usize,
    badge: Option<(&'static str, egui::Color32)>,
    icon: Option<&'a str>,
    label: &'a str,
    selected: bool,
    weak_suffix: Option<String>,
}

/// Paints one full-width tree row and returns its response.
fn tree_row(ui: &mut Ui, p: &Palette, s: RowStyle<'_>) -> egui::Response {
    let height = 26.0;
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), height), Sense::click());
    let painter = ui.painter();
    if s.selected {
        painter.rect_filled(rect, 5.0, if p.dark { theme::NAVY_700 } else { p.surface_alt });
        painter.rect_filled(egui::Rect::from_min_size(rect.min, egui::vec2(3.0, rect.height())), 1.5, theme::ORANGE);
    } else if resp.hovered() {
        painter.rect_filled(rect, 5.0, if p.dark { theme::NAVY_850 } else { p.surface_alt });
    }
    let mut x = rect.left() + 8.0 + s.depth as f32 * 14.0;
    let cy = rect.center().y;
    if let Some(icon) = s.icon {
        painter.text(egui::pos2(x, cy), Align2::LEFT_CENTER, icon, FontId::proportional(12.0), p.text_weak);
        x += 16.0;
    }
    if let Some((label, color)) = s.badge {
        painter.text(egui::pos2(x, cy), Align2::LEFT_CENTER, label, FontId::monospace(10.5), color);
        x += 42.0;
    }
    let mut right = rect.right() - 8.0;
    if let Some(suffix) = s.weak_suffix {
        let g = painter.layout_no_wrap(suffix, FontId::proportional(11.0), p.text_weak);
        right -= g.size().x;
        painter.galley(egui::pos2(right, cy - g.size().y / 2.0), g, p.text_weak);
        right -= 10.0;
    }
    let mut job = egui::text::LayoutJob::single_section(
        s.label.to_string(),
        egui::TextFormat { font_id: FontId::proportional(13.5), color: p.text, ..Default::default() },
    );
    job.wrap = egui::text::TextWrapping {
        max_width: (right - x).max(10.0),
        max_rows: 1,
        break_anywhere: true,
        overflow_character: Some('…'),
    };
    let galley = ui.ctx().fonts_mut(|f| f.layout_job(job));
    painter.galley(egui::pos2(x, cy - galley.size().y / 2.0), galley, p.text);
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

fn matches(filter: &str, item: &Item) -> bool {
    if filter.is_empty() {
        return true;
    }
    match item {
        Item::Request(r) => r.name.to_lowercase().contains(filter) || r.url.to_lowercase().contains(filter),
        Item::Folder(f) => f.name.to_lowercase().contains(filter) || f.items.iter().any(|i| matches(filter, i)),
    }
}

impl PakPosApp {
    pub fn sidebar(&mut self, ui: &mut Ui) {
        let p = self.palette;
        egui::Panel::left("sidebar")
            .resizable(true)
            .default_size(290.0)
            .min_size(220.0)
            .max_size(560.0)
            .frame(
                egui::Frame::new()
                    .fill(p.sidebar)
                    .inner_margin(Margin::symmetric(10, 10))
                    .stroke(Stroke::new(1.0, p.border)),
            )
            .show(ui, |ui| {
                let mut view = self.sidebar;
                widgets::tab_bar(
                    ui,
                    &mut view,
                    &[
                        (SidebarView::Collections, "Collections".into()),
                        (SidebarView::Environments, "Environments".into()),
                        (SidebarView::History, "History".into()),
                    ],
                );
                self.sidebar = view;
                ui.add_space(2.0);
                match self.sidebar {
                    SidebarView::Collections => self.collections_view(ui),
                    SidebarView::Environments => self.environments_view(ui),
                    SidebarView::History => self.history_view(ui),
                }
            });
    }

    fn collections_view(&mut self, ui: &mut Ui) {
        let p = self.palette;
        // Right-to-left so the search box takes exactly the space left by the
        // button (sizing it as "available - N" makes the panel grow every frame).
        let row = egui::vec2(ui.available_width(), ui.spacing().interact_size.y);
        ui.allocate_ui_with_layout(row, egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("+").on_hover_text("New collection").clicked() {
                self.actions.push(Action::Dialog(Dialog::NewCollection { name: String::new() }));
            }
            ui.add(
                egui::TextEdit::singleline(&mut self.search)
                    .hint_text("🔍 Search requests")
                    .desired_width(ui.available_width()),
            );
        });
        ui.add_space(6.0);

        if self.ws.collections.is_empty() {
            widgets::empty_state(ui, &p, "No collections yet", "Create one with + or import a Postman file.");
            return;
        }

        let filter = self.search.trim().to_lowercase();
        let active_source = self.tabs.get(self.active).and_then(|t| t.source.clone());
        let mut actions = Vec::new();
        let mut pending: Vec<Action> = Vec::new();
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            for c in &self.ws.collections {
                if !filter.is_empty()
                    && !c.items.iter().any(|i| matches(&filter, i))
                    && !c.name.to_lowercase().contains(&filter)
                {
                    continue;
                }
                let open = self.open_nodes.contains(&c.id) || !filter.is_empty();
                let resp = tree_row(
                    ui,
                    &p,
                    RowStyle {
                        depth: 0,
                        badge: None,
                        icon: Some(if open { "▾" } else { "▸" }),
                        label: &c.name,
                        selected: false,
                        weak_suffix: Some(c.request_count().to_string()),
                    },
                );
                if resp.clicked() {
                    actions.push(ToggleNode(c.id.clone()));
                }
                resp.context_menu(|ui| {
                    if ui.button("➕ Add request").clicked() {
                        pending.push(Action::NewRequestIn { collection: c.id.clone(), folder: None });
                    }
                    if ui.button("📁 Add folder").clicked() {
                        pending.push(Action::NewFolderIn { collection: c.id.clone(), folder: None });
                    }
                    if ui.button("⚙ Edit collection…").clicked() {
                        pending.push(Action::Dialog(Dialog::CollectionSettings {
                            id: c.id.clone(),
                            tab: crate::app::CollectionTab::General,
                        }));
                    }
                    if ui.button("▶ Run collection").clicked() {
                        pending.push(Action::RunCollection(c.id.clone()));
                    }
                    ui.separator();
                    if ui.button("✏ Rename").clicked() {
                        pending.push(Action::Rename(RenameTarget::Collection(c.id.clone()), c.name.clone()));
                    }
                    if ui.button("⬆ Export (Postman v2.1)…").clicked() {
                        pending.push(Action::ExportCollection(c.id.clone()));
                    }
                    ui.separator();
                    if ui.button(RichText::new("🗑 Delete").color(theme::RED)).clicked() {
                        pending.push(Action::Delete(DeleteTarget::Collection(c.id.clone()), c.name.clone()));
                    }
                });
                if open {
                    self.items_tree(ui, &p, &c.id, &c.items, None, 1, &filter, active_source.as_ref(), &mut actions);
                }
                ui.add_space(4.0);
            }
        });
        self.actions.extend(pending);
        for ToggleNode(id) in actions {
            if self.handle_tree_command(&id) {
                continue;
            }
            if !self.open_nodes.remove(&id) {
                self.open_nodes.insert(id);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn items_tree(
        &self,
        ui: &mut Ui,
        p: &Palette,
        collection: &str,
        items: &[Item],
        _parent: Option<&str>,
        depth: usize,
        filter: &str,
        active: Option<&(String, String)>,
        toggles: &mut Vec<ToggleNode>,
    ) {
        for item in items {
            if !matches(filter, item) {
                continue;
            }
            match item {
                Item::Folder(f) => {
                    let open = self.open_nodes.contains(&f.id) || !filter.is_empty();
                    let resp = tree_row(
                        ui,
                        p,
                        RowStyle {
                            depth,
                            badge: None,
                            icon: Some(if open { "▾" } else { "▸" }),
                            label: &format!("{} {}", if open { "🗁" } else { "🗀" }, f.name),
                            selected: false,
                            weak_suffix: None,
                        },
                    );
                    if resp.clicked() {
                        toggles.push(ToggleNode(f.id.clone()));
                    }
                    resp.context_menu(|ui| {
                        folder_menu(ui, collection, &f.id, &f.name, toggles);
                    });
                    if open {
                        self.items_tree(ui, p, collection, &f.items, Some(&f.id), depth + 1, filter, active, toggles);
                    }
                }
                Item::Request(r) => {
                    let selected = active.is_some_and(|(c, id)| c == collection && id == &r.id);
                    let resp = tree_row(
                        ui,
                        p,
                        RowStyle {
                            depth,
                            badge: Some(theme::badge(r)),
                            icon: None,
                            label: &r.name,
                            selected,
                            weak_suffix: None,
                        },
                    )
                    .on_hover_text(&r.url);
                    if resp.clicked() {
                        toggles.push(ToggleNode(format!("open:{collection}:{}", r.id)));
                    }
                    resp.context_menu(|ui| {
                        if ui.button("Open").clicked() {
                            toggles.push(ToggleNode(format!("open:{collection}:{}", r.id)));
                        }
                        if ui.button("📋 Duplicate").clicked() {
                            toggles.push(ToggleNode(format!("dup:{collection}:{}", r.id)));
                        }
                        if ui.button("✏ Rename").clicked() {
                            toggles.push(ToggleNode(format!("ren:{collection}:{}:{}", r.id, r.name)));
                        }
                        ui.separator();
                        if ui.button(RichText::new("🗑 Delete").color(theme::RED)).clicked() {
                            toggles.push(ToggleNode(format!("del:{collection}:{}:{}", r.id, r.name)));
                        }
                    });
                }
            }
        }
    }

    fn environments_view(&mut self, ui: &mut Ui) {
        let p = self.palette;
        let mut pending: Vec<Action> = Vec::new();
        ui.horizontal(|ui| {
            if ui.button("+ New").clicked() {
                pending.push(Action::NewEnvironment);
            }
            if ui.button("Import…").clicked() {
                pending.push(Action::ImportEnvironment);
            }
            if ui.button("Globals").on_hover_text("Variables available in every environment").clicked() {
                pending.push(Action::EditEnvironment(None));
            }
        });
        ui.add_space(8.0);
        if self.ws.environments.is_empty() {
            widgets::empty_state(ui, &p, "No environments", "Environments hold variables like {{base_url}}.");
            self.actions.extend(pending);
            return;
        }
        let active = self.ws.settings.active_environment.clone();
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            for e in &self.ws.environments {
                let is_active = active.as_deref() == Some(e.id.as_str());
                let resp = tree_row(
                    ui,
                    &p,
                    RowStyle {
                        depth: 0,
                        badge: None,
                        icon: Some(if is_active { "●" } else { "○" }),
                        label: &e.name,
                        selected: is_active,
                        weak_suffix: Some(format!("{} vars", e.variables.iter().filter(|v| !v.key.is_empty()).count())),
                    },
                );
                if resp.clicked() {
                    pending.push(Action::SetActiveEnvironment(if is_active { None } else { Some(e.id.clone()) }));
                }
                if resp.double_clicked() {
                    pending.push(Action::EditEnvironment(Some(e.id.clone())));
                }
                resp.context_menu(|ui| {
                    if ui.button("✏ Edit variables").clicked() {
                        pending.push(Action::EditEnvironment(Some(e.id.clone())));
                    }
                    if ui.button("Rename").clicked() {
                        pending.push(Action::Rename(RenameTarget::Environment(e.id.clone()), e.name.clone()));
                    }
                    if ui.button("⬆ Export (Postman)…").clicked() {
                        pending.push(Action::ExportEnvironment(e.id.clone()));
                    }
                    ui.separator();
                    if ui.button(RichText::new("🗑 Delete").color(theme::RED)).clicked() {
                        pending.push(Action::Delete(DeleteTarget::Environment(e.id.clone()), e.name.clone()));
                    }
                });
            }
            ui.add_space(10.0);
            ui.label(RichText::new("Click to activate · double-click to edit").small().color(p.text_weak));
        });
        self.actions.extend(pending);
    }

    fn history_view(&mut self, ui: &mut Ui) {
        let p = self.palette;
        let mut pending: Vec<Action> = Vec::new();
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("{} requests", self.ws.history.len())).color(p.text_weak));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if !self.ws.history.is_empty() && ui.button("Clear").clicked() {
                    pending.push(Action::ClearHistory);
                }
            });
        });
        ui.add_space(6.0);
        if self.ws.history.is_empty() {
            widgets::empty_state(ui, &p, "No history yet", "Requests you send appear here.");
            self.actions.extend(pending);
            return;
        }
        let now = chrono::Utc::now();
        let mut last_day = String::new();
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            for entry in &self.ws.history {
                let local = entry.timestamp.with_timezone(&chrono::Local);
                let day = local.format("%A, %d %b %Y").to_string();
                if day != last_day {
                    ui.add_space(6.0);
                    widgets::section_title(ui, &day);
                    last_day = day;
                }
                let ago = now.signed_duration_since(entry.timestamp);
                let when = if ago.num_minutes() < 1 {
                    "now".to_string()
                } else if ago.num_hours() < 1 {
                    format!("{}m", ago.num_minutes())
                } else {
                    local.format("%H:%M").to_string()
                };
                let suffix = match entry.status {
                    Some(s) => format!("{s} · {when}"),
                    None => format!("ERR · {when}"),
                };
                let resp = tree_row(
                    ui,
                    &p,
                    RowStyle {
                        depth: 0,
                        badge: Some(theme::badge(&entry.request)),
                        icon: None,
                        label: &entry.request.url,
                        selected: false,
                        weak_suffix: Some(suffix),
                    },
                )
                .on_hover_text(&entry.request.url);
                if resp.clicked() {
                    let mut req = entry.request.clone();
                    req.id = pakpos_core::model::new_id();
                    pending.push(Action::NewTab(Box::new(req)));
                }
            }
        });
        self.actions.extend(pending);
    }
}

/// Deferred tree interactions encoded as strings (keeps the recursive tree
/// rendering free of `&mut self` borrows).
pub struct ToggleNode(pub String);

fn folder_menu(ui: &mut Ui, collection: &str, folder: &str, name: &str, out: &mut Vec<ToggleNode>) {
    if ui.button("➕ Add request").clicked() {
        out.push(ToggleNode(format!("newreq:{collection}:{folder}")));
    }
    if ui.button("📁 Add folder").clicked() {
        out.push(ToggleNode(format!("newfolder:{collection}:{folder}")));
    }
    if ui.button("✏ Rename").clicked() {
        out.push(ToggleNode(format!("ren:{collection}:{folder}:{name}")));
    }
    ui.separator();
    if ui.button(RichText::new("🗑 Delete").color(theme::RED)).clicked() {
        out.push(ToggleNode(format!("del:{collection}:{folder}:{name}")));
    }
}

impl PakPosApp {
    /// Converts encoded tree interactions into actions.
    pub fn handle_tree_command(&mut self, cmd: &str) -> bool {
        let mut parts = cmd.splitn(4, ':');
        let kind = parts.next().unwrap_or("");
        let collection = parts.next().unwrap_or("").to_string();
        let item = parts.next().unwrap_or("").to_string();
        let name = parts.next().unwrap_or("").to_string();
        let action = match kind {
            "open" => Action::OpenRequest { collection, request: item },
            "dup" => Action::Duplicate { collection, request: item },
            "ren" => Action::Rename(RenameTarget::Item { collection, item }, name),
            "del" => Action::Delete(DeleteTarget::Item { collection, item }, name),
            "newreq" => Action::NewRequestIn { collection, folder: Some(item) },
            "newfolder" => Action::NewFolderIn { collection, folder: Some(item) },
            _ => return false,
        };
        self.actions.push(action);
        true
    }
}
