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
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), height), Sense::click_and_drag());
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

/// What is being dragged in the tree.
#[derive(Clone)]
struct DragItem {
    collection: String,
    id: String,
    name: String,
    is_folder: bool,
}

/// Drop position from the pointer's height in the row.
fn drop_zone(resp: &egui::Response, is_folder: bool) -> DropZone {
    let y = resp.ctx.pointer_interact_pos().map(|p| p.y).unwrap_or(resp.rect.center().y);
    let rel = (y - resp.rect.top()) / resp.rect.height().max(1.0);
    match (is_folder, rel) {
        (true, r) if r < 0.28 => DropZone::Before,
        (true, r) if r > 0.72 => DropZone::After,
        (true, _) => DropZone::Into,
        (false, r) if r < 0.5 => DropZone::Before,
        _ => DropZone::After,
    }
}

#[derive(Clone, Copy, PartialEq)]
enum DropZone {
    Before,
    After,
    Into,
}

/// Makes a row draggable and a drop target; returns the move command on drop.
fn drag_and_drop(ui: &Ui, resp: &egui::Response, me: DragItem, collection: &str) -> Option<ToggleNode> {
    resp.dnd_set_drag_payload(me.clone());
    let hovered = resp.dnd_hover_payload::<DragItem>();
    let released = resp.dnd_release_payload::<DragItem>();
    let payload = released.as_ref().or(hovered.as_ref())?;
    if payload.id == me.id {
        return None;
    }
    // Rows are collection roots when `me.id` is the collection id.
    let is_root = me.id == collection;
    let zone = if is_root { DropZone::Into } else { drop_zone(resp, me.is_folder) };
    let r = resp.rect;
    let painter = ui.painter();
    match zone {
        DropZone::Before => {
            painter.hline(r.x_range(), r.top(), Stroke::new(2.0, theme::ORANGE));
        }
        DropZone::After => {
            painter.hline(r.x_range(), r.bottom(), Stroke::new(2.0, theme::ORANGE));
        }
        DropZone::Into => {
            painter.rect_stroke(r, 5.0, Stroke::new(1.5, theme::ORANGE), egui::StrokeKind::Inside);
        }
    }
    let payload = released?;
    let at = match zone {
        _ if is_root => "into-root".to_string(),
        DropZone::Before => format!("before|{}", me.id),
        DropZone::After => format!("after|{}", me.id),
        DropZone::Into => format!("into|{}", me.id),
    };
    Some(ToggleNode(format!("move:{}:{}:{collection}|{at}", payload.collection, payload.id)))
}

/// A label following the pointer while something is dragged.
fn drag_ghost(ctx: &egui::Context, p: &Palette) {
    let Some(payload) = egui::DragAndDrop::payload::<DragItem>(ctx) else { return };
    let Some(pos) = ctx.pointer_interact_pos() else { return };
    egui::Area::new(egui::Id::new("tree_drag_ghost"))
        .order(egui::Order::Tooltip)
        .fixed_pos(pos + egui::vec2(14.0, 6.0))
        .interactable(false)
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(p.surface)
                .stroke(Stroke::new(1.0, theme::ORANGE))
                .corner_radius(6)
                .inner_margin(6)
                .show(ui, |ui| {
                    ui.label(
                        RichText::new(format!("{} {}", if payload.is_folder { "🗀" } else { "↕" }, payload.name))
                            .color(p.text),
                    );
                });
        });
    ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
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
                        (SidebarView::Collections, crate::i18n::tr("Collections").into()),
                        (SidebarView::Environments, crate::i18n::tr("Environments").into()),
                        (SidebarView::History, crate::i18n::tr("History").into()),
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
            if ui.button("+").on_hover_text(crate::i18n::tr("New collection")).clicked() {
                self.actions.push(Action::Dialog(Dialog::NewCollection { name: String::new() }));
            }
            ui.add(
                egui::TextEdit::singleline(&mut self.search)
                    .min_size(egui::vec2(0.0, crate::theme::CONTROL_HEIGHT))
                    .vertical_align(egui::Align::Center)
                    .hint_text(crate::i18n::tr("🔍 Search requests"))
                    .desired_width(ui.available_width()),
            );
        });
        ui.add_space(6.0);

        if self.ws.collections.is_empty() {
            widgets::empty_state(
                ui,
                &p,
                crate::i18n::tr("No collections yet"),
                crate::i18n::tr("Create one with + or import a collection file."),
            );
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
                let root =
                    DragItem { collection: c.id.clone(), id: c.id.clone(), name: c.name.clone(), is_folder: true };
                if let Some(cmd) = drag_and_drop(ui, &resp, root, &c.id) {
                    actions.push(cmd);
                }
                resp.context_menu(|ui| {
                    if ui.button(crate::i18n::tr("➕ Add request")).clicked() {
                        pending.push(Action::NewRequestIn { collection: c.id.clone(), folder: None });
                    }
                    if ui.button(crate::i18n::tr("📁 Add folder")).clicked() {
                        pending.push(Action::NewFolderIn { collection: c.id.clone(), folder: None });
                    }
                    if ui.button(crate::i18n::tr("⚙ Edit collection…")).clicked() {
                        pending.push(Action::Dialog(Dialog::CollectionSettings {
                            id: c.id.clone(),
                            tab: crate::app::CollectionTab::General,
                        }));
                    }
                    if self.mock.as_ref().is_some_and(|m| m.collection == c.id) {
                        if ui.button(crate::i18n::tr("■ Stop mock server")).clicked() {
                            pending.push(Action::StopMock);
                        }
                    } else if ui.button(crate::i18n::tr("◉ Start mock server")).clicked() {
                        pending.push(Action::StartMock(c.id.clone()));
                    }
                    if ui.button(crate::i18n::tr("▶ Run collection")).clicked() {
                        pending.push(Action::RunCollection(c.id.clone()));
                    }
                    ui.separator();
                    if ui.button(crate::i18n::tr("✏ Rename")).clicked() {
                        pending.push(Action::Rename(RenameTarget::Collection(c.id.clone()), c.name.clone()));
                    }
                    if ui.button(crate::i18n::tr("⬆ Export (Collection v2.1)…")).clicked() {
                        pending.push(Action::ExportCollection(c.id.clone()));
                    }
                    if ui.button(crate::i18n::tr("⬆ Export as OpenAPI 3…")).clicked() {
                        pending.push(Action::ExportOpenApi(c.id.clone()));
                    }
                    ui.separator();
                    if ui.button(RichText::new(crate::i18n::tr("🗑 Delete")).color(theme::RED)).clicked() {
                        pending.push(Action::Delete(DeleteTarget::Collection(c.id.clone()), c.name.clone()));
                    }
                });
                if open {
                    self.items_tree(ui, &p, &c.id, &c.items, None, 1, &filter, active_source.as_ref(), &mut actions);
                }
                ui.add_space(4.0);
            }
        });
        drag_ghost(ui.ctx(), &p);
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
                    let me = DragItem {
                        collection: collection.to_string(),
                        id: f.id.clone(),
                        name: f.name.clone(),
                        is_folder: true,
                    };
                    if let Some(cmd) = drag_and_drop(ui, &resp, me, collection) {
                        toggles.push(cmd);
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
                    let me = DragItem {
                        collection: collection.to_string(),
                        id: r.id.clone(),
                        name: r.name.clone(),
                        is_folder: false,
                    };
                    if let Some(cmd) = drag_and_drop(ui, &resp, me, collection) {
                        toggles.push(cmd);
                    }
                    resp.context_menu(|ui| {
                        if ui.button(crate::i18n::tr("Open")).clicked() {
                            toggles.push(ToggleNode(format!("open:{collection}:{}", r.id)));
                        }
                        if ui.button(crate::i18n::tr("📋 Duplicate")).clicked() {
                            toggles.push(ToggleNode(format!("dup:{collection}:{}", r.id)));
                        }
                        if ui.button(crate::i18n::tr("✏ Rename")).clicked() {
                            toggles.push(ToggleNode(format!("ren:{collection}:{}:{}", r.id, r.name)));
                        }
                        ui.separator();
                        if ui.button(RichText::new(crate::i18n::tr("🗑 Delete")).color(theme::RED)).clicked() {
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
            if ui.button(crate::i18n::tr("+ New")).clicked() {
                pending.push(Action::NewEnvironment);
            }
            if ui.button(crate::i18n::tr("Import…")).clicked() {
                pending.push(Action::ImportEnvironment);
            }
            if ui
                .button(crate::i18n::tr("Globals"))
                .on_hover_text(crate::i18n::tr("Variables available in every environment"))
                .clicked()
            {
                pending.push(Action::EditEnvironment(None));
            }
        });
        ui.add_space(8.0);
        if self.ws.environments.is_empty() {
            widgets::empty_state(
                ui,
                &p,
                crate::i18n::tr("No environments"),
                crate::i18n::tr("Environments hold variables like {{base_url}}."),
            );
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
                        weak_suffix: Some(crate::i18n::trf(
                            "{n} vars",
                            &[("n", &e.variables.iter().filter(|v| !v.key.is_empty()).count().to_string())],
                        )),
                    },
                );
                if resp.clicked() {
                    pending.push(Action::SetActiveEnvironment(if is_active { None } else { Some(e.id.clone()) }));
                }
                if resp.double_clicked() {
                    pending.push(Action::EditEnvironment(Some(e.id.clone())));
                }
                resp.context_menu(|ui| {
                    if ui.button(crate::i18n::tr("✏ Edit variables")).clicked() {
                        pending.push(Action::EditEnvironment(Some(e.id.clone())));
                    }
                    if ui.button(crate::i18n::tr("Rename")).clicked() {
                        pending.push(Action::Rename(RenameTarget::Environment(e.id.clone()), e.name.clone()));
                    }
                    if ui.button(crate::i18n::tr("⬆ Export…")).clicked() {
                        pending.push(Action::ExportEnvironment(e.id.clone()));
                    }
                    ui.separator();
                    if ui.button(RichText::new(crate::i18n::tr("🗑 Delete")).color(theme::RED)).clicked() {
                        pending.push(Action::Delete(DeleteTarget::Environment(e.id.clone()), e.name.clone()));
                    }
                });
            }
            ui.add_space(10.0);
            ui.label(
                RichText::new(crate::i18n::tr("Click to activate · double-click to edit")).small().color(p.text_weak),
            );
        });
        self.actions.extend(pending);
    }

    fn history_view(&mut self, ui: &mut Ui) {
        let p = self.palette;
        let mut pending: Vec<Action> = Vec::new();
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(crate::i18n::trf("{n} requests", &[("n", &self.ws.history.len().to_string())]))
                    .color(p.text_weak),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if !self.ws.history.is_empty() && ui.button(crate::i18n::tr("Clear")).clicked() {
                    pending.push(Action::ClearHistory);
                }
            });
        });
        ui.add_space(6.0);
        if self.ws.history.is_empty() {
            widgets::empty_state(
                ui,
                &p,
                crate::i18n::tr("No history yet"),
                crate::i18n::tr("Requests you send appear here."),
            );
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
    if ui.button(crate::i18n::tr("➕ Add request")).clicked() {
        out.push(ToggleNode(format!("newreq:{collection}:{folder}")));
    }
    if ui.button(crate::i18n::tr("📁 Add folder")).clicked() {
        out.push(ToggleNode(format!("newfolder:{collection}:{folder}")));
    }
    if ui.button(crate::i18n::tr("✏ Rename")).clicked() {
        out.push(ToggleNode(format!("ren:{collection}:{folder}:{name}")));
    }
    ui.separator();
    if ui.button(RichText::new(crate::i18n::tr("🗑 Delete")).color(theme::RED)).clicked() {
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
            "move" => {
                // name = "<target collection>|before|<id>", "|after|<id>", "|into|<id>" or "|into-root"
                let mut parts = name.splitn(3, '|');
                let to = parts.next().unwrap_or("").to_string();
                let mode = parts.next().unwrap_or("");
                let target = parts.next().unwrap_or("").to_string();
                let at = match mode {
                    "before" => pakpos_core::model::DropTarget::Before(target),
                    "after" => pakpos_core::model::DropTarget::After(target),
                    "into" => pakpos_core::model::DropTarget::Into(Some(target)),
                    _ => pakpos_core::model::DropTarget::Into(None),
                };
                Action::MoveItem { from: collection, item, to, at }
            }
            _ => return false,
        };
        self.actions.push(action);
        true
    }
}
