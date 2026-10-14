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
    sent: bool,
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
            sent: false,
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
    crate::i18n::set(crate::i18n::Lang::En);
    match name {
        "bahasa" => {
            crate::i18n::set(crate::i18n::Lang::Id);
            open_by_name(app, "Get with query params");
            app.tabs[app.active].section = ReqSection::Tests;
            app.tabs[app.active].resp_section = RespSection::Tests;
            app.send(app.active, ctx);
        }
        "workspaces" => {
            let team = app.ws.root.with_file_name("payments-team");
            app.registry.add("Payments team", &team);
            app.dialog =
                Some(crate::app::Dialog::NewWorkspace { name: "Mobile app".into(), path: String::new(), error: None });
        }
        "close-tabs" => {
            open_by_name(app, "Create user (JSON)");
            open_by_name(app, "Get with query params");
            open_by_name(app, "Update user");
            for t in &mut app.tabs[..2] {
                t.dirty = true;
            }
            let uids = app.tabs.iter().map(|t| t.uid).collect();
            app.request_close(uids, false);
        }
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
        "git" => {
            // A workspace with history, a remote and one pending change.
            let root = app.ws.root.clone();
            let remote = root.parent().unwrap_or(&root).join("team-remote.git");
            let sh = |dir: &std::path::Path, args: &[&str]| {
                let _ = std::process::Command::new("git").args(args).current_dir(dir).output();
            };
            let _ = std::fs::create_dir_all(&remote);
            sh(&remote, &["init", "-q", "--bare"]);
            let _ = pakpos_core::git::init(&root);
            sh(&root, &["config", "user.name", "Budi"]);
            sh(&root, &["config", "user.email", "budi@example.com"]);
            sh(&root, &["config", "commit.gpgsign", "false"]);
            let _ = pakpos_core::git::commit_all(&root, "Add PakPos demo collection");
            let _ = pakpos_core::git::set_remote(&root, &format!("file://{}", remote.display()));
            let _ = pakpos_core::git::push(&root);
            if let Some(c) = app.ws.collections.first_mut() {
                c.description.push_str("\n\nOwned by the payments team.");
            }
            let id = app.ws.collections[0].id.clone();
            app.persist_collection(&id);
            open_by_name(app, "Get with query params");
            app.git.open = true;
            app.git.message = "Document collection owner".into();
        }
        "grpc-stream" => {
            open_by_name(app, "Chat (gRPC stream)");
            app.tabs[app.active].section = ReqSection::Body;
            app.grpc_load(app.active, ctx);
        }
        "grpc" => {
            open_by_name(app, "Health check (gRPC)");
            app.tabs[app.active].section = ReqSection::Body;
            app.grpc_load(app.active, ctx);
            app.send(app.active, ctx);
        }
        "mock" => {
            open_by_name(app, "Create user (JSON)");
            app.tabs[app.active].section = ReqSection::Examples;
            let id = app.ws.collections[0].id.clone();
            app.start_mock(&id);
            if let Some(m) = &app.mock {
                let base = format!("http://{}", m.server.addr);
                std::thread::spawn(move || {
                    let c = reqwest::blocking::Client::new();
                    std::thread::sleep(Duration::from_millis(300));
                    let _ = c.post(format!("{base}/post")).body("{}").send();
                    let _ = c.post(format!("{base}/post")).header("x-mock-response-code", "422").send();
                    let _ = c.get(format!("{base}/uuid")).send();
                    let _ = c.get(format!("{base}/orders/42")).send();
                });
            }
        }
        "scripts" => {
            open_by_name(app, "Scripted checks (JavaScript)");
            app.tabs[app.active].section = ReqSection::Scripts;
            app.tabs[app.active].resp_section = RespSection::Tests;
            app.send(app.active, ctx);
        }
        "mcp" => {
            open_by_name(app, "Weather tool (MCP)");
            app.tabs[app.active].section = ReqSection::Body;
            app.mcp_load(app.active, ctx);
            app.send(app.active, ctx);
        }
        "sse" | "socketio" | "mqtt" => {
            let name = match name {
                "sse" => "Price stream (SSE)",
                "socketio" => "Parcel events (Socket.IO)",
                _ => "Office sensors (MQTT)",
            };
            open_by_name(app, name);
            app.tabs[app.active].section = ReqSection::Body;
            app.ws_connect(app.active, ctx);
        }
        "websocket" => {
            open_by_name(app, "Live prices (WebSocket)");
            app.tabs[app.active].section = ReqSection::Body;
            app.ws_connect(app.active, ctx);
        }
        "graphql" => {
            open_by_name(app, "Users by city (GraphQL)");
            app.tabs[app.active].section = ReqSection::Body;
            app.fetch_graphql_schema(app.active, ctx);
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
        "tree" => {
            open_by_name(app, "JSON document");
            let t = &mut app.tabs[app.active];
            t.body_view = crate::body_view::BodyView::Tree;
            t.body_search = "title".into();
            app.send(app.active, ctx);
        }
        "search" => {
            open_by_name(app, "Create user (JSON)");
            app.tabs[app.active].body_search = "budi".into();
            app.send(app.active, ctx);
        }
        "html" | "image" => {
            let path = if name == "html" { "/html" } else { "/image/png" };
            let mut req = pakpos_core::Request::new(pakpos_core::HttpMethod::Get, format!("{{{{base_url}}}}{path}"));
            req.name = if name == "html" { "Shipment page".into() } else { "Logo".into() };
            let idx = app.push_tab(req, None);
            app.active = idx;
            app.tabs[idx].body_view = crate::body_view::BodyView::Preview;
            app.send(idx, ctx);
        }
        "cookies" => {
            open_by_name(app, "Set a cookie");
            app.send(app.active, ctx);
            app.cookie_jar.with(|j| {
                j.set_cookie(
                    "https://api.example.com/v1/login",
                    "session=8f3a91c2e7; Path=/; Secure; HttpOnly; Max-Age=86400",
                );
                j.set_cookie("https://api.example.com/", "lang=id; Domain=example.com; Path=/");
            });
            app.dialog = Some(Dialog::Cookies(Default::default()));
        }
        "runner-data" => {
            open_by_name(app, "Get with query params");
            let id = app.ws.collections[0].id.clone();
            let name = app.ws.collections[0].name.clone();
            let mut setup = crate::app::RunnerSetup::new(&id, &name);
            let csv = app.ws.root.join("users.csv");
            let _ = std::fs::write(
                &csv,
                "user,city,expected_status\nbudi,Jakarta,200\nsiti,Bandung,200\nagus,Medan,200\nwayan,Denpasar,200\n",
            );
            setup.load_data(csv.display().to_string());
            setup.delay_ms = 50;
            app.runner_setup = Some(setup);
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
        "environments" | "curl" | "snippet" | "settings" | "runner-data" => true,
        "git" => app.git.job.is_none() && app.git.status.as_ref().is_some_and(|s| s.as_ref().is_ok_and(|s| s.is_repo)),
        "grpc" => app.tabs.get(app.active).is_some_and(|t| {
            t.grpc.job.is_none() && matches!(t.response, ResponseState::Done(_) | ResponseState::Error(_))
        }),
        "mock" => app.mock.as_ref().is_some_and(|m| m.hits.len() >= 4),
        "mcp" => app.tabs.get(app.active).is_some_and(|t| {
            t.mcp.job.is_none()
                && !t.mcp.items.is_empty()
                && matches!(t.response, ResponseState::Done(_) | ResponseState::Error(_))
        }),
        "grpc-stream" => app.tabs.get(app.active).is_some_and(|t| {
            t.grpc.live.log.iter().filter(|e| e.direction == Some(pakpos_core::ws::Direction::Received)).count() >= 2
        }),
        "websocket" => app.tabs.get(app.active).is_some_and(|t| t.ws.log.iter().any(|e| e.text.contains("\"tick\""))),
        "sse" | "socketio" | "mqtt" => app.tabs.get(app.active).is_some_and(|t| {
            t.ws.log.iter().filter(|e| e.direction == Some(pakpos_core::ws::Direction::Received)).count() >= 4
        }),
        "graphql" => {
            app.graphql.job.is_none()
                && app
                    .tabs
                    .get(app.active)
                    .is_some_and(|t| matches!(t.response, ResponseState::Done(_) | ResponseState::Error(_)))
        }
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
    if state.name == "grpc-stream" {
        let idx = app.active;
        let loaded = app.tabs.get(idx).is_some_and(|t| t.grpc.pool.is_some() && t.grpc.job.is_none());
        if loaded && !state.sent {
            state.sent = true;
            app.grpc_open_stream(idx, ctx);
            for text in ["paket saya sudah sampai mana?", "terima kasih!"] {
                let msg = format!("{{\"user\": \"budi\", \"text\": \"{text}\"}}");
                app.handle_grpc_action(idx, crate::grpc_view::GrpcAction::StreamSend(msg), ctx);
            }
        }
    }
    if state.name == "socketio" || state.name == "mqtt" {
        let idx = app.active;
        if !state.sent && app.tabs.get(idx).is_some_and(|t| t.ws.connected) {
            state.sent = true;
            let r = app.tabs[idx].request.clone();
            let cfg = r.realtime.clone().unwrap_or_default();
            if let pakpos_core::Body::Raw { content, .. } = r.body {
                let action = if state.name == "mqtt" {
                    crate::ws_view::WsAction::Publish { topic: cfg.topic, payload: content, qos: 1, retain: false }
                } else {
                    crate::ws_view::WsAction::Emit { event: cfg.event, data: content }
                };
                app.handle_ws_action(idx, action, ctx);
            }
        }
    }
    if state.name == "websocket" {
        let idx = app.active;
        let needs_send = !state.sent && app.tabs.get(idx).is_some_and(|t| t.ws.connected);
        if needs_send {
            state.sent = true;
            if let pakpos_core::Body::Raw { content, .. } = app.tabs[idx].request.body.clone() {
                app.handle_ws_action(idx, crate::ws_view::WsAction::Send(content), ctx);
            }
        }
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
