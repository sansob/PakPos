//! On-disk workspace: plain JSON files, one per collection / environment, so
//! a workspace folder can be versioned with git and shared with a team.
//!
//! ```text
//! <workspace>/
//!   collections/<id>.json
//!   environments/<id>.json
//!   globals.json
//!   history.json
//!   settings.json
//! ```

use crate::http::HttpSettings;
use crate::model::{Collection, Environment, KeyValue, Request};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Maximum number of history entries kept on disk.
pub const HISTORY_LIMIT: usize = 200;

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("I/O error on `{0}`: {1}")]
    Io(PathBuf, io::Error),
    #[error("invalid JSON in `{0}`: {1}")]
    Json(PathBuf, serde_json::Error),
}

/// One entry of the request history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub id: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub request: Request,
    pub status: Option<u16>,
    pub elapsed_ms: Option<u128>,
}

/// User preferences.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default)]
    pub http: HttpSettings,
    #[serde(default = "default_theme")]
    pub theme: Theme,
    #[serde(default)]
    pub active_environment: Option<String>,
    #[serde(default = "default_font_scale")]
    pub font_scale_percent: u32,
}

fn default_theme() -> Theme {
    Theme::Dark
}

fn default_font_scale() -> u32 {
    100
}

impl Default for Settings {
    fn default() -> Self {
        Self { http: HttpSettings::default(), theme: Theme::Dark, active_environment: None, font_scale_percent: 100 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    #[default]
    Dark,
    Light,
}

/// Everything the user works with, loaded in memory.
#[derive(Debug, Clone, Default)]
pub struct Workspace {
    pub root: PathBuf,
    pub collections: Vec<Collection>,
    pub environments: Vec<Environment>,
    pub globals: Vec<KeyValue>,
    pub history: Vec<HistoryEntry>,
    pub settings: Settings,
    /// OAuth 2.0 tokens (`tokens.json`, keep it out of git).
    pub tokens: crate::oauth2::TokenStore,
}

/// Default workspace location (overridable with `PAKPOS_HOME`).
pub fn default_root() -> PathBuf {
    if let Ok(dir) = std::env::var("PAKPOS_HOME") {
        if !dir.trim().is_empty() {
            return PathBuf::from(dir);
        }
    }
    directories::ProjectDirs::from("dev", "PakPos", "PakPos")
        .map(|d| d.data_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from(".pakpos"))
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<Option<T>, StorageError> {
    match fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text).map(Some).map_err(|e| StorageError::Json(path.to_path_buf(), e)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(StorageError::Io(path.to_path_buf(), e)),
    }
}

/// Writes atomically: write to a temp file, then rename.
fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), StorageError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| StorageError::Io(parent.to_path_buf(), e))?;
    }
    let text = serde_json::to_string_pretty(value).map_err(|e| StorageError::Json(path.to_path_buf(), e))?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, text).map_err(|e| StorageError::Io(tmp.clone(), e))?;
    fs::rename(&tmp, path).map_err(|e| StorageError::Io(path.to_path_buf(), e))
}

fn read_dir_json<T: for<'de> Deserialize<'de>>(dir: &Path, errors: &mut Vec<StorageError>) -> Vec<T> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .collect();
    paths.sort();
    paths
        .into_iter()
        .filter_map(|p| match read_json(&p) {
            Ok(v) => v,
            Err(e) => {
                errors.push(e);
                None
            }
        })
        .collect()
}

/// File names are derived from ids; keep them filesystem-safe.
fn file_name(id: &str) -> String {
    let safe: String =
        id.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
    format!("{safe}.json")
}

impl Workspace {
    /// Loads a workspace. Broken files are skipped and returned as errors so
    /// one corrupt collection never prevents the app from starting.
    pub fn load(root: impl Into<PathBuf>) -> (Workspace, Vec<StorageError>) {
        let root = root.into();
        let mut errors = Vec::new();
        let mut collections: Vec<Collection> = read_dir_json(&root.join("collections"), &mut errors);
        collections.sort_by_key(|c| c.name.to_lowercase());
        let mut environments: Vec<Environment> = read_dir_json(&root.join("environments"), &mut errors);
        environments.sort_by_key(|e| e.name.to_lowercase());
        let globals = take(read_json(&root.join("globals.json")), &mut errors).unwrap_or_default();
        let history = take(read_json(&root.join("history.json")), &mut errors).unwrap_or_default();
        let settings = take(read_json(&root.join("settings.json")), &mut errors).unwrap_or_default();
        let tokens = take(read_json(&root.join("tokens.json")), &mut errors).unwrap_or_default();
        (Workspace { root, collections, environments, globals, history, settings, tokens }, errors)
    }

    pub fn collections_dir(&self) -> PathBuf {
        self.root.join("collections")
    }

    pub fn environments_dir(&self) -> PathBuf {
        self.root.join("environments")
    }

    pub fn save_collection(&self, c: &Collection) -> Result<(), StorageError> {
        write_json(&self.collections_dir().join(file_name(&c.id)), c)
    }

    pub fn delete_collection_file(&self, id: &str) -> Result<(), StorageError> {
        remove(&self.collections_dir().join(file_name(id)))
    }

    pub fn save_environment(&self, e: &Environment) -> Result<(), StorageError> {
        write_json(&self.environments_dir().join(file_name(&e.id)), e)
    }

    pub fn delete_environment_file(&self, id: &str) -> Result<(), StorageError> {
        remove(&self.environments_dir().join(file_name(id)))
    }

    pub fn save_globals(&self) -> Result<(), StorageError> {
        write_json(&self.root.join("globals.json"), &self.globals)
    }

    pub fn save_history(&self) -> Result<(), StorageError> {
        write_json(&self.root.join("history.json"), &self.history)
    }

    pub fn save_tokens(&self) -> Result<(), StorageError> {
        write_json(&self.root.join("tokens.json"), &self.tokens)
    }

    pub fn save_settings(&self) -> Result<(), StorageError> {
        write_json(&self.root.join("settings.json"), &self.settings)
    }

    /// Saves everything.
    pub fn save_all(&self) -> Result<(), StorageError> {
        for c in &self.collections {
            self.save_collection(c)?;
        }
        for e in &self.environments {
            self.save_environment(e)?;
        }
        self.save_globals()?;
        self.save_history()?;
        self.save_settings()
    }

    /// Adds a history entry (newest first) and trims the list.
    pub fn push_history(&mut self, request: Request, status: Option<u16>, elapsed_ms: Option<u128>) {
        self.history.insert(
            0,
            HistoryEntry { id: crate::model::new_id(), timestamp: chrono::Utc::now(), request, status, elapsed_ms },
        );
        self.history.truncate(HISTORY_LIMIT);
    }

    pub fn active_environment(&self) -> Option<&Environment> {
        let id = self.settings.active_environment.as_deref()?;
        self.environments.iter().find(|e| e.id == id)
    }

    pub fn active_environment_mut(&mut self) -> Option<&mut Environment> {
        let id = self.settings.active_environment.clone()?;
        self.environments.iter_mut().find(|e| e.id == id)
    }
}

fn take<T>(r: Result<Option<T>, StorageError>, errors: &mut Vec<StorageError>) -> Option<T> {
    match r {
        Ok(v) => v,
        Err(e) => {
            errors.push(e);
            None
        }
    }
}

fn remove(path: &Path) -> Result<(), StorageError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(StorageError::Io(path.to_path_buf(), e)),
    }
}

/// Loads a collection from a file: PakPos native format or Postman export.
pub fn load_collection_file(path: &Path) -> Result<(Collection, Vec<String>), String> {
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    parse_collection(&text).map_err(|e| format!("{}: {e}", path.display()))
}

/// Parses collection JSON in either PakPos or Postman format.
pub fn parse_collection(text: &str) -> Result<(Collection, Vec<String>), String> {
    let value: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    if value.get("info").is_some() {
        let imported = crate::postman::import_collection(text).map_err(|e| e.to_string())?;
        Ok((imported.value, imported.warnings))
    } else {
        serde_json::from_value(value)
            .map(|c| (c, Vec::new()))
            .map_err(|e| format!("not a PakPos or Postman collection: {e}"))
    }
}

/// Loads an environment from a file: PakPos native format or Postman export.
pub fn load_environment_file(path: &Path) -> Result<Environment, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let value: serde_json::Value = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    if value.get("values").is_some() {
        crate::postman::import_environment(&text).map_err(|e| e.to_string())
    } else {
        serde_json::from_value(value).map_err(|e| format!("{}: {e}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{HttpMethod, Item};

    #[test]
    fn save_and_load() {
        let dir = tempfile::tempdir().unwrap();
        let (mut ws, errors) = Workspace::load(dir.path());
        assert!(errors.is_empty());
        let mut c = Collection::new("Demo");
        c.items.push(Item::Request(Request::new(HttpMethod::Get, "http://x")));
        ws.collections.push(c.clone());
        let mut env = Environment::new("Local");
        env.set("base", "http://localhost");
        ws.settings.active_environment = Some(env.id.clone());
        ws.environments.push(env);
        ws.push_history(Request::new(HttpMethod::Get, "http://y"), Some(200), Some(5));
        ws.save_all().unwrap();

        let (loaded, errors) = Workspace::load(dir.path());
        assert!(errors.is_empty());
        assert_eq!(loaded.collections, vec![c.clone()]);
        assert_eq!(loaded.active_environment().unwrap().get("base"), Some("http://localhost"));
        assert_eq!(loaded.history.len(), 1);

        loaded.delete_collection_file(&c.id).unwrap();
        let (loaded, _) = Workspace::load(dir.path());
        assert!(loaded.collections.is_empty());
    }

    #[test]
    fn corrupt_file_does_not_break_load() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("collections")).unwrap();
        fs::write(dir.path().join("collections/bad.json"), "{ nope").unwrap();
        let (ws, errors) = Workspace::load(dir.path());
        assert!(ws.collections.is_empty());
        assert_eq!(errors.len(), 1);
    }

    #[test]
    fn history_is_trimmed() {
        let mut ws = Workspace::default();
        for _ in 0..(HISTORY_LIMIT + 10) {
            ws.push_history(Request::default(), None, None);
        }
        assert_eq!(ws.history.len(), HISTORY_LIMIT);
    }
}
