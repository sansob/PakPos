//! UI translations. English is the source language; `tr` returns the
//! translation for the current language, or the English text when there is
//! none (so a missing entry never breaks the UI).

use std::collections::HashMap;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::OnceLock;

/// Languages of the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    Id,
}

impl Lang {
    pub const ALL: [Lang; 2] = [Lang::En, Lang::Id];

    pub fn code(&self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::Id => "id",
        }
    }

    /// The language's own name, for the picker.
    pub fn name(&self) -> &'static str {
        match self {
            Lang::En => "English",
            Lang::Id => "Bahasa Indonesia",
        }
    }

    pub fn from_code(code: &str) -> Lang {
        match code.trim().to_ascii_lowercase().split(['-', '_', '.']).next() {
            Some("id") | Some("in") => Lang::Id,
            _ => Lang::En,
        }
    }

    /// From `LANG` / `LC_ALL` / `LC_MESSAGES` (Linux, macOS) when no setting is saved.
    pub fn from_system() -> Lang {
        for var in ["LC_ALL", "LC_MESSAGES", "LANG", "LANGUAGE"] {
            if let Ok(v) = std::env::var(var) {
                if !v.trim().is_empty() && v != "C" && v != "POSIX" {
                    return Lang::from_code(&v);
                }
            }
        }
        Lang::En
    }
}

static CURRENT: AtomicU8 = AtomicU8::new(0);

pub fn set(lang: Lang) {
    CURRENT.store(lang as u8, Ordering::Relaxed);
}

pub fn current() -> Lang {
    if CURRENT.load(Ordering::Relaxed) == Lang::Id as u8 {
        Lang::Id
    } else {
        Lang::En
    }
}

fn indonesian() -> &'static HashMap<&'static str, &'static str> {
    static TABLE: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    TABLE.get_or_init(|| crate::i18n_id::ENTRIES.iter().copied().collect())
}

/// The text in the current language.
pub fn tr(english: &str) -> &str {
    match current() {
        Lang::En => english,
        Lang::Id => indonesian().get(english).copied().unwrap_or(english),
    }
}

/// `tr` for templates with `{name}` placeholders: `trf("Saved {file}", &[("file", path)])`.
pub fn trf(english: &str, args: &[(&str, &str)]) -> String {
    let mut out = tr(english).to_string();
    for (k, v) in args {
        out = out.replace(&format!("{{{k}}}"), v);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn languages_and_fallback() {
        assert_eq!(Lang::from_code("id_ID.UTF-8"), Lang::Id);
        assert_eq!(Lang::from_code("en_US"), Lang::En);
        set(Lang::Id);
        assert_eq!(tr("Send"), "Kirim");
        assert_eq!(tr("no such text"), "no such text");
        assert_eq!(trf("Hi {who}", &[("who", "Budi")]), "Hi Budi");
        set(Lang::En);
        assert_eq!(tr("Send"), "Send");
    }

    #[test]
    fn placeholders_survive_translation() {
        // Every {placeholder} in an English key must also be in its translation.
        for (en, id) in crate::i18n_id::ENTRIES {
            let holes = |s: &str| {
                let mut v: Vec<String> = s
                    .match_indices('{')
                    .filter_map(|(i, _)| s[i..].find('}').map(|j| s[i..i + j + 1].to_string()))
                    .collect();
                v.sort();
                v
            };
            assert_eq!(holes(en), holes(id), "{en:?} → {id:?}");
        }
    }

    /// Every literal passed to `tr` / `trf` / `counted` has a translation, is
    /// listed as the same in both languages, or is example input (no spaces
    /// between words, or code-like).
    #[test]
    fn every_ui_text_is_translated() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let keys: std::collections::HashSet<&str> = crate::i18n_id::ENTRIES.iter().map(|(en, _)| *en).collect();
        let mut missing = Vec::new();
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.ends_with("i18n.rs") {
                continue;
            }
            let src = std::fs::read_to_string(&path).unwrap();
            for marker in ["i18n::tr(", "i18n::trf(", "counted("] {
                for (at, _) in src.match_indices(marker) {
                    let Some(rest) = src[at + marker.len()..].trim_start().strip_prefix('"') else { continue };
                    let Some(end) = rest.find('"') else { continue };
                    let text = &rest[..end];
                    let example = text.contains("://")
                        || text.contains('{') && !text.contains("{{variables}}") && !text.contains(' ')
                        || text.starts_with("curl ")
                        || text.starts_with("query ")
                        || text.starts_with("json.")
                        || text.contains('/') && !text.contains(' ')
                        || text.starts_with("git@")
                        || text.ends_with('\\');
                    if !keys.contains(text) && !crate::i18n_id::SAME.contains(&text) && !example {
                        missing.push(format!("{}: {text}", path.display()));
                    }
                }
            }
        }
        assert!(missing.is_empty(), "untranslated UI text:\n{}", missing.join("\n"));
    }
}
