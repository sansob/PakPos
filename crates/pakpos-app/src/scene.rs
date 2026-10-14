//! Scripted demo scenes, used to produce the screenshots in the docs.
//!
//! `PAKPOS_SCENE=<name> PAKPOS_SCREENSHOT=out.png pakpos` sets up a scene,
//! waits until it is ready, saves a screenshot and exits. Not used otherwise.

use crate::app::{Dialog, PakPosApp, ReqSection, RespSection, ResponseState};
use pakpos_core::codegen::SnippetLang;
use pakpos_core::storage::Theme;
use std::sync::Mutex;
use std::time::{Duration, Instant};

struct SceneState {
    name: String,
    output: Option<String>,
    started: Instant,
    setup_done: bool,
    ready_at: Option<Instant>,
    requested: bool,
}

static SCENE: Mutex<Option<SceneState>> = Mutex::new(None);

pub fn init_from_env() {
    if let Ok(name) = std::env::var("PAKPOS_SCENE") {
        *SCENE.lock().unwrap() = Some(SceneState {
            name,
            output: std::env::var("PAKPOS_SCREENSHOT").ok(),
            started: Instant::now(),
            setup_done: false,
            ready_at: None,
            requested: false,
        });
    }
}

fn open_by_name(app: &mut PakPosApp, name: &str) {
    let found =
        app.ws.collections.iter().find_map(|c| {
            c.flatten().into_iter().find(|(_, r)| r.name == name).map(|(_, r)| (c.id.clone(), r.id.clone()))
        });
    if let Some((cid, rid)) = found {
        for c in &app.ws.collections {
            app.open_nodes.insert(c.id.clone());
            for item in &c.items {
                if let pakpos_core::Item::Folder(f) = item {
                    app.open_nodes.insert(f.id.clone());
                }
            }
        }
        app.open_saved(&cid, &rid);
    }
}

fn setup(app: &mut PakPosApp, name: &str, ctx: &egui::Context) {
    app.ws.settings.active_environment = app.ws.environments.first().map(|e| e.id.clone());
    match name {
        "light" => {
            app.ws.settings.theme = Theme::Light;
            open_by_name(app, "Create user (JSON)");
            app.tabs[app.active].section = ReqSection::Body;
            app.send(app.active, ctx);
        }
        "tests" => {
            open_by_name(app, "Get with query params");
            app.tabs[app.active].section = ReqSection::Tests;
            app.tabs[app.active].resp_section = RespSection::Tests;
            app.send(app.active, ctx);
        }
        "runner" => {
            open_by_name(app, "Bearer token");
            app.tabs[app.active].section = ReqSection::Auth;
            let id = app.ws.collections[0].id.clone();
            app.start_runner(&id, ctx);
        }
        "environments" => {
            open_by_name(app, "Get with query params");
            let id = app.ws.settings.active_environment.clone();
            app.dialog = Some(Dialog::Environments { selected: id });
        }
        "curl" => {
            app.dialog = Some(Dialog::ImportCurl {
                text: "curl 'https://api.example.com/v1/orders?status=paid' \\\n  -H 'Authorization: Bearer {{token}}' \\\n  -H 'Content-Type: application/json' \\\n  --data-raw '{\"customer\":\"Siti\",\"total\":150000}'".into(),
                error: None,
            });
        }
        "snippet" => {
            open_by_name(app, "Create user (JSON)");
            app.dialog = Some(Dialog::Snippet { lang: SnippetLang::Python });
        }
        "history" => {
            let reqs: Vec<_> = app.ws.collections[0].flatten().into_iter().map(|(_, r)| r).collect();
            let statuses = [200, 200, 201, 200, 404, 200, 401, 200, 500, 200];
            for (i, r) in reqs.iter().cycle().take(10).enumerate() {
                let mut r = r.clone();
                r.url = r.url.replace("{{base_url}}", "https://api.example.com");
                app.ws.push_history(r, Some(statuses[i]), Some(40 + i as u128 * 13));
            }
            app.sidebar = crate::app::SidebarView::History;
            open_by_name(app, "Bearer token");
            app.tabs[app.active].section = ReqSection::Auth;
            app.send(app.active, ctx);
        }
        "oauth" => {
            open_by_name(app, "Profile (OAuth 2.0)");
            app.tabs[app.active].section = ReqSection::Auth;
            app.send(app.active, ctx);
        }
        "settings" => {
            open_by_name(app, "Get with query params");
            app.dialog = Some(Dialog::Settings);
        }
        _ => {
            open_by_name(app, "Create user (JSON)");
            app.tabs[app.active].section = ReqSection::Body;
            open_by_name(app, "Get with query params");
            app.send(app.active, ctx);
        }
    }
}

fn is_ready(app: &PakPosApp, name: &str) -> bool {
    match name {
        "runner" => app.runner.as_ref().is_some_and(|r| r.report.is_some()),
        "environments" | "curl" | "snippet" | "settings" => true,
        _ => app
            .tabs
            .get(app.active)
            .is_some_and(|t| matches!(t.response, ResponseState::Done(_) | ResponseState::Error(_))),
    }
}

pub fn tick(app: &mut PakPosApp, ctx: &egui::Context) {
    let mut guard = SCENE.lock().unwrap();
    let Some(state) = guard.as_mut() else { return };
    if !state.setup_done {
        state.setup_done = true;
        let name = state.name.clone();
        drop(guard);
        setup(app, &name, ctx);
        ctx.request_repaint();
        return;
    }
    if state.ready_at.is_none() && (is_ready(app, &state.name) || state.started.elapsed() > Duration::from_secs(20)) {
        state.ready_at = Some(Instant::now());
    }
    ctx.request_repaint_after(Duration::from_millis(100));
    let Some(ready) = state.ready_at else { return };
    if ready.elapsed() < Duration::from_millis(900) {
        return;
    }
    let Some(output) = state.output.clone() else { return };
    if !state.requested {
        state.requested = true;
        ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
        return;
    }
    let shot = ctx.input(|i| {
        i.raw.events.iter().find_map(|e| match e {
            egui::Event::Screenshot { image, .. } => Some(image.clone()),
            _ => None,
        })
    });
    if let Some(image) = shot {
        let [w, h] = image.size;
        let mut buf = Vec::with_capacity(w * h * 4);
        for px in &image.pixels {
            buf.extend_from_slice(&px.to_array());
        }
        if let Some(img) = image::RgbaImage::from_raw(w as u32, h as u32, buf) {
            if let Err(e) = img.save(&output) {
                eprintln!("screenshot failed: {e}");
            } else {
                eprintln!("saved {output}");
            }
        }
        *guard = None;
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }
}
