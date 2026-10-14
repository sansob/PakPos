//! Application state and the main frame loop.

use crate::theme::{self, Palette};
use crate::widgets;
use egui::{Margin, RichText};
use pakpos_core::assertions::{self, AssertionResult};
use pakpos_core::codegen::SnippetLang;
use pakpos_core::http::{self, PreparedRequest, Response};
use pakpos_core::model::{Auth, Collection, Environment, Item, Request};
use pakpos_core::runner::{RequestReport, RunOptions, RunReport};
use pakpos_core::storage::{Theme, Workspace};
use pakpos_core::vars::VarScope;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReqSection {
    Params,
    Auth,
    Headers,
    Body,
    Tests,
    Extract,
    Scripts,
    Examples,
    Docs,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RespSection {
    Body,
    Headers,
    Tests,
    Console,
    Request,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarView {
    Collections,
    Environments,
    History,
}

/// Result of a finished request, computed on the worker thread.
pub struct Outcome {
    pub prepared: PreparedRequest,
    pub response: Response,
    pub pretty: String,
    pub tests: Vec<AssertionResult>,
    pub extracted: Vec<(String, String)>,
    /// OAuth 2.0 token obtained or refreshed for this send, to store.
    pub new_token: Option<(String, pakpos_core::oauth2::OAuthToken)>,
    /// Script console output.
    pub logs: Vec<String>,
    /// Variables set by scripts, to persist.
    pub script_changes: pakpos_core::scripts::VarChanges,
    /// Parsed JSON body for the tree view (bodies up to 8 MB).
    pub json: Option<serde_json::Value>,
}

/// The body as JSON, parsed once when the response arrives.
pub fn json_of(response: &Response) -> Option<serde_json::Value> {
    (response.body.len() <= 8_000_000).then(|| response.json()).flatten()
}

pub enum ResponseState {
    Empty,
    Loading { started: Instant, rx: Receiver<Result<Outcome, String>> },
    Done(Box<Outcome>),
    Error(String),
}

/// An open editor tab.
pub struct Tab {
    pub uid: u64,
    pub request: Request,
    /// (collection id, request id) when the tab is backed by a saved request.
    pub source: Option<(String, String)>,
    pub dirty: bool,
    pub section: ReqSection,
    pub resp_section: RespSection,
    pub response: ResponseState,
    pub body_view: crate::body_view::BodyView,
    pub body_search: String,
    /// Text of the URL bar (base URL + query), kept in sync with params.
    pub url_edit: String,
    pub ws: crate::ws_view::WsState,
    pub grpc: crate::grpc_view::GrpcState,
}

#[derive(Serialize, Deserialize)]
struct SavedTab {
    request: Request,
    source: Option<(String, String)>,
    dirty: bool,
}

#[derive(Serialize, Deserialize, Default)]
struct SavedSession {
    tabs: Vec<SavedTab>,
    active: usize,
    sidebar_width: Option<f32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenameTarget {
    Collection(String),
    Item { collection: String, item: String },
    Environment(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeleteTarget {
    Collection(String),
    Item { collection: String, item: String },
    Environment(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CollectionTab {
    General,
    Variables,
    Scripts,
}

pub enum Dialog {
    CollectionSettings { id: String, tab: CollectionTab },
    ImportCurl { text: String, error: Option<String> },
    SaveRequest { tab: usize, collection: Option<String>, new_collection: String, name: String },
    Rename { target: RenameTarget, name: String },
    ConfirmDelete { target: DeleteTarget, label: String },
    NewCollection { name: String },
    Environments { selected: Option<String> },
    Settings,
    Cookies(crate::cookies_view::CookiesDialog),
    Snippet { lang: SnippetLang },
    About,
}

pub struct Toast {
    pub text: String,
    pub error: bool,
    pub until: Instant,
}

pub enum RunnerMsg {
    Progress(RequestReport),
    Done(RunReport, Option<Environment>),
}

/// Runner options picked before a run starts.
#[derive(Clone)]
pub struct RunnerSetup {
    pub collection_id: String,
    pub collection: String,
    pub iterations: usize,
    pub delay_ms: u64,
    pub stop_on_failure: bool,
    pub data_path: Option<String>,
    pub data: Vec<pakpos_core::data::Row>,
    pub data_error: Option<String>,
}

impl RunnerSetup {
    pub fn new(collection_id: &str, collection: &str) -> Self {
        Self {
            collection_id: collection_id.to_string(),
            collection: collection.to_string(),
            iterations: 0,
            delay_ms: 0,
            stop_on_failure: false,
            data_path: None,
            data: Vec::new(),
            data_error: None,
        }
    }

    pub fn load_data(&mut self, path: String) {
        match pakpos_core::data::load(&path) {
            Ok(rows) => {
                self.data = rows;
                self.data_error = None;
            }
            Err(e) => {
                self.data.clear();
                self.data_error = Some(e.to_string());
            }
        }
        self.data_path = Some(path);
    }
}

pub struct RunnerState {
    pub setup: RunnerSetup,
    pub collection: String,
    pub environment: Option<String>,
    pub rx: Receiver<RunnerMsg>,
    pub progress: Vec<RequestReport>,
    pub report: Option<RunReport>,
    pub total: usize,
    pub open: bool,
}

/// Things the UI asks for; applied after drawing to keep borrows simple.
pub enum Action {
    OpenRequest { collection: String, request: String },
    MoveItem { from: String, item: String, to: String, at: pakpos_core::model::DropTarget },
    NewTab(Box<Request>),
    NewRequestIn { collection: String, folder: Option<String> },
    NewFolderIn { collection: String, folder: Option<String> },
    Duplicate { collection: String, request: String },
    Rename(RenameTarget, String),
    Delete(DeleteTarget, String),
    ExportCollection(String),
    RunCollection(String),
    ImportPostmanCollection,
    ImportEnvironment,
    ExportEnvironment(String),
    SetActiveEnvironment(Option<String>),
    EditEnvironment(Option<String>),
    NewEnvironment,
    ClearHistory,
    StartMock(String),
    StopMock,
    ShowMock,
    Dialog(Dialog),
}

pub struct PakPosApp {
    pub ws: Workspace,
    pub palette: Palette,
    pub tabs: Vec<Tab>,
    pub active: usize,
    pub sidebar: SidebarView,
    pub search: String,
    pub open_nodes: HashSet<String>,
    pub dialog: Option<Dialog>,
    pub toasts: Vec<Toast>,
    pub env_dirty: Option<Instant>,
    /// Collection edited in the settings dialog, saved after a short pause.
    pub collection_dirty: Option<(String, Instant)>,
    pub runner: Option<RunnerState>,
    pub runner_setup: Option<RunnerSetup>,
    /// Cookie jar shared by every request (mirrored into `ws.cookies` to save it).
    pub cookie_jar: std::sync::Arc<pakpos_core::cookies::SharedJar>,
    pub oauth_job: Option<crate::oauth_view::OAuthJob>,
    pub graphql: crate::graphql_view::GqlState,
    pub mock: Option<crate::mock_view::MockState>,
    pub git: crate::git_view::GitState,
    applied_theme: Option<(Theme, u32)>,
    next_uid: u64,
    pub actions: Vec<Action>,
}

impl PakPosApp {
    pub fn new(cc: &eframe::CreationContext<'_>, ws: Workspace, load_errors: Vec<String>) -> Self {
        let palette = Palette::of(ws.settings.theme);
        let cookie_jar = pakpos_core::cookies::SharedJar::new(ws.cookies.clone());
        let mut app = PakPosApp {
            cookie_jar,
            ws,
            palette,
            tabs: Vec::new(),
            active: 0,
            sidebar: SidebarView::Collections,
            search: String::new(),
            open_nodes: HashSet::new(),
            dialog: None,
            toasts: Vec::new(),
            env_dirty: None,
            collection_dirty: None,
            runner: None,
            runner_setup: None,
            oauth_job: None,
            graphql: Default::default(),
            mock: None,
            git: Default::default(),
            applied_theme: None,
            next_uid: 1,
            actions: Vec::new(),
        };
        for c in &app.ws.collections {
            app.open_nodes.insert(c.id.clone());
        }
        let restore = std::env::var_os("PAKPOS_SCENE").is_none();
        if let Some(session) = cc
            .storage
            .filter(|_| restore)
            .and_then(|s| s.get_string("session"))
            .and_then(|s| serde_json::from_str::<SavedSession>(&s).ok())
        {
            for t in session.tabs {
                let idx = app.push_tab(t.request, t.source);
                app.tabs[idx].dirty = t.dirty;
            }
            app.active = session.active.min(app.tabs.len().saturating_sub(1));
        }
        if app.tabs.is_empty() {
            app.new_blank_tab();
        }
        for e in load_errors {
            app.toast_error(e);
        }
        app
    }

    // ----- tabs -------------------------------------------------------------

    pub fn push_tab(&mut self, request: Request, source: Option<(String, String)>) -> usize {
        let url_edit = crate::request_view::compose_url(&request);
        self.tabs.push(Tab {
            url_edit,
            ws: Default::default(),
            grpc: Default::default(),
            uid: self.next_uid,
            request,
            source,
            dirty: false,
            section: ReqSection::Params,
            resp_section: RespSection::Body,
            response: ResponseState::Empty,
            body_view: Default::default(),
            body_search: String::new(),
        });
        self.next_uid += 1;
        self.tabs.len() - 1
    }

    pub fn new_blank_tab(&mut self) {
        let req = Request { name: "Untitled request".into(), ..Default::default() };
        self.active = self.push_tab(req, None);
    }

    pub fn close_tab(&mut self, idx: usize) {
        if idx < self.tabs.len() {
            self.tabs.remove(idx);
        }
        if self.tabs.is_empty() {
            self.new_blank_tab();
        }
        if self.active >= self.tabs.len() {
            self.active = self.tabs.len() - 1;
        } else if idx < self.active {
            self.active -= 1;
        }
    }

    pub fn open_saved(&mut self, collection: &str, request: &str) {
        if let Some(i) =
            self.tabs.iter().position(|t| t.source.as_ref().is_some_and(|(c, r)| c == collection && r == request))
        {
            self.active = i;
            return;
        }
        let Some(req) =
            self.ws.collections.iter().find(|c| c.id == collection).and_then(|c| c.find_request(request)).cloned()
        else {
            return;
        };
        // Replace a pristine blank tab instead of piling up tabs.
        if let Some(t) = self.tabs.get(self.active) {
            if t.source.is_none() && !t.dirty && t.request.url.is_empty() {
                self.tabs.remove(self.active);
            }
        }
        self.active = self.push_tab(req, Some((collection.to_string(), request.to_string())));
    }

    // ----- notifications ---------------------------------------------------

    pub fn toast(&mut self, text: impl Into<String>) {
        self.toasts.push(Toast { text: text.into(), error: false, until: Instant::now() + Duration::from_secs(3) });
    }

    pub fn toast_error(&mut self, text: impl Into<String>) {
        self.toasts.push(Toast { text: text.into(), error: true, until: Instant::now() + Duration::from_secs(6) });
    }

    // ----- variables & sending ---------------------------------------------

    pub fn collection(&self, id: &str) -> Option<&Collection> {
        self.ws.collections.iter().find(|c| c.id == id)
    }

    pub fn collection_mut(&mut self, id: &str) -> Option<&mut Collection> {
        self.ws.collections.iter_mut().find(|c| c.id == id)
    }

    /// Variable scope for a tab (environment + collection + globals).
    pub fn scope_for(&self, tab: &Tab) -> VarScope {
        let collection_vars =
            tab.source.as_ref().and_then(|(c, _)| self.collection(c)).map(|c| c.variables.clone()).unwrap_or_default();
        let mut scope = VarScope::new()
            .with_environment(self.ws.active_environment())
            .with_collection_vars(&collection_vars)
            .with_globals(&self.ws.globals);
        pakpos_core::oauth2::load_into_scope(&self.ws.tokens, &mut scope);
        scope
    }

    /// Auth a request would inherit from its folder / collection.
    pub fn inherited_auth(&self, tab: &Tab) -> Option<Auth> {
        let (cid, rid) = tab.source.as_ref()?;
        let mut c = self.collection(cid)?.clone();
        if let Some(r) = c.find_request_mut(rid) {
            r.auth = Auth::Inherit;
        }
        c.effective_auth(rid)
    }

    pub fn send(&mut self, idx: usize, ctx: &egui::Context) {
        let Some(tab) = self.tabs.get(idx) else { return };
        if tab.request.kind == pakpos_core::model::RequestKind::WebSocket {
            self.ws_connect(idx, ctx);
            return;
        }
        if tab.request.kind == pakpos_core::model::RequestKind::Grpc {
            self.grpc_send(idx, ctx);
            return;
        }
        let mut req = tab.request.clone();
        widgets::trim_blank_rows(&mut req.params);
        widgets::trim_blank_rows(&mut req.headers);
        let mut scope = self.scope_for(tab);
        let fallback = self.inherited_auth(tab);
        let settings = self.ws.settings.http.clone();
        let mut tokens = self.ws.tokens.clone();
        let jar = self.cookie_jar.clone();
        let collection = tab.source.as_ref().and_then(|(c, _)| self.collection(c));
        let chain = pakpos_core::scripts::chain_for(collection, &req);
        // Unsaved requests are the user's own; saved ones follow collection trust.
        let trusted = tab.source.as_ref().is_none_or(|(c, _)| self.ws.is_trusted(c));
        // Unsaved requests may use pm.sendRequest; saved ones need the collection's permission.
        let network = tab.source.as_ref().is_none_or(|(c, _)| self.ws.has_network(c));
        let script_network = pakpos_core::scripts::Network {
            settings: self.ws.settings.http.clone(),
            cookies: Some(self.cookie_jar.clone()),
            ..Default::default()
        };
        let run_scripts = trusted && !chain.is_empty();
        let skipped = !trusted && !chain.is_empty();
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let limits = if network {
                pakpos_core::scripts::Limits::with_network(script_network)
            } else {
                pakpos_core::scripts::Limits::default()
            };
            let mut logs = Vec::new();
            let mut script_tests = Vec::new();
            let mut changes = pakpos_core::scripts::VarChanges::default();
            if skipped {
                logs.push(
                    "Scripts were not run: this collection is not trusted (see the banner in the Scripts tab).".into(),
                );
            }
            if run_scripts {
                let pre = pakpos_core::scripts::run_pre_request(&chain, &mut req, &mut scope, limits.clone());
                script_tests.extend(pre.error_results());
                logs.extend(pre.logs);
                changes.merge(pre.changes);
            }
            let effective = match &req.auth {
                Auth::Inherit => fallback.clone().unwrap_or(Auth::None),
                a => a.clone(),
            };
            let new_token = match pakpos_core::oauth2::ensure_token(&effective, &mut scope, &mut tokens, &settings) {
                Ok(t) => t,
                Err(pakpos_core::oauth2::OAuthError::NeedsInteractive) => {
                    let _ = tx.send(Err(
                        "No OAuth 2.0 token yet: open the Auth tab and click “Get new access token”.".into()
                    ));
                    ctx.request_repaint();
                    return;
                }
                Err(e) => {
                    let _ = tx.send(Err(e.to_string()));
                    ctx.request_repaint();
                    return;
                }
            };
            let result = http::prepare(&req, &scope, fallback.as_ref())
                .and_then(|mut p| {
                    jar.with(|j| j.merge_into_manual(&mut p));
                    let client = http::client_with_jar(&settings, &p.url, &jar)?;
                    http::send(&client, &p).map(|r| (p, r))
                })
                .map(|(prepared, response)| {
                    let pretty = if response.body.len() > 2_000_000 { response.text() } else { response.pretty_body() };
                    let mut tests = assertions::run_all(&response, &req.assertions, |s| scope.resolve(s));
                    let extracted = assertions::extract_all(&response, &req.extractions);
                    if run_scripts {
                        let t = pakpos_core::scripts::run_tests(&chain, &req, &response, &mut scope, limits);
                        tests.extend(t.tests.iter().cloned());
                        script_tests.extend(t.error_results());
                        logs.extend(t.logs.iter().cloned());
                        changes.merge(t.changes);
                    }
                    tests.extend(script_tests);
                    let json = json_of(&response);
                    Outcome {
                        prepared,
                        response,
                        pretty,
                        tests,
                        extracted,
                        new_token,
                        logs,
                        script_changes: changes,
                        json,
                    }
                })
                .map_err(|e| e.to_string());
            let _ = tx.send(result);
            ctx.request_repaint();
        });
        if let Some(tab) = self.tabs.get_mut(idx) {
            tab.response = ResponseState::Loading { started: Instant::now(), rx };
        }
    }

    fn poll_responses(&mut self) {
        let mut finished = Vec::new();
        for (i, tab) in self.tabs.iter_mut().enumerate() {
            if let ResponseState::Loading { rx, .. } = &tab.response {
                if let Ok(result) = rx.try_recv() {
                    finished.push((i, result));
                }
            }
        }
        for (i, result) in finished {
            let mut req = self.tabs[i].request.clone();
            widgets::trim_blank_rows(&mut req.params);
            widgets::trim_blank_rows(&mut req.headers);
            match result {
                Ok(outcome) => {
                    self.ws.push_history(req, Some(outcome.response.status), Some(outcome.response.elapsed_ms));
                    self.sync_cookies();
                    if let Some((key, token)) = outcome.new_token.clone() {
                        self.ws.tokens.insert(key, token);
                        let _ = self.ws.save_tokens();
                    }
                    let source = self.tabs[i].source.clone();
                    self.apply_script_changes(&outcome.script_changes, source.as_ref().map(|(c, _)| c.as_str()));
                    if !outcome.extracted.is_empty() {
                        let names: Vec<String> =
                            outcome.extracted.iter().map(|(k, _)| format!("{{{{{k}}}}}")).collect();
                        if let Some(env) = self.ws.active_environment_mut() {
                            for (k, v) in &outcome.extracted {
                                env.set(k, v.clone());
                            }
                            self.env_dirty = Some(Instant::now());
                            self.toast(format!("Saved {} to environment", names.join(", ")));
                        } else {
                            for (k, v) in &outcome.extracted {
                                if let Some(g) = self.ws.globals.iter_mut().find(|g| &g.key == k) {
                                    g.value = v.clone();
                                } else {
                                    self.ws.globals.push(pakpos_core::KeyValue::new(k, v.clone()));
                                }
                            }
                            let _ = self.ws.save_globals();
                            self.toast(format!("Saved {} to globals", names.join(", ")));
                        }
                    }
                    let tab = &mut self.tabs[i];
                    if !outcome.tests.is_empty()
                        && tab.resp_section == RespSection::Body
                        && outcome.tests.iter().any(|t| !t.passed)
                    {
                        tab.resp_section = RespSection::Tests;
                    }
                    tab.response = ResponseState::Done(Box::new(outcome));
                }
                Err(e) => {
                    self.ws.push_history(req, None, None);
                    self.tabs[i].response = ResponseState::Error(e);
                }
            }
            let _ = self.ws.save_history();
        }
    }

    pub fn trust_collection(&mut self, id: &str) {
        if !self.ws.is_trusted(id) {
            self.ws.settings.trusted_collections.push(id.to_string());
            self.persist_settings();
            self.toast("Collection trusted: its scripts will run");
        }
    }

    /// Persists variables set by scripts (environment, collection, globals).
    pub fn apply_script_changes(&mut self, changes: &pakpos_core::scripts::VarChanges, collection: Option<&str>) {
        use pakpos_core::scripts::VarChanges;
        if changes.is_empty() {
            return;
        }
        if !changes.environment.is_empty() {
            match self.ws.active_environment_mut() {
                Some(env) => VarChanges::apply_to_list(&mut env.variables, &changes.environment),
                None => VarChanges::apply_to_list(&mut self.ws.globals, &changes.environment),
            }
            self.env_dirty = Some(Instant::now());
        }
        if !changes.globals.is_empty() {
            VarChanges::apply_to_list(&mut self.ws.globals, &changes.globals);
            self.env_dirty = Some(Instant::now());
        }
        if let (false, Some(cid)) = (changes.collection.is_empty(), collection) {
            if let Some(c) = self.collection_mut(cid) {
                VarChanges::apply_to_list(&mut c.variables, &changes.collection);
            }
            self.persist_collection(cid);
        }
    }

    // ----- saving -----------------------------------------------------------

    /// Saves the tab into its collection, or opens the "Save as" dialog.
    pub fn save_tab(&mut self, idx: usize) {
        let Some(tab) = self.tabs.get(idx) else { return };
        let Some((cid, rid)) = tab.source.clone() else {
            let name = tab.request.name.clone();
            let collection = self.ws.collections.first().map(|c| c.id.clone());
            self.dialog = Some(Dialog::SaveRequest { tab: idx, collection, new_collection: String::new(), name });
            return;
        };
        let mut req = tab.request.clone();
        clean_request(&mut req);
        let saved = match self.collection_mut(&cid).and_then(|c| c.find_request_mut(&rid)) {
            Some(slot) => {
                *slot = req;
                true
            }
            None => false,
        };
        if saved {
            self.persist_collection(&cid);
            self.tabs[idx].dirty = false;
            self.toast("Saved");
        } else {
            // The saved request was deleted meanwhile: save as new.
            self.tabs[idx].source = None;
            self.save_tab(idx);
        }
    }

    pub fn persist_collection(&mut self, id: &str) {
        if let Some(c) = self.collection(id) {
            if let Err(e) = self.ws.save_collection(c) {
                self.toast_error(e.to_string());
            }
        }
        self.reload_mock(id);
    }

    /// Stores the tab's current response as an example of its saved request.
    pub fn save_example(&mut self, idx: usize) {
        let Some(tab) = self.tabs.get(idx) else { return };
        let ResponseState::Done(outcome) = &tab.response else { return };
        let Some((cid, rid)) = tab.source.clone() else {
            self.toast_error("Save the request to a collection first, then save examples.");
            return;
        };
        let r = &outcome.response;
        let example =
            pakpos_core::model::Example::from_response(format!("{} {}", r.status, r.status_text).trim().to_string(), r);
        self.tabs[idx].request.examples.push(example.clone());
        if let Some(saved) = self.collection_mut(&cid).and_then(|c| c.find_request_mut(&rid)) {
            saved.examples.push(example);
        }
        self.persist_collection(&cid);
        self.toast("Saved as example (served by the mock server)");
    }

    pub fn persist_environments(&mut self) {
        let mut err = None;
        for e in &self.ws.environments {
            if let Err(x) = self.ws.save_environment(e) {
                err = Some(x.to_string());
            }
        }
        if let Err(x) = self.ws.save_globals() {
            err = Some(x.to_string());
        }
        if let Some(e) = err {
            self.toast_error(e);
        }
        self.env_dirty = None;
    }

    pub fn persist_settings(&mut self) {
        if let Err(e) = self.ws.save_settings() {
            self.toast_error(e.to_string());
        }
    }

    // ----- runner -------------------------------------------------------------

    /// Saves the shared cookie jar when it changed.
    pub fn sync_cookies(&mut self) {
        let snapshot = self.cookie_jar.snapshot();
        if snapshot != self.ws.cookies {
            self.ws.cookies = snapshot;
            let _ = self.ws.save_cookies();
        }
    }

    /// Runs a collection with default options.
    pub fn start_runner(&mut self, collection_id: &str, ctx: &egui::Context) {
        let Some(name) = self.collection(collection_id).map(|c| c.name.clone()) else { return };
        self.start_runner_with(RunnerSetup::new(collection_id, &name), ctx);
    }

    pub fn start_runner_with(&mut self, setup: RunnerSetup, ctx: &egui::Context) {
        let Some(collection) = self.collection(&setup.collection_id).cloned() else { return };
        let env = self.ws.active_environment().cloned();
        let options = RunOptions {
            settings: self.ws.settings.http.clone(),
            globals: self.ws.globals.clone(),
            tokens: self.ws.tokens.clone(),
            cookies: self.cookie_jar.snapshot(),
            allow_scripts: self.ws.is_trusted(&collection.id),
            allow_network: self.ws.has_network(&collection.id),
            data: setup.data.clone(),
            iterations: setup.iterations,
            delay_ms: setup.delay_ms,
            stop_on_failure: setup.stop_on_failure,
            ..Default::default()
        };
        let runnable =
            collection.flatten().iter().filter(|(_, r)| r.kind != pakpos_core::model::RequestKind::WebSocket).count();
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        let total = runnable * options.iteration_count();
        self.runner = Some(RunnerState {
            setup,
            collection: collection.name.clone(),
            environment: env.as_ref().map(|e| e.name.clone()),
            rx,
            progress: Vec::new(),
            report: None,
            total,
            open: true,
        });
        std::thread::spawn(move || {
            let mut env = env;
            let progress_tx = tx.clone();
            let progress_ctx = ctx.clone();
            let report = pakpos_core::runner::run_collection(&collection, env.as_mut(), &options, |r| {
                let _ = progress_tx.send(RunnerMsg::Progress(r.clone()));
                progress_ctx.request_repaint();
            });
            let _ = tx.send(RunnerMsg::Done(report, env));
            ctx.request_repaint();
        });
    }

    fn poll_runner(&mut self) {
        let mut done_env = None;
        let mut done_cookies = false;
        if let Some(r) = &mut self.runner {
            while let Ok(msg) = r.rx.try_recv() {
                match msg {
                    RunnerMsg::Progress(p) => r.progress.push(p),
                    RunnerMsg::Done(report, env) => {
                        if report.tokens != self.ws.tokens {
                            self.ws.tokens = report.tokens.clone();
                            let _ = self.ws.save_tokens();
                        }
                        let cookies = report.cookies.clone();
                        self.cookie_jar.with(|j| *j = cookies);
                        done_cookies = true;
                        r.report = Some(report);
                        done_env = env;
                    }
                }
            }
        }
        if done_cookies {
            self.sync_cookies();
        }
        if let Some(env) = done_env {
            if let Some(slot) = self.ws.environments.iter_mut().find(|e| e.id == env.id) {
                if slot.variables != env.variables {
                    *slot = env;
                    self.env_dirty = Some(Instant::now());
                }
            }
        }
    }

    // ----- frame -------------------------------------------------------------

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        use egui::{Key, KeyboardShortcut, Modifiers};
        let send = KeyboardShortcut::new(Modifiers::COMMAND, Key::Enter);
        let save = KeyboardShortcut::new(Modifiers::COMMAND, Key::S);
        let new_tab = KeyboardShortcut::new(Modifiers::COMMAND, Key::T);
        let close = KeyboardShortcut::new(Modifiers::COMMAND, Key::W);
        let import = KeyboardShortcut::new(Modifiers::COMMAND, Key::I);
        if ctx.input_mut(|i| i.consume_shortcut(&send)) {
            self.send(self.active, ctx);
        }
        if ctx.input_mut(|i| i.consume_shortcut(&save)) {
            self.save_tab(self.active);
        }
        if ctx.input_mut(|i| i.consume_shortcut(&new_tab)) {
            self.new_blank_tab();
        }
        if ctx.input_mut(|i| i.consume_shortcut(&close)) {
            self.close_tab(self.active);
        }
        if ctx.input_mut(|i| i.consume_shortcut(&import)) {
            self.dialog = Some(Dialog::ImportCurl { text: String::new(), error: None });
        }
    }

    fn header(&mut self, ui: &mut egui::Ui) {
        let p = self.palette;
        egui::Panel::top("header")
            .frame(egui::Frame::new().fill(p.header).inner_margin(Margin::symmetric(14, 8)))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    // Brand mark: an envelope-like badge + wordmark.
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(26.0, 26.0), egui::Sense::hover());
                    ui.painter().rect_filled(rect, 6.0, theme::ORANGE);
                    let r = rect.shrink2(egui::vec2(5.0, 8.0));
                    let stroke = egui::Stroke::new(2.0, theme::NAVY_700);
                    ui.painter().rect_stroke(r, 1.5, stroke, egui::StrokeKind::Inside);
                    ui.painter().line_segment([r.left_top(), r.center()], stroke);
                    ui.painter().line_segment([r.right_top(), r.center()], stroke);
                    ui.add_space(2.0);
                    ui.label(RichText::new("Pak").size(18.0).strong().color(theme::ORANGE));
                    ui.add_space(-8.0);
                    ui.label(RichText::new("Pos").size(18.0).strong().color(p.header_text));

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let white = p.header_text;
                        if ui
                            .add(egui::Button::new(RichText::new("⚙").size(16.0).color(white)).frame(false))
                            .on_hover_text("Settings")
                            .clicked()
                        {
                            self.dialog = Some(Dialog::Settings);
                        }
                        let theme_icon = if p.dark { "☀" } else { "🌙" };
                        if ui
                            .add(egui::Button::new(RichText::new(theme_icon).size(15.0).color(white)).frame(false))
                            .on_hover_text("Toggle light / dark")
                            .clicked()
                        {
                            self.ws.settings.theme = if p.dark { Theme::Light } else { Theme::Dark };
                            self.persist_settings();
                        }
                        ui.add_space(8.0);
                        self.environment_picker(ui);
                        ui.label(RichText::new("Environment").color(white.gamma_multiply(0.7)).small());
                        ui.add_space(12.0);
                        let changes = self.git.change_count();
                        let sync_label =
                            if changes > 0 { format!("⇅ Sync ({changes})") } else { "⇅ Sync".to_string() };
                        if ui
                            .add(
                                egui::Button::new(RichText::new(sync_label).color(white))
                                    .fill(theme::NAVY_600)
                                    .stroke(egui::Stroke::NONE),
                            )
                            .on_hover_text("Share this workspace with git")
                            .clicked()
                        {
                            self.git.open = true;
                            self.git.last_refresh = None;
                        }
                        let cookie_count = self.ws.cookies.cookies.len();
                        let cookies_label =
                            if cookie_count > 0 { format!("Cookies ({cookie_count})") } else { "Cookies".to_string() };
                        if ui
                            .add(
                                egui::Button::new(RichText::new(cookies_label).color(white))
                                    .fill(theme::NAVY_600)
                                    .stroke(egui::Stroke::NONE),
                            )
                            .on_hover_text("View and edit the cookie jar")
                            .clicked()
                        {
                            self.dialog = Some(Dialog::Cookies(Default::default()));
                        }
                        let import = ui.add(
                            egui::Button::new(RichText::new("Import").color(white))
                                .fill(theme::NAVY_600)
                                .stroke(egui::Stroke::NONE),
                        );
                        egui::Popup::menu(&import).show(|ui| {
                            if ui.button("Collection file (PakPos / v2.1)…").clicked() {
                                self.actions.push(Action::ImportPostmanCollection);
                            }
                            if ui.button("Environment…").clicked() {
                                self.actions.push(Action::ImportEnvironment);
                            }
                            if ui.button("cURL command…").clicked() {
                                self.actions
                                    .push(Action::Dialog(Dialog::ImportCurl { text: String::new(), error: None }));
                            }
                        });
                        if ui
                            .add(
                                egui::Button::new(RichText::new("+ New").color(theme::INK).strong())
                                    .fill(theme::ORANGE)
                                    .stroke(egui::Stroke::NONE),
                            )
                            .clicked()
                        {
                            self.new_blank_tab();
                        }
                    });
                });
            });
    }

    fn environment_picker(&mut self, ui: &mut egui::Ui) {
        let current = self.ws.active_environment().map(|e| e.name.clone()).unwrap_or_else(|| "No environment".into());
        let mut selected = self.ws.settings.active_environment.clone();
        egui::ComboBox::from_id_salt("env_picker").width(170.0).selected_text(current).show_ui(ui, |ui| {
            ui.selectable_value(&mut selected, None, "No environment");
            for e in &self.ws.environments {
                ui.selectable_value(&mut selected, Some(e.id.clone()), &e.name);
            }
            ui.separator();
            if ui.button("Manage environments…").clicked() {
                self.actions.push(Action::EditEnvironment(self.ws.settings.active_environment.clone()));
            }
        });
        if selected != self.ws.settings.active_environment {
            self.actions.push(Action::SetActiveEnvironment(selected));
        }
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        let p = self.palette;
        egui::Panel::bottom("status")
            .frame(
                egui::Frame::new()
                    .fill(p.surface)
                    .inner_margin(Margin::symmetric(12, 3))
                    .stroke(egui::Stroke::new(1.0, p.border)),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("PakPos {}", pakpos_core::VERSION)).small().color(p.text_weak));
                    ui.separator();
                    ui.label(
                        RichText::new(format!("Workspace: {}", self.ws.root.display())).small().color(p.text_weak),
                    );
                    if let Some(m) = &self.mock {
                        ui.separator();
                        let label =
                            RichText::new(format!("● Mock http://{}", m.server.addr)).small().color(theme::GREEN);
                        if ui
                            .add(egui::Label::new(label).sense(egui::Sense::click()))
                            .on_hover_text("Show mock server")
                            .clicked()
                        {
                            self.actions.push(Action::ShowMock);
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add(
                                egui::Label::new(RichText::new("About").small().color(p.text_weak))
                                    .sense(egui::Sense::click()),
                            )
                            .clicked()
                        {
                            self.dialog = Some(Dialog::About);
                        }
                        ui.label(
                            RichText::new("Ctrl+Enter send  ·  Ctrl+S save  ·  Ctrl+T new tab  ·  Ctrl+I import cURL")
                                .small()
                                .color(p.text_weak),
                        );
                    });
                });
            });
    }

    fn show_toasts(&mut self, ctx: &egui::Context) {
        let now = Instant::now();
        self.toasts.retain(|t| t.until > now);
        if self.toasts.is_empty() {
            return;
        }
        let p = self.palette;
        egui::Area::new(egui::Id::new("toasts"))
            .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-16.0, -36.0))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                for t in &self.toasts {
                    let accent = if t.error { theme::RED } else { theme::GREEN };
                    egui::Frame::new()
                        .fill(p.surface)
                        .stroke(egui::Stroke::new(1.0, accent))
                        .corner_radius(8)
                        .inner_margin(Margin::symmetric(12, 8))
                        .show(ui, |ui| {
                            ui.set_max_width(380.0);
                            ui.label(RichText::new(&t.text).color(p.text));
                        });
                    ui.add_space(6.0);
                }
            });
        ctx.request_repaint_after(Duration::from_millis(250));
    }

    pub fn apply_actions(&mut self) {
        let actions = std::mem::take(&mut self.actions);
        for action in actions {
            crate::actions::apply(self, action);
        }
    }
}

/// Removes UI-only blank rows before persisting a request.
pub fn clean_request(req: &mut Request) {
    widgets::trim_blank_rows(&mut req.params);
    widgets::trim_blank_rows(&mut req.headers);
    match &mut req.body {
        pakpos_core::Body::FormUrlEncoded { fields } => widgets::trim_blank_rows(fields),
        pakpos_core::Body::Multipart { fields } => widgets::trim_blank_fields(fields),
        _ => {}
    }
    req.assertions.retain(|a| !a.target.trim().is_empty());
    req.extractions.retain(|e| !e.variable.trim().is_empty() || !e.source.trim().is_empty());
}

/// Removes UI-only blank rows across a collection tree.
pub fn clean_items(items: &mut [Item]) {
    for item in items {
        match item {
            Item::Request(r) => clean_request(r),
            Item::Folder(f) => clean_items(&mut f.items),
        }
    }
}

impl eframe::App for PakPosApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_git(ctx);
        self.poll_responses();
        self.poll_runner();
        self.poll_oauth();
        self.poll_graphql();
        self.poll_ws();
        self.poll_mock();
        self.poll_grpc();
        if let Some(t) = self.env_dirty {
            if t.elapsed() > Duration::from_millis(600) {
                self.persist_environments();
            }
        }
        if let Some((id, t)) = self.collection_dirty.clone() {
            if t.elapsed() > Duration::from_millis(600) {
                self.persist_collection(&id);
                self.collection_dirty = None;
            }
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let want = (self.ws.settings.theme, self.ws.settings.font_scale_percent);
        if self.applied_theme != Some(want) {
            theme::apply(&ctx, want.0, want.1);
            self.palette = Palette::of(want.0);
            self.applied_theme = Some(want);
        }
        if self.dialog.is_none() {
            self.handle_shortcuts(&ctx);
        }

        self.header(ui);
        self.status_bar(ui);
        self.sidebar(ui);
        self.workbench(ui);

        self.dialogs(&ctx);
        self.runner_setup_window(&ctx);
        self.runner_window(&ctx);
        self.mock_window(&ctx);
        self.git_window(&ctx);
        self.show_toasts(&ctx);
        self.apply_actions();

        if self.oauth_job.is_some()
            || self.graphql.job.is_some()
            || self.tabs.iter().any(|t| matches!(t.response, ResponseState::Loading { .. }))
        {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
        if self.env_dirty.is_some() {
            ctx.request_repaint_after(Duration::from_millis(700));
        }
        crate::scene::tick(self, &ctx);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        let session = SavedSession {
            tabs: self
                .tabs
                .iter()
                .map(|t| SavedTab { request: t.request.clone(), source: t.source.clone(), dirty: t.dirty })
                .collect(),
            active: self.active,
            sidebar_width: None,
        };
        if let Ok(json) = serde_json::to_string(&session) {
            storage.set_string("session", json);
        }
        if self.env_dirty.is_some() {
            self.persist_environments();
        }
    }
}
