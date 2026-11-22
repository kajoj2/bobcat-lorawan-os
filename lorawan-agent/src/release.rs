// SPDX-License-Identifier: MIT
//! Updates from the project's GitHub releases (docs/updates.md, "From the panel"): the update card lists the
//! releases that carry a bundle for this machine, says whether one is newer than the running system, and installs
//! one straight from GitHub: `curl` downloads it onto the data partition, then RAUC installs it as an uploaded
//! bundle would be (signature checked against the gateway's keyring, so a download can only install what the
//! project signed). The repository comes from the image (`/usr/share/lorawan-agent/release-repo`, set by the
//! recipe's LORAWAN_RELEASE_REPO); without one the feature is off.

use crate::http::Response;
use crate::json;
use crate::radio::now;
use crate::status::json_str;
use crate::update::{self, State};
use std::fmt::Write as _;
use std::fs;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

const REPO_FILE: &str = "/usr/share/lorawan-agent/release-repo";
/// RAUC reads the first that exists
const RAUC_CONF: [&str; 2] = ["/etc/rauc/system.conf", "/usr/lib/rauc/system.conf"];
const CACHE_S: u64 = 600;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Release {
    pub tag: String,
    pub name: String,
    pub prerelease: bool,
    pub published: String,
    pub notes_url: String,
    pub asset: String,
    pub asset_url: String,
    pub asset_size: u64,
}

#[derive(Default)]
pub struct Cache {
    at: u64,
    releases: Vec<Release>,
    error: String,
}

pub type Shared = Arc<Mutex<Cache>>;

pub fn repo() -> Option<String> {
    let r = fs::read_to_string(REPO_FILE).ok()?.trim().to_string();
    valid_repo(&r).then_some(r)
}

fn valid_repo(r: &str) -> bool {
    let mut parts = r.split('/');
    let ok = |p: Option<&str>| {
        p.is_some_and(|p| {
            !p.is_empty()
                && p.len() <= 100
                && p.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
        })
    };
    ok(parts.next()) && ok(parts.next()) && parts.next().is_none()
}

/// The machine name RAUC checks bundles against (`compatible=` in its system.conf).
pub fn machine() -> String {
    RAUC_CONF
        .iter()
        .find_map(|p| fs::read_to_string(p).ok())
        .unwrap_or_default()
        .lines()
        .find_map(|l| {
            l.trim()
                .strip_prefix("compatible=")
                .map(|v| v.trim().to_string())
        })
        .unwrap_or_default()
}

/// The running system's version (DISTRO_VERSION): "0.1.0", "1.0.0-rc1", or "dev" for a local build.
pub fn current_version() -> String {
    fs::read_to_string("/etc/os-release")
        .unwrap_or_default()
        .lines()
        .find_map(|l| l.strip_prefix("VERSION_ID="))
        .map(|v| v.trim_matches('"').to_string())
        .unwrap_or_default()
}

/// Version order: numbers first ("1.10.0" > "1.9.2"), a release above its pre-releases ("1.0.0" > "1.0.0-rc2"),
/// pre-releases by their own numbers ("rc10" > "rc9").
pub fn newer(a: &str, b: &str) -> bool {
    fn key(v: &str) -> (Vec<u64>, bool, Vec<u64>, String) {
        let v = v.trim_start_matches('v');
        let (main, pre) = v.split_once('-').unwrap_or((v, ""));
        let nums = |s: &str| -> Vec<u64> {
            s.split(|c: char| !c.is_ascii_digit())
                .filter(|p| !p.is_empty())
                .map(|p| p.parse().unwrap_or(0))
                .collect()
        };
        (nums(main), pre.is_empty(), nums(pre), pre.to_string())
    }
    let (ka, kb) = (key(a), key(b));
    (&ka.0, ka.1, &ka.2) > (&kb.0, kb.1, &kb.2)
}

/// Releases with a bundle for `machine`, newest first, from GitHub's `/releases` JSON.
pub fn parse_releases(text: &[u8], repo: &str, machine: &str) -> Result<Vec<Release>, String> {
    let v = json::parse(text)?;
    let suffix = format!("-{machine}.raucb");
    let prefix = format!("https://github.com/{repo}/releases/download/");
    let mut out = Vec::new();
    for r in v.arr() {
        if r.get("draft").bool() == Some(true) {
            continue;
        }
        let Some(a) = r.get("assets").arr().iter().find(|a| {
            a.get("name").str().is_some_and(|n| n.ends_with(&suffix))
                && a.get("browser_download_url")
                    .str()
                    .is_some_and(|u| u.starts_with(&prefix))
        }) else {
            continue;
        };
        out.push(Release {
            tag: r.get("tag_name").str().unwrap_or_default().to_string(),
            name: r.get("name").str().unwrap_or_default().to_string(),
            prerelease: r.get("prerelease").bool().unwrap_or(false),
            published: r.get("published_at").str().unwrap_or_default().to_string(),
            notes_url: r.get("html_url").str().unwrap_or_default().to_string(),
            asset: a.get("name").str().unwrap_or_default().to_string(),
            asset_url: a
                .get("browser_download_url")
                .str()
                .unwrap_or_default()
                .to_string(),
            asset_size: a.get("size").num().unwrap_or(0.0) as u64,
        });
    }
    out.sort_by(|a, b| {
        if newer(&a.tag, &b.tag) {
            std::cmp::Ordering::Less
        } else if newer(&b.tag, &a.tag) {
            std::cmp::Ordering::Greater
        } else {
            std::cmp::Ordering::Equal
        }
    });
    Ok(out)
}

fn fetch(repo: &str, machine: &str) -> Result<Vec<Release>, String> {
    let url = format!("https://api.github.com/repos/{repo}/releases?per_page=20");
    let out = Command::new("curl")
        .args([
            "-fsSL",
            "-m",
            "20",
            "-H",
            "Accept: application/vnd.github+json",
            "-H",
        ])
        .arg(format!(
            "User-Agent: lorawan-agent/{}",
            env!("CARGO_PKG_VERSION")
        ))
        .arg("--")
        .arg(&url)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("curl: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(if err.is_empty() {
            "GitHub did not answer".into()
        } else {
            err
        });
    }
    parse_releases(&out.stdout, repo, machine)
}

pub fn info_json(cache: &Shared, refresh: bool) -> String {
    let Some(repo) = repo() else {
        return "{\"enabled\":false}".into();
    };
    let machine = machine();
    let stale = {
        let c = cache.lock().unwrap();
        // A refresh asked for on the card: at most once a minute (GitHub limits anonymous API calls)
        let age = now().saturating_sub(c.at);
        (refresh && age > 60) || age > CACHE_S
    };
    if stale {
        let r = fetch(&repo, &machine);
        let mut c = cache.lock().unwrap();
        c.at = now();
        match r {
            Ok(list) => {
                c.releases = list;
                c.error.clear();
            }
            Err(e) => c.error = e,
        }
    }
    let c = cache.lock().unwrap();
    let current = current_version();
    let mut o = String::from("{\"enabled\":true,\"repo\":");
    json_str(&mut o, &repo);
    o.push_str(",\"current\":");
    json_str(&mut o, &current);
    let _ = write!(o, ",\"checked\":{},\"error\":", c.at);
    json_str(&mut o, &c.error);
    o.push_str(",\"releases\":[");
    for (i, r) in c.releases.iter().enumerate() {
        if i > 0 {
            o.push(',');
        }
        o.push_str("{\"tag\":");
        json_str(&mut o, &r.tag);
        o.push_str(",\"name\":");
        json_str(&mut o, &r.name);
        o.push_str(",\"published\":");
        json_str(&mut o, &r.published);
        o.push_str(",\"notes\":");
        json_str(&mut o, &r.notes_url);
        o.push_str(",\"asset\":");
        json_str(&mut o, &r.asset);
        let _ = write!(
            o,
            ",\"size\":{},\"prerelease\":{},\"newer\":{},\"running\":{}}}",
            r.asset_size,
            r.prerelease,
            newer(&r.tag, &current) && current != "dev",
            r.tag.trim_start_matches('v') == current
        );
    }
    o.push_str("]}");
    o
}

/// Downloads the release's bundle and installs it like an uploaded one. `body`: the form with `tag=`.
pub fn download(cache: &Shared, progress: &update::Shared, body: &[u8]) -> Response {
    if !update::available() {
        return Response::new(404, "text/plain", "no A/B updates on this system\n");
    }
    let tag = crate::bridge::form_fields(body)
        .into_iter()
        .find(|(k, _)| k == "tag")
        .map(|(_, v)| v)
        .unwrap_or_default();
    let Some(rel) = cache
        .lock()
        .unwrap()
        .releases
        .iter()
        .find(|r| r.tag == tag)
        .cloned()
    else {
        return Response::new(
            404,
            "text/plain",
            "no such release (check for updates first)\n",
        );
    };
    if matches!(
        progress.lock().unwrap().state,
        State::Uploading | State::Installing
    ) {
        return Response::new(409, "text/plain", "an update is already running\n");
    }
    update::set(
        progress,
        State::Uploading,
        0,
        format!("downloading {}", rel.asset),
    );
    let progress = progress.clone();
    thread::spawn(move || {
        let _ = fs::create_dir_all(update::BUNDLE_DIR);
        let child = Command::new("curl")
            .args([
                "-fsSL",
                "--retry",
                "3",
                "-m",
                "3600",
                "-o",
                update::BUNDLE,
                "--",
            ])
            .arg(&rel.asset_url)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn();
        let mut child = match child {
            Ok(c) => c,
            Err(e) => return update::set(&progress, State::Failed, 0, format!("curl: {e}")),
        };
        let status = loop {
            if let Ok(Some(s)) = child.try_wait() {
                break s;
            }
            let have = fs::metadata(update::BUNDLE).map(|m| m.len()).unwrap_or(0);
            let pct = (have * 100)
                .checked_div(rel.asset_size)
                .unwrap_or(0)
                .min(99) as u32;
            update::set(
                &progress,
                State::Uploading,
                pct,
                format!("downloading {}", rel.asset),
            );
            thread::sleep(Duration::from_secs(1));
        };
        let size = fs::metadata(update::BUNDLE).map(|m| m.len()).unwrap_or(0);
        if !status.success() || (rel.asset_size > 0 && size != rel.asset_size) {
            let mut err = String::new();
            if let Some(mut e) = child.stderr.take() {
                use std::io::Read;
                let _ = e.read_to_string(&mut err);
            }
            let _ = fs::remove_file(update::BUNDLE);
            return update::set(
                &progress,
                State::Failed,
                0,
                format!(
                    "download failed: {}",
                    err.trim().lines().last().unwrap_or("incomplete")
                ),
            );
        }
        update::set(&progress, State::Installing, 0, "checking the bundle");
        update::install(&progress);
    });
    Response::new(202, "text/plain", "downloading\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_order() {
        assert!(newer("v1.0.0", "1.0.0-rc3"));
        assert!(newer("1.0.0-rc10", "1.0.0-rc9"));
        assert!(newer("v1.10.0", "v1.9.9"));
        assert!(newer("0.0.1-rc3", "0.0.1-rc2"));
        assert!(!newer("1.0.0", "1.0.0"));
        assert!(!newer("1.0.0-rc1", "1.0.0"));
    }

    #[test]
    fn releases_for_this_machine() {
        let gh = br#"[
          {"tag_name":"v0.0.1-rc2","name":"LoRaWAN Gateway OS v0.0.1-rc2","prerelease":true,"draft":false,
           "published_at":"2026-11-22T09:00:00Z","html_url":"https://github.com/o/r/releases/tag/v0.0.1-rc2",
           "assets":[{"name":"lorawan-gateway-os-0.0.1-rc2-bobcat-g285.raucb","size":110,
                      "browser_download_url":"https://github.com/o/r/releases/download/v0.0.1-rc2/lorawan-gateway-os-0.0.1-rc2-bobcat-g285.raucb"}]},
          {"tag_name":"v1.0.0","prerelease":false,"draft":false,
           "assets":[{"name":"lorawan-gateway-os-1.0.0-bobcat-g285.raucb","size":120,
                      "browser_download_url":"https://github.com/o/r/releases/download/v1.0.0/lorawan-gateway-os-1.0.0-bobcat-g285.raucb"}]},
          {"tag_name":"v0.9.0","draft":false,"assets":[{"name":"lorawan-gateway-os-0.9.0-other.raucb","size":1,
                      "browser_download_url":"https://github.com/o/r/releases/download/v0.9.0/x"}]},
          {"tag_name":"v2.0.0","draft":false,"assets":[{"name":"evil-bobcat-g285.raucb","size":1,
                      "browser_download_url":"https://example.com/evil-bobcat-g285.raucb"}]},
          {"tag_name":"v3.0.0","draft":true,"assets":[]}
        ]"#;
        let r = parse_releases(gh, "o/r", "bobcat-g285").unwrap();
        assert_eq!(
            r.iter().map(|x| x.tag.as_str()).collect::<Vec<_>>(),
            ["v1.0.0", "v0.0.1-rc2"]
        );
        assert_eq!(r[1].asset_size, 110);
        assert!(r[1].prerelease);
    }

    #[test]
    fn repo_names() {
        assert!(valid_repo("kajoj2/bobcat-lorawan-os"));
        for bad in ["", "a", "a/b/c", "a/../b", "a/b c", "/b"] {
            assert!(!valid_repo(bad), "{bad}");
        }
    }
}
