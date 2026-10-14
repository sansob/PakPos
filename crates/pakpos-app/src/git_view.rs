//! Built-in Git sync for the workspace folder.

use crate::app::PakPosApp;
use crate::theme;
use crate::widgets;
use egui::{Id, RichText};
use pakpos_core::git::{self, Commit, GitStatus};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub enum GitOp {
    Refresh,
    Init,
    SetRemote(String),
    Commit(String),
    Pull,
    Push,
    Sync(String),
}

pub struct GitResult {
    pub op: GitOp,
    pub status: Result<GitStatus, String>,
    pub log: Vec<Commit>,
    pub output: Result<String, String>,
    /// Whether files on disk may have changed (reload the workspace).
    pub changed_files: bool,
}

#[derive(Default)]
pub struct GitState {
    pub status: Option<Result<GitStatus, String>>,
    pub log: Vec<Commit>,
    pub output: Vec<(bool, String)>,
    pub message: String,
    pub remote_input: String,
    pub job: Option<(GitOp, Receiver<GitResult>)>,
    pub open: bool,
    pub last_refresh: Option<Instant>,
}

impl GitState {
    pub fn change_count(&self) -> usize {
        match &self.status {
            Some(Ok(s)) if s.is_repo => s.changes.len(),
            _ => 0,
        }
    }
}

fn op_label(op: &GitOp) -> &'static str {
    match op {
        GitOp::Refresh => "Refreshing",
        GitOp::Init => "Initializing",
        GitOp::SetRemote(_) => "Setting remote",
        GitOp::Commit(_) => "Committing",
        GitOp::Pull => "Pulling",
        GitOp::Push => "Pushing",
        GitOp::Sync(_) => "Syncing",
    }
}

fn run_op(root: &std::path::Path, op: &GitOp) -> (Result<String, String>, bool) {
    let e = |r: Result<String, git::GitError>| r.map_err(|e| e.to_string());
    match op {
        GitOp::Refresh => (Ok(String::new()), false),
        GitOp::Init => {
            (e(git::init(root).map(|_| "Initialized a git repository with a PakPos .gitignore".into())), false)
        }
        GitOp::SetRemote(url) => (e(git::set_remote(root, url).map(|_| format!("Remote origin = {url}"))), false),
        GitOp::Commit(msg) => (
            e(git::commit_all(root, msg).map(|h| h.map_or("Nothing to commit".into(), |h| format!("Committed {h}")))),
            false,
        ),
        GitOp::Pull => (e(git::pull(root)), true),
        GitOp::Push => (e(git::push(root)), false),
        GitOp::Sync(msg) => {
            // Commit local work, bring in teammates' changes, then publish.
            let steps = git::commit_all(root, msg)
                .map(|h| h.map_or("Nothing to commit".to_string(), |h| format!("Committed {h}")))
                .and_then(|c| git::pull(root).map(|p| format!("{c}\n{p}")))
                .and_then(|cp| git::push(root).map(|p| format!("{cp}\n{p}")));
            (e(steps), true)
        }
    }
}

impl PakPosApp {
    pub fn git_run(&mut self, op: GitOp, ctx: &egui::Context) {
        if self.git.job.is_some() {
            return;
        }
        let root = self.ws.root.clone();
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        let op2 = op.clone();
        std::thread::spawn(move || {
            let (output, changed_files) = run_op(&root, &op2);
            let status = git::status(&root).map_err(|e| e.to_string());
            let log = git::log(&root, 15).unwrap_or_default();
            let _ = tx.send(GitResult { op: op2, status, log, output, changed_files });
            ctx.request_repaint();
        });
        self.git.job = Some((op, rx));
    }

    pub fn poll_git(&mut self, ctx: &egui::Context) {
        if let Some((_, rx)) = &self.git.job {
            if let Ok(r) = rx.try_recv() {
                self.git.job = None;
                self.git.last_refresh = Some(Instant::now());
                if let Ok(s) = &r.status {
                    if let Some(url) = &s.remote_url {
                        if self.git.remote_input.is_empty() {
                            self.git.remote_input = url.clone();
                        }
                    }
                }
                self.git.status = Some(r.status);
                self.git.log = r.log;
                let quiet = matches!(r.op, GitOp::Refresh);
                match r.output {
                    Ok(out) if !out.is_empty() => {
                        self.git.output.push((false, out.clone()));
                        if !quiet {
                            self.toast(out.lines().last().unwrap_or("Done").to_string());
                        }
                        if matches!(r.op, GitOp::Commit(_) | GitOp::Sync(_)) {
                            self.git.message.clear();
                        }
                    }
                    Ok(_) => {}
                    Err(e) => {
                        self.git.output.push((true, e.clone()));
                        self.toast_error(e);
                    }
                }
                if r.changed_files {
                    self.reload_workspace();
                }
            }
        } else {
            // Keep the header badge fresh: often while the window is open, rarely otherwise.
            let every = if self.git.open { Duration::from_secs(15) } else { Duration::from_secs(60) };
            if self.git.last_refresh.is_none_or(|t| t.elapsed() > every) {
                self.git_run(GitOp::Refresh, ctx);
            }
        }
    }

    /// Re-reads collections and environments from disk (after a pull), keeping
    /// unsaved tab edits.
    pub fn reload_workspace(&mut self) {
        let (fresh, errors) = pakpos_core::storage::Workspace::load(&self.ws.root);
        self.ws.collections = fresh.collections;
        self.ws.environments = fresh.environments;
        self.ws.globals = fresh.globals;
        for t in &mut self.tabs {
            if t.dirty {
                continue;
            }
            if let Some((cid, rid)) = &t.source {
                match self.ws.collections.iter().find(|c| &c.id == cid).and_then(|c| c.find_request(rid)) {
                    Some(r) => {
                        t.request = r.clone();
                        t.url_edit = crate::request_view::compose_url(&t.request);
                    }
                    None => {
                        t.source = None;
                        t.dirty = true;
                    }
                }
            }
        }
        for e in errors {
            self.toast_error(e.to_string());
        }
        for c in &self.ws.collections {
            self.open_nodes.insert(c.id.clone());
        }
    }

    pub fn git_window(&mut self, ctx: &egui::Context) {
        if !self.git.open {
            return;
        }
        let p = self.palette;
        let mut open = true;
        let mut op: Option<GitOp> = None;
        egui::Window::new("Sync with Git")
            .id(Id::new("git_window"))
            .open(&mut open)
            .default_size([640.0, 560.0])
            .pivot(egui::Align2::CENTER_CENTER)
            .default_pos(ctx.content_rect().center())
            .collapsible(false)
            .frame(
                egui::Frame::new().fill(p.surface).stroke(egui::Stroke::new(1.0, p.border)).corner_radius(12).inner_margin(20),
            )
            .show(ctx, |ui| {
                // Show collection / environment names next to their id-based file names.
                let names: std::collections::HashMap<String, String> = self
                    .ws
                    .collections
                    .iter()
                    .map(|c| (format!("collections/{}.json", c.id), c.name.clone()))
                    .chain(self.ws.environments.iter().map(|e| (format!("environments/{}.json", e.id), e.name.clone())))
                    .collect();
                let g = &mut self.git;
                ui.label(RichText::new(format!("Workspace: {}", self.ws.root.display())).small().color(p.text_weak));
                ui.add_space(6.0);
                if let Some((running, _)) = &g.job {
                    ui.horizontal(|ui| {
                        ui.add(egui::Spinner::new().color(theme::ORANGE));
                        ui.label(op_label(running));
                    });
                }
                match &g.status {
                    None => {
                        ui.label(RichText::new("Checking…").color(p.text_weak));
                    }
                    Some(Err(e)) => {
                        ui.label(RichText::new(e).color(theme::RED));
                        ui.label(RichText::new("Install Git from git-scm.com, then reopen this window.").small().color(p.text_weak));
                    }
                    Some(Ok(st)) if !st.is_repo => {
                        ui.label(RichText::new("This workspace is not under version control yet.").strong());
                        ui.label(
                            RichText::new(
                                "Initialize a git repository to share collections with your team through GitHub, GitLab or any git server. Personal files (history, settings, OAuth tokens) are added to .gitignore.",
                            )
                            .small()
                            .color(p.text_weak),
                        );
                        ui.add_space(8.0);
                        if widgets::primary_button(ui, "Initialize repository").clicked() {
                            op = Some(GitOp::Init);
                        }
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new("Already have a repository? Set PAKPOS_HOME to a folder inside your clone.")
                                .small()
                                .color(p.text_weak),
                        );
                    }
                    Some(Ok(st)) => {
                        ui.horizontal(|ui| {
                            widgets::pill(ui, format!("branch {}", st.branch), p.text, p.surface_alt);
                            match &st.upstream {
                                Some(u) => {
                                    widgets::pill(ui, format!("↑{} ↓{}", st.ahead, st.behind), p.text, p.surface_alt);
                                    ui.label(RichText::new(u).small().color(p.text_weak));
                                }
                                None => {
                                    ui.label(RichText::new("not pushed yet").small().color(p.text_weak));
                                }
                            }
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.button("⟳").on_hover_text("Refresh").clicked() {
                                    op = Some(GitOp::Refresh);
                                }
                            });
                        });
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            ui.label("Remote");
                            ui.add(
                                egui::TextEdit::singleline(&mut g.remote_input).min_size(egui::vec2(0.0, crate::theme::CONTROL_HEIGHT)).vertical_align(egui::Align::Center)
                                    .hint_text("git@github.com:team/api-collections.git")
                                    .desired_width(380.0),
                            );
                            let current = st.remote_url.clone().unwrap_or_default();
                            if !g.remote_input.trim().is_empty() && g.remote_input.trim() != current && ui.button("Set").clicked() {
                                op = Some(GitOp::SetRemote(g.remote_input.trim().to_string()));
                            }
                        });
                        ui.add_space(8.0);
                        widgets::section_title(ui, &format!("Changes ({})", st.changes.len()));
                        egui::ScrollArea::vertical().id_salt("git_changes").max_height(140.0).show(ui, |ui| {
                            if st.changes.is_empty() {
                                ui.label(RichText::new("Nothing to commit.").color(p.text_weak));
                            }
                            for c in &st.changes {
                                ui.horizontal(|ui| {
                                    let (label, color) = match c.code {
                                        '?' | 'A' => ("added", theme::GREEN),
                                        'D' => ("deleted", theme::RED),
                                        'R' => ("renamed", theme::BLUE),
                                        'U' => ("conflict", theme::RED),
                                        _ => ("modified", theme::YELLOW),
                                    };
                                    ui.label(RichText::new(label).small().color(color));
                                    ui.label(RichText::new(&c.path).monospace().small());
                                    if let Some(name) = names.get(c.path.as_str()) {
                                        ui.label(RichText::new(name).small().strong());
                                    }
                                });
                            }
                        });
                        ui.add_space(6.0);
                        ui.add(
                            egui::TextEdit::singleline(&mut g.message).min_size(egui::vec2(0.0, crate::theme::CONTROL_HEIGHT)).vertical_align(egui::Align::Center)
                                .hint_text("Commit message (e.g. Add payment endpoints)")
                                .desired_width(f32::INFINITY),
                        );
                        ui.add_space(6.0);
                        let has_remote = st.remote_url.is_some();
                        ui.horizontal(|ui| {
                            if widgets::primary_button(ui, "Sync").on_hover_text("Commit, pull, then push").clicked() {
                                op = Some(GitOp::Sync(g.message.clone()));
                            }
                            if ui.button("Commit").clicked() {
                                op = Some(GitOp::Commit(g.message.clone()));
                            }
                            if ui.add_enabled(has_remote, egui::Button::new("⬇ Pull")).clicked() {
                                op = Some(GitOp::Pull);
                            }
                            if ui.add_enabled(has_remote, egui::Button::new("⬆ Push")).clicked() {
                                op = Some(GitOp::Push);
                            }
                        });
                        if !has_remote {
                            ui.label(RichText::new("Set a remote to pull and push.").small().color(p.text_weak));
                        }
                        ui.add_space(8.0);
                        widgets::section_title(ui, "History");
                        egui::ScrollArea::vertical().id_salt("git_log").max_height(140.0).show(ui, |ui| {
                            if g.log.is_empty() {
                                ui.label(RichText::new("No commits yet.").color(p.text_weak));
                            }
                            for c in &g.log {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(&c.hash).monospace().small().color(theme::ORANGE));
                                    ui.label(&c.summary);
                                    ui.label(RichText::new(format!("{} · {}", c.author, c.when)).small().color(p.text_weak));
                                });
                            }
                        });
                    }
                }
                if let Some((err, last)) = g.output.last() {
                    ui.add_space(6.0);
                    ui.label(RichText::new(last).small().monospace().color(if *err { theme::RED } else { p.text_weak }));
                }
            });
        if let Some(op) = op {
            self.git_run(op, ctx);
        }
        self.git.open = open;
    }
}
