//! PakPos desktop application.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod actions;
mod app;
mod dialogs;
mod graphql_view;
mod oauth_view;
mod request_view;
mod response_view;
mod scene;
mod sidebar;
mod theme;
mod widgets;
mod ws_view;

use pakpos_core::storage::{self, Workspace};

fn load_icon() -> Option<egui::IconData> {
    let img = image::load_from_memory(include_bytes!("../../../assets/icon-256.png")).ok()?.into_rgba8();
    let (width, height) = img.dimensions();
    Some(egui::IconData { rgba: img.into_raw(), width, height })
}

/// Loads the workspace; seeds a demo collection + environment on first run.
fn load_workspace() -> (Workspace, Vec<String>) {
    let root = storage::default_root();
    let first_run = !root.exists();
    let (mut ws, errors) = Workspace::load(&root);
    let mut errors: Vec<String> = errors.into_iter().map(|e| e.to_string()).collect();
    if first_run && ws.collections.is_empty() && ws.environments.is_empty() {
        let base = std::env::var("PAKPOS_DEMO_BASE_URL").unwrap_or_else(|_| "https://httpbin.org".into());
        let env = pakpos_core::samples::demo_environment(&base);
        ws.settings.active_environment = Some(env.id.clone());
        ws.environments.push(env);
        ws.collections.push(pakpos_core::samples::demo_collection());
        if let Err(e) = ws.save_all() {
            errors.push(e.to_string());
        }
    }
    (ws, errors)
}

fn main() -> eframe::Result {
    scene::init_from_env();
    let (ws, errors) = load_workspace();

    let mut viewport = egui::ViewportBuilder::default()
        .with_title("PakPos")
        .with_app_id("dev.pakpos.PakPos")
        .with_inner_size([1360.0, 860.0])
        .with_min_inner_size([900.0, 560.0]);
    if let Some(icon) = load_icon() {
        viewport = viewport.with_icon(icon);
    }
    let scripted = std::env::var_os("PAKPOS_SCENE").is_some();
    let options = eframe::NativeOptions {
        viewport,
        // Scripted screenshot scenes must not reuse the user's UI session.
        persist_window: !scripted,
        persistence_path: scripted.then(|| storage::default_root().join("ui-state.ron")),
        ..Default::default()
    };
    eframe::run_native(
        "PakPos",
        options,
        Box::new(move |cc| {
            egui_extras::install_image_loaders(&cc.egui_ctx);
            // Hack has arrows and shapes (▸ ▾ ● →) missing from the UI font.
            let mut fonts = egui::FontDefinitions::default();
            if let Some(list) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
                list.push("Hack".to_owned());
            }
            cc.egui_ctx.set_fonts(fonts);
            Ok(Box::new(app::PakPosApp::new(cc, ws, errors)))
        }),
    )
}
