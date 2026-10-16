//! Checks GitHub for a newer PakPos release.
//!
//! Only the public releases list is read; nothing is sent besides the
//! request itself. A stable build is offered stable releases only, a
//! pre-release build (`-rc.N`, `-beta`, …) is offered pre-releases too.

use serde::Deserialize;
use std::cmp::Ordering;
use std::time::Duration;

/// Public list of releases, newest first.
pub const RELEASES_API: &str = "https://api.github.com/repos/sansob/PakPos/releases?per_page=30";
/// Page users open to download a release.
pub const RELEASES_PAGE: &str = "https://github.com/sansob/PakPos/releases";

/// A release newer than the running version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    /// Version without the leading `v`, e.g. `0.3.0-rc.3`.
    pub version: String,
    /// The release page on GitHub.
    pub url: String,
    pub prerelease: bool,
}

/// One entry of the GitHub releases API (only the fields used here).
#[derive(Debug, Clone, Deserialize)]
pub struct GithubRelease {
    pub tag_name: String,
    #[serde(default)]
    pub html_url: String,
    #[serde(default)]
    pub prerelease: bool,
    #[serde(default)]
    pub draft: bool,
}

/// Fetches the releases list and returns the newest release that is newer
/// than `current`, or `None` when `current` is up to date.
pub fn check(current: &str) -> Result<Option<Release>, String> {
    let client = reqwest::blocking::Client::builder()
        .user_agent(format!("PakPos/{current}"))
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())?;
    let resp =
        client.get(RELEASES_API).header("Accept", "application/vnd.github+json").send().map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("GitHub answered {}", resp.status()));
    }
    let releases: Vec<GithubRelease> = resp.json().map_err(|e| e.to_string())?;
    Ok(newest_after(current, &releases))
}

/// The newest release in `releases` that is newer than `current`.
pub fn newest_after(current: &str, releases: &[GithubRelease]) -> Option<Release> {
    let cur = Version::parse(current)?;
    releases
        .iter()
        .filter(|r| !r.draft && (cur.is_prerelease() || !r.prerelease))
        .filter_map(|r| Version::parse(&r.tag_name).map(|v| (v, r)))
        .filter(|(v, _)| *v > cur)
        .max_by(|(a, _), (b, _)| a.cmp(b))
        .map(|(v, r)| Release {
            version: v.text,
            url: if r.html_url.is_empty() { RELEASES_PAGE.to_string() } else { r.html_url.clone() },
            prerelease: r.prerelease,
        })
}

/// A semantic version, ordered the way semver orders them
/// (`1.0.0-rc.1 < 1.0.0-rc.2 < 1.0.0`).
#[derive(Debug, Clone)]
struct Version {
    core: [u64; 3],
    pre: Vec<String>,
    text: String,
}

impl Version {
    fn parse(s: &str) -> Option<Version> {
        let text = s.trim().trim_start_matches(['v', 'V']);
        let text = text.split('+').next().unwrap_or(text);
        let (core, pre) = match text.split_once('-') {
            Some((c, p)) => (c, p.split('.').map(str::to_string).collect()),
            None => (text, Vec::new()),
        };
        let mut parts = core.split('.').map(|n| n.parse::<u64>().ok());
        let v = [parts.next()??, parts.next().unwrap_or(Some(0))?, parts.next().unwrap_or(Some(0))?];
        if parts.next().is_some() {
            return None;
        }
        Some(Version { core: v, pre, text: text.to_string() })
    }

    fn is_prerelease(&self) -> bool {
        !self.pre.is_empty()
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        self.core.cmp(&other.core).then_with(|| match (self.pre.is_empty(), other.pre.is_empty()) {
            (true, true) => Ordering::Equal,
            (true, false) => Ordering::Greater,
            (false, true) => Ordering::Less,
            (false, false) => {
                for (a, b) in self.pre.iter().zip(&other.pre) {
                    let o = match (a.parse::<u64>(), b.parse::<u64>()) {
                        (Ok(x), Ok(y)) => x.cmp(&y),
                        (Ok(_), Err(_)) => Ordering::Less,
                        (Err(_), Ok(_)) => Ordering::Greater,
                        (Err(_), Err(_)) => a.cmp(b),
                    };
                    if o != Ordering::Equal {
                        return o;
                    }
                }
                self.pre.len().cmp(&other.pre.len())
            }
        })
    }
}

impl PartialEq for Version {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for Version {}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rel(tag: &str, prerelease: bool) -> GithubRelease {
        GithubRelease { tag_name: tag.into(), html_url: format!("https://example.com/{tag}"), prerelease, draft: false }
    }

    #[test]
    fn orders_like_semver() {
        let v = |s| Version::parse(s).unwrap();
        assert!(v("0.3.0-rc.2") < v("0.3.0-rc.10"));
        assert!(v("0.3.0-rc.2") < v("0.3.0"));
        assert!(v("0.3.0-alpha") < v("0.3.0-beta"));
        assert!(v("0.3.0-rc") < v("0.3.0-rc.1"));
        assert!(v("v0.10.0") > v("0.9.9"));
        assert_eq!(v("v1.2"), v("1.2.0"));
        assert!(Version::parse("nightly").is_none());
    }

    #[test]
    fn prerelease_builds_see_prereleases() {
        let list = [rel("v0.3.0-rc.3", true), rel("v0.3.0-rc.2", true), rel("v0.2.0", false)];
        let r = newest_after("0.3.0-rc.2", &list).unwrap();
        assert_eq!(r.version, "0.3.0-rc.3");
        assert_eq!(r.url, "https://example.com/v0.3.0-rc.3");
        assert!(newest_after("0.3.0-rc.3", &list).is_none());
    }

    #[test]
    fn stable_builds_skip_prereleases_and_drafts() {
        let mut draft = rel("v0.5.0", false);
        draft.draft = true;
        let list = [draft, rel("v0.4.0-rc.1", true), rel("v0.3.1", false)];
        assert_eq!(newest_after("0.3.0", &list).unwrap().version, "0.3.1");
        assert!(newest_after("0.3.1", &list).is_none());
    }

    #[test]
    fn stable_release_supersedes_its_rc() {
        let list = [rel("v0.3.0", false), rel("v0.3.0-rc.2", true)];
        assert_eq!(newest_after("0.3.0-rc.2", &list).unwrap().version, "0.3.0");
    }
}
