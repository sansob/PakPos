//! Applies deferred [`Action`]s produced while drawing.

use crate::app::{Action, Dialog, PakPosApp, RenameTarget};
use pakpos_core::model::{Folder, Item};
use pakpos_core::{postman, storage, Environment, Request};

pub fn apply(app: &mut PakPosApp, action: Action) {
    match action {
        Action::OpenRequest { collection, request } => app.open_saved(&collection, &request),
        Action::NewTab(req) => {
            app.active = app.push_tab(*req, None);
        }
        Action::NewRequestIn { collection, folder } => {
            let req = Request { name: "New request".into(), ..Default::default() };
            let rid = req.id.clone();
            if let Some(c) = app.collection_mut(&collection) {
                let target = match &folder {
                    Some(f) => c.find_folder_mut(f).map(|f| &mut f.items),
                    None => None,
                };
                match target {
                    Some(items) => items.push(Item::Request(req)),
                    None => c.items.push(Item::Request(req)),
                }
            }
            app.open_nodes.insert(collection.clone());
            if let Some(f) = folder {
                app.open_nodes.insert(f);
            }
            app.persist_collection(&collection);
            app.open_saved(&collection, &rid);
        }
        Action::NewFolderIn { collection, folder } => {
            let f = Folder::new("New folder");
            let fid = f.id.clone();
            if let Some(c) = app.collection_mut(&collection) {
                let target = match &folder {
                    Some(parent) => c.find_folder_mut(parent).map(|f| &mut f.items),
                    None => None,
                };
                match target {
                    Some(items) => items.push(Item::Folder(f)),
                    None => c.items.push(Item::Folder(f)),
                }
            }
            app.open_nodes.insert(collection.clone());
            app.open_nodes.insert(fid.clone());
            app.persist_collection(&collection);
            app.dialog = Some(Dialog::Rename {
                target: RenameTarget::Item { collection, item: fid },
                name: "New folder".into(),
            });
        }
        Action::MoveItem { from, item, to, at } => {
            if pakpos_core::model::move_item(&mut app.ws.collections, &from, &item, &to, &at) {
                app.persist_collection(&from);
                if from != to {
                    app.persist_collection(&to);
                    // Open tabs follow their requests to the other collection.
                    let moved: Vec<String> = app
                        .collection(&to)
                        .map(|c| c.flatten().into_iter().map(|(_, r)| r.id).collect())
                        .unwrap_or_default();
                    for tab in &mut app.tabs {
                        if let Some((c, r)) = &mut tab.source {
                            if *c == from && moved.contains(r) {
                                *c = to.clone();
                            }
                        }
                    }
                    app.open_nodes.insert(to);
                }
            }
        }
        Action::Duplicate { collection, request } => {
            let mut new_id = None;
            if let Some(c) = app.collection_mut(&collection) {
                if let Some(r) = c.find_request(&request) {
                    let copy = r.duplicate();
                    new_id = Some(copy.id.clone());
                    insert_after(&mut c.items, &request, Item::Request(copy));
                }
            }
            app.persist_collection(&collection);
            if let Some(id) = new_id {
                app.open_saved(&collection, &id);
            }
        }
        Action::Rename(target, name) => app.dialog = Some(Dialog::Rename { target, name }),
        Action::Delete(target, label) => app.dialog = Some(Dialog::ConfirmDelete { target, label }),
        Action::ExportCollection(id) => {
            let Some(c) = app.collection(&id).cloned() else { return };
            let file = format!("{}.collection.json", sanitize(&c.name));
            if let Some(path) = rfd::FileDialog::new().set_file_name(file).add_filter("JSON", &["json"]).save_file() {
                match std::fs::write(&path, postman::export_collection(&c)) {
                    Ok(()) => app.toast(format!("Exported to {}", path.display())),
                    Err(e) => app.toast_error(e.to_string()),
                }
            }
        }
        Action::RunCollection(id) => {
            if let Some(name) = app.collection(&id).map(|c| c.name.clone()) {
                app.runner_setup = Some(crate::app::RunnerSetup::new(&id, &name));
            }
        }
        Action::ImportPostmanCollection => {
            if let Some(paths) = rfd::FileDialog::new().add_filter("JSON", &["json"]).pick_files() {
                for path in paths {
                    match storage::load_collection_file(&path) {
                        Ok((mut c, warnings)) => {
                            crate::app::clean_items(&mut c.items);
                            // Re-importing the same file should not clash with an existing id.
                            if app.collection(&c.id).is_some() {
                                c.id = pakpos_core::model::new_id();
                            }
                            app.open_nodes.insert(c.id.clone());
                            let id = c.id.clone();
                            let msg = format!("Imported “{}” ({} requests)", c.name, c.request_count());
                            app.ws.collections.push(c);
                            app.persist_collection(&id);
                            app.toast(msg);
                            if !warnings.is_empty() {
                                app.toast_error(format!(
                                    "{} warning(s): {}",
                                    warnings.len(),
                                    warnings.first().cloned().unwrap_or_default()
                                ));
                            }
                        }
                        Err(e) => app.toast_error(e),
                    }
                }
            }
        }
        Action::ImportEnvironment => {
            if let Some(path) = rfd::FileDialog::new().add_filter("JSON", &["json"]).pick_file() {
                match storage::load_environment_file(&path) {
                    Ok(mut env) => {
                        if app.ws.environments.iter().any(|e| e.id == env.id) {
                            env.id = pakpos_core::model::new_id();
                        }
                        app.toast(format!("Imported environment “{}”", env.name));
                        app.ws.environments.push(env);
                        app.persist_environments();
                    }
                    Err(e) => app.toast_error(e),
                }
            }
        }
        Action::ExportEnvironment(id) => {
            let Some(env) = app.ws.environments.iter().find(|e| e.id == id).cloned() else { return };
            let file = format!("{}.environment.json", sanitize(&env.name));
            if let Some(path) = rfd::FileDialog::new().set_file_name(file).save_file() {
                match std::fs::write(&path, postman::export_environment(&env)) {
                    Ok(()) => app.toast(format!("Exported to {}", path.display())),
                    Err(e) => app.toast_error(e.to_string()),
                }
            }
        }
        Action::SetActiveEnvironment(id) => {
            app.ws.settings.active_environment = id;
            app.persist_settings();
        }
        Action::EditEnvironment(id) => app.dialog = Some(Dialog::Environments { selected: id }),
        Action::NewEnvironment => {
            let env = Environment::new(format!("Environment {}", app.ws.environments.len() + 1));
            let id = env.id.clone();
            app.ws.environments.push(env);
            app.persist_environments();
            app.dialog = Some(Dialog::Environments { selected: Some(id) });
        }
        Action::ClearHistory => {
            app.ws.history.clear();
            let _ = app.ws.save_history();
        }
        Action::Dialog(d) => app.dialog = Some(d),
        Action::StartMock(id) => {
            app.stop_mock();
            app.start_mock(&id);
        }
        Action::StopMock => app.stop_mock(),
        Action::ShowMock => {
            if let Some(m) = &mut app.mock {
                m.open = true;
            }
        }
    }
}

fn insert_after(items: &mut Vec<Item>, id: &str, new: Item) -> bool {
    if let Some(pos) = items.iter().position(|i| i.id() == id) {
        items.insert(pos + 1, new);
        return true;
    }
    for item in items.iter_mut() {
        if let Item::Folder(f) = item {
            if insert_after(&mut f.items, id, new.clone()) {
                return true;
            }
        }
    }
    false
}

fn sanitize(name: &str) -> String {
    name.chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect()
}
