//! Git sync for the workspace folder, using the `git` executable so the
//! user's existing credentials (SSH keys, credential helpers) just work.
//!
//! Every command is scoped to the workspace path, so a workspace that lives
//! inside a bigger repository (e.g. `my-api/.pakpos`) never commits anything
//! outside it.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// Files that are personal or secret and should not be shared.
pub const GITIGNORE: &str = "# PakPos: personal and secret files stay local\nhistory.json\nsettings.json\ntokens.json\nui-state.ron\n*.json.tmp\n";

#[derive(Debug, thiserror::Error)]
pub enum GitError {
    #[error("git is not installed or not on PATH")]
    NotInstalled,
    #[error("{0}")]
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileChange {
    /// Path relative to the workspace.
    pub path: String,
    /// `M` modified, `A` added, `D` deleted, `R` renamed, `?` untracked, `U` conflict.
    pub code: char,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GitStatus {
    pub is_repo: bool,
    pub branch: String,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    pub changes: Vec<FileChange>,
    pub remote_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Commit {
    pub hash: String,
    pub summary: String,
    pub author: String,
    pub when: String,
}

fn git(dir: &Path, args: &[&str]) -> Result<Output, GitError> {
    Command::new("git")
        .args(args)
        .current_dir(dir)
        // Never block waiting for a password prompt in a GUI.
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_ASKPASS", "")
        .env("LC_ALL", "C")
        .output()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                GitError::NotInstalled
            } else {
                GitError::Failed(e.to_string())
            }
        })
}

fn run(dir: &Path, args: &[&str]) -> Result<String, GitError> {
    let out = git(dir, args)?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim_end().to_string())
    } else {
        let mut msg = String::from_utf8_lossy(&out.stderr).trim().to_string();
        if msg.is_empty() {
            msg = String::from_utf8_lossy(&out.stdout).trim().to_string();
        }
        Err(GitError::Failed(format!("git {}: {msg}", args.first().unwrap_or(&""))))
    }
}

pub fn is_installed() -> bool {
    Command::new("git").arg("--version").output().is_ok_and(|o| o.status.success())
}

/// Top level of the repository containing `dir`, if any.
pub fn repo_root(dir: &Path) -> Option<PathBuf> {
    run(dir, &["rev-parse", "--show-toplevel"]).ok().map(PathBuf::from)
}

/// Creates a repository in the workspace with a sensible `.gitignore`.
pub fn init(dir: &Path) -> Result<(), GitError> {
    std::fs::create_dir_all(dir).map_err(|e| GitError::Failed(e.to_string()))?;
    if repo_root(dir).is_none() {
        run(dir, &["init", "-q"])?;
    }
    ensure_gitignore(dir)
}

/// Adds the PakPos entries to `.gitignore` in the workspace (idempotent).
pub fn ensure_gitignore(dir: &Path) -> Result<(), GitError> {
    let path = dir.join(".gitignore");
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    let missing: Vec<&str> = GITIGNORE
        .lines()
        .filter(|l| !l.starts_with('#') && !l.is_empty() && !existing.lines().any(|e| e.trim() == *l))
        .collect();
    if missing.is_empty() {
        return Ok(());
    }
    let mut text = existing;
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    if !text.contains("# PakPos") {
        text.push_str("# PakPos: personal and secret files stay local\n");
    }
    for m in missing {
        text.push_str(m);
        text.push('\n');
    }
    std::fs::write(&path, text).map_err(|e| GitError::Failed(e.to_string()))
}

/// Parses `git status --porcelain=v2 --branch` output.
pub fn parse_status(text: &str) -> GitStatus {
    let mut st = GitStatus { is_repo: true, ..Default::default() };
    for line in text.lines() {
        if let Some(b) = line.strip_prefix("# branch.head ") {
            st.branch = b.to_string();
        } else if let Some(u) = line.strip_prefix("# branch.upstream ") {
            st.upstream = Some(u.to_string());
        } else if let Some(ab) = line.strip_prefix("# branch.ab ") {
            for part in ab.split_whitespace() {
                if let Some(n) = part.strip_prefix('+') {
                    st.ahead = n.parse().unwrap_or(0);
                } else if let Some(n) = part.strip_prefix('-') {
                    st.behind = n.parse().unwrap_or(0);
                }
            }
        } else if let Some(rest) = line.strip_prefix("1 ") {
            // 1 XY sub mH mI mW hH hI path
            let xy: Vec<char> = rest.chars().take(2).collect();
            let path = rest.splitn(8, ' ').nth(7).unwrap_or("").to_string();
            let code = if xy.first().is_some_and(|c| *c != '.') { xy[0] } else { *xy.get(1).unwrap_or(&'M') };
            st.changes.push(FileChange { path, code });
        } else if let Some(rest) = line.strip_prefix("2 ") {
            // 2 XY sub mH mI mW hH hI Xscore path<TAB>origPath
            let path = rest.splitn(9, ' ').nth(8).unwrap_or("").split('\t').next().unwrap_or("").to_string();
            st.changes.push(FileChange { path, code: 'R' });
        } else if let Some(rest) = line.strip_prefix("u ") {
            let path = rest.splitn(10, ' ').nth(9).unwrap_or("").to_string();
            st.changes.push(FileChange { path, code: 'U' });
        } else if let Some(path) = line.strip_prefix("? ") {
            st.changes.push(FileChange { path: path.to_string(), code: '?' });
        }
    }
    st
}

/// Status of the workspace folder only.
pub fn status(dir: &Path) -> Result<GitStatus, GitError> {
    if !dir.exists() || repo_root(dir).is_none() {
        // Distinguish "git missing" from "not a repository".
        git(dir.parent().unwrap_or(dir), &["--version"])?;
        return Ok(GitStatus::default());
    }
    let out = run(dir, &["status", "--porcelain=v2", "--branch", "--untracked-files=all", "--", "."])?;
    let mut st = parse_status(&out);
    // Paths are relative to the repo root; make them relative to the workspace.
    if let (Some(root), Ok(dir_abs)) = (repo_root(dir), dir.canonicalize()) {
        if let Ok(prefix) = dir_abs.strip_prefix(root.canonicalize().unwrap_or(root)) {
            let prefix = prefix.to_string_lossy().replace('\\', "/");
            if !prefix.is_empty() {
                for c in &mut st.changes {
                    if let Some(rest) = c.path.strip_prefix(&format!("{prefix}/")) {
                        c.path = rest.to_string();
                    }
                }
            }
        }
    }
    st.remote_url = run(dir, &["remote", "get-url", "origin"]).ok().filter(|s| !s.is_empty());
    Ok(st)
}

/// Stages and commits every change inside the workspace (and nothing else).
/// Returns the new commit hash, or `None` when there was nothing to commit.
pub fn commit_all(dir: &Path, message: &str) -> Result<Option<String>, GitError> {
    ensure_gitignore(dir)?;
    run(dir, &["add", "-A", "--", "."])?;
    let staged = git(dir, &["diff", "--cached", "--quiet", "--", "."])?;
    if staged.status.success() {
        return Ok(None);
    }
    let msg = if message.trim().is_empty() { "Update PakPos workspace" } else { message.trim() };
    run(dir, &["commit", "-q", "-m", msg, "--", "."])?;
    Ok(Some(run(dir, &["rev-parse", "--short", "HEAD"])?))
}

/// Pulls with rebase (local commits are replayed on top). On conflicts the
/// rebase is aborted and the workspace is left as it was.
pub fn pull(dir: &Path) -> Result<String, GitError> {
    match run(dir, &["pull", "--rebase", "--autostash"]) {
        Ok(out) => Ok(if out.is_empty() { "Up to date".into() } else { out }),
        Err(e) => {
            if repo_root(dir)
                .is_some_and(|r| r.join(".git/rebase-merge").exists() || r.join(".git/rebase-apply").exists())
            {
                let _ = run(dir, &["rebase", "--abort"]);
                return Err(GitError::Failed(
                    "pull stopped on conflicting changes; nothing was changed. Resolve it in a terminal (git pull) or ask a teammate."
                        .into(),
                ));
            }
            Err(e)
        }
    }
}

/// Pushes the current branch, setting the upstream on first push.
pub fn push(dir: &Path) -> Result<String, GitError> {
    let st = status(dir)?;
    let out = if st.upstream.is_some() { run(dir, &["push"])? } else { run(dir, &["push", "-u", "origin", "HEAD"])? };
    Ok(if out.is_empty() { "Pushed".into() } else { out })
}

pub fn set_remote(dir: &Path, url: &str) -> Result<(), GitError> {
    if run(dir, &["remote", "get-url", "origin"]).is_ok() {
        run(dir, &["remote", "set-url", "origin", url.trim()])?;
    } else {
        run(dir, &["remote", "add", "origin", url.trim()])?;
    }
    Ok(())
}

/// Recent commits touching the workspace.
pub fn log(dir: &Path, n: usize) -> Result<Vec<Commit>, GitError> {
    let out = match run(dir, &["log", &format!("-{n}"), "--format=%h%x1f%s%x1f%an%x1f%ar", "--", "."]) {
        Ok(o) => o,
        Err(_) => return Ok(Vec::new()), // no commits yet
    };
    Ok(out
        .lines()
        .filter_map(|l| {
            let mut p = l.split('\u{1f}');
            Some(Commit {
                hash: p.next()?.to_string(),
                summary: p.next()?.to_string(),
                author: p.next()?.to_string(),
                when: p.next()?.to_string(),
            })
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_porcelain_v2() {
        let text = "# branch.oid abc\n# branch.head main\n# branch.upstream origin/main\n# branch.ab +2 -1\n\
1 .M N... 100644 100644 100644 aaa bbb collections/a.json\n\
1 A. N... 000000 100644 100644 000 ccc environments/b.json\n\
2 R. N... 100644 100644 100644 ddd eee R100 collections/new.json\tcollections/old.json\n\
? collections/c.json\n";
        let st = parse_status(text);
        assert_eq!(st.branch, "main");
        assert_eq!(st.upstream.as_deref(), Some("origin/main"));
        assert_eq!((st.ahead, st.behind), (2, 1));
        let codes: Vec<(String, char)> = st.changes.iter().map(|c| (c.path.clone(), c.code)).collect();
        assert_eq!(
            codes,
            vec![
                ("collections/a.json".into(), 'M'),
                ("environments/b.json".into(), 'A'),
                ("collections/new.json".into(), 'R'),
                ("collections/c.json".into(), '?')
            ]
        );
    }

    fn git_ok() -> bool {
        is_installed()
    }

    fn configure(dir: &Path) {
        run(dir, &["config", "user.email", "test@example.com"]).unwrap();
        run(dir, &["config", "user.name", "Test"]).unwrap();
        run(dir, &["config", "commit.gpgsign", "false"]).unwrap();
    }

    #[test]
    fn commit_push_pull_between_two_workspaces() {
        if !git_ok() {
            eprintln!("git not installed; skipping");
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let remote = tmp.path().join("remote.git");
        std::fs::create_dir_all(&remote).unwrap();
        run(&remote, &["init", "-q", "--bare"]).unwrap();

        // Workspace A lives inside a bigger repo: only its folder is committed.
        let project = tmp.path().join("project");
        let a = project.join(".pakpos");
        std::fs::create_dir_all(a.join("collections")).unwrap();
        init(&project).unwrap();
        configure(&project);
        std::fs::write(project.join("main.rs"), "fn main() {}").unwrap();
        std::fs::write(a.join("collections/x.json"), "{}").unwrap();
        std::fs::write(a.join("tokens.json"), "{\"secret\":1}").unwrap();
        let st = status(&a).unwrap();
        assert!(st.is_repo);
        assert!(st.changes.iter().any(|c| c.path == "collections/x.json" && c.code == '?'), "{:?}", st.changes);
        assert!(commit_all(&a, "Add collection").unwrap().is_some());
        // tokens.json is ignored and main.rs (outside the workspace) untouched.
        let tracked = run(&project, &["ls-files"]).unwrap();
        assert!(tracked.contains(".pakpos/collections/x.json"));
        assert!(!tracked.contains("tokens.json"));
        assert!(!tracked.contains("main.rs"));
        assert!(commit_all(&a, "nothing").unwrap().is_none());

        set_remote(&a, remote.to_str().unwrap()).unwrap();
        push(&a).unwrap();
        assert_eq!(log(&a, 5).unwrap()[0].summary, "Add collection");

        // Workspace B clones, changes, pushes; A pulls.
        let b_repo = tmp.path().join("b");
        run(tmp.path(), &["clone", "-q", remote.to_str().unwrap(), b_repo.to_str().unwrap()]).unwrap();
        configure(&b_repo);
        let b = b_repo.join(".pakpos");
        std::fs::write(b.join("collections/x.json"), "{\"name\":\"changed\"}").unwrap();
        commit_all(&b, "Change from B").unwrap();
        push(&b).unwrap();

        let before = status(&a).unwrap();
        assert_eq!(before.ahead, 0);
        pull(&a).unwrap();
        assert_eq!(std::fs::read_to_string(a.join("collections/x.json")).unwrap(), "{\"name\":\"changed\"}");
    }
}
