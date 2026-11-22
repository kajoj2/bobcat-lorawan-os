// SPDX-License-Identifier: MIT
//! The panel's system update page (docs/updates.md): which A/B slot runs and what is in each (`rauc status`),
//! uploading a signed update bundle, installing it into the other slot with RAUC (which checks the signature), and
//! restarting into it. Shown only where RAUC is installed and the system was started as an A/B slot.

use crate::http::{Request, Response};
use crate::radio::now;
use crate::status::json_str;
use std::fmt::Write as _;
use std::fs;
use std::io::{self, BufRead, BufReader, Read};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;

const RAUC: &str = "/usr/bin/rauc";
/// The upload lands on the data partition (the unit's StateDirectory), not in RAM: bundles are 100+ MB.
pub const BUNDLE_DIR: &str = "/var/lib/lorawan-update";
pub const BUNDLE: &str = "/var/lib/lorawan-update/update.raucb";

#[derive(Clone, Debug, Default, PartialEq)]
pub enum State {
    #[default]
    Idle,
    Uploading,
    Installing,
    Done,
    Failed,
}

impl State {
    fn name(&self) -> &'static str {
        match self {
            State::Idle => "idle",
            State::Uploading => "uploading",
            State::Installing => "installing",
            State::Done => "done",
            State::Failed => "failed",
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Progress {
    pub state: State,
    pub percent: u32,
    pub message: String,
    pub at: u64,
}

pub type Shared = Arc<Mutex<Progress>>;

/// The A/B slot this system was started as (`rauc.slot=` from U-Boot), if any.
pub fn booted_slot(cmdline: &str) -> Option<String> {
    cmdline
        .split_whitespace()
        .find_map(|a| a.strip_prefix("rauc.slot="))
        .map(str::to_string)
}

pub fn available() -> bool {
    Path::new(RAUC).exists()
        && booted_slot(&fs::read_to_string("/proc/cmdline").unwrap_or_default()).is_some()
}

/// `rauc status` as RAUC's own JSON (slots, what was installed in each, boot state), or null.
fn rauc_status() -> String {
    let out = Command::new(RAUC)
        .args(["status", "--detailed", "--output-format=json"])
        .stderr(Stdio::null())
        .output();
    match out {
        Ok(o) if o.status.success() => {
            let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if s.starts_with('{') {
                s
            } else {
                "null".into()
            }
        }
        _ => "null".into(),
    }
}

/// `default_password`: whether ChirpStack still accepts admin/admin (None: it did not answer).
pub fn info_json(progress: &Shared, default_password: Option<bool>) -> String {
    if !available() {
        return "{\"available\":false}".into();
    }
    let p = progress.lock().unwrap().clone();
    let mut o = String::from("{\"available\":true,\"booted\":");
    json_str(
        &mut o,
        &booted_slot(&fs::read_to_string("/proc/cmdline").unwrap_or_default()).unwrap_or_default(),
    );
    let _ = write!(o, ",\"rauc\":{},\"install\":{{\"state\":", rauc_status());
    json_str(&mut o, p.state.name());
    let _ = write!(o, ",\"percent\":{},\"message\":", p.percent);
    json_str(&mut o, &p.message);
    let dp = match default_password {
        Some(true) => "true",
        Some(false) => "false",
        None => "null",
    };
    let _ = write!(o, ",\"at\":{}}},\"default_password\":{dp}}}", p.at);
    o
}

pub fn set(progress: &Shared, state: State, percent: u32, message: impl Into<String>) {
    let mut p = progress.lock().unwrap();
    *p = Progress {
        state,
        percent,
        message: message.into(),
        at: now(),
    };
}

/// One line of `rauc install`'s output: "20% Checking bundle" → (20, "Checking bundle").
pub fn parse_progress(line: &str) -> Option<(u32, String)> {
    let (pct, rest) = line.trim().split_once('%')?;
    let pct: u32 = pct.trim().parse().ok()?;
    Some((pct.min(100), rest.trim().to_string()))
}

/// Receives the bundle (the request's streamed body) and starts the install in the background.
pub fn upload(req: &Request, progress: &Shared) -> Response {
    if !available() {
        return Response::new(404, "text/plain", "no A/B updates on this system\n");
    }
    {
        let p = progress.lock().unwrap();
        if matches!(p.state, State::Uploading | State::Installing) {
            return Response::new(409, "text/plain", "an update is already running\n");
        }
    }
    let Some(body) = req.upload.as_ref() else {
        return Response::new(400, "text/plain", "send the bundle as the request body\n");
    };
    set(progress, State::Uploading, 0, "receiving the bundle");
    let received = (|| -> io::Result<u64> {
        if let Some(dir) = Path::new(BUNDLE).parent() {
            fs::create_dir_all(dir)?;
        }
        let mut f = fs::File::create(BUNDLE)?;
        let n = io::copy(&mut *body.lock().unwrap(), &mut f)?;
        f.sync_all()?;
        Ok(n)
    })();
    match received {
        Ok(n) if n == req.upload_len && n > 0 => {}
        Ok(n) => {
            let _ = fs::remove_file(BUNDLE);
            set(
                progress,
                State::Failed,
                0,
                format!("upload cut off after {n} of {} bytes", req.upload_len),
            );
            return Response::new(400, "text/plain", "upload incomplete\n");
        }
        Err(e) => {
            let _ = fs::remove_file(BUNDLE);
            set(
                progress,
                State::Failed,
                0,
                format!("saving the upload: {e}"),
            );
            return Response::new(500, "text/plain", format!("saving the upload: {e}\n"));
        }
    }
    set(progress, State::Installing, 0, "checking the bundle");
    let progress = progress.clone();
    thread::spawn(move || install(&progress));
    Response::new(202, "text/plain", "received, installing\n")
}

/// `rauc install` on the uploaded file: RAUC checks the signature and the compatible string, writes the other slot
/// and makes it the next to boot. Its progress lines feed the panel.
pub fn install(progress: &Shared) {
    let child = Command::new(RAUC)
        .args(["install", BUNDLE])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();
    let mut child = match child {
        Ok(c) => c,
        Err(e) => {
            set(progress, State::Failed, 0, format!("rauc: {e}"));
            let _ = fs::remove_file(BUNDLE);
            return;
        }
    };
    let stderr = child.stderr.take().map(|mut e| {
        thread::spawn(move || {
            let mut s = String::new();
            let _ = e.read_to_string(&mut s);
            s
        })
    });
    let mut last = String::new();
    if let Some(out) = child.stdout.take() {
        for line in BufReader::new(out).lines().map_while(Result::ok) {
            if let Some((pct, msg)) = parse_progress(&line) {
                set(progress, State::Installing, pct, msg);
            } else if !line.trim().is_empty() {
                last = line.trim().to_string();
            }
        }
    }
    let status = child.wait();
    let err = stderr
        .and_then(|t| t.join().ok())
        .unwrap_or_default()
        .lines()
        .rfind(|l| !l.trim().is_empty())
        .unwrap_or_default()
        .to_string();
    let _ = fs::remove_file(BUNDLE);
    match status {
        Ok(s) if s.success() => set(
            progress,
            State::Done,
            100,
            "Installed into the other slot. Restart to start it; if it does not come up three times, the gateway \
             goes back to this one.",
        ),
        Ok(_) => set(
            progress,
            State::Failed,
            0,
            if !err.is_empty() { err } else if !last.is_empty() { last } else { "rauc install failed".into() },
        ),
        Err(e) => set(progress, State::Failed, 0, format!("rauc: {e}")),
    }
}

/// Restarts into the newly installed slot (or the same one, if nothing was installed).
pub fn reboot(progress: &Shared) -> Response {
    if matches!(
        progress.lock().unwrap().state,
        State::Uploading | State::Installing
    ) {
        return Response::new(409, "text/plain", "an update is running, wait for it\n");
    }
    match Command::new("systemctl").args(["reboot"]).status() {
        Ok(s) if s.success() => Response::new(202, "text/plain", "restarting\n"),
        Ok(s) => Response::new(500, "text/plain", format!("systemctl reboot: {s}\n")),
        Err(e) => Response::new(500, "text/plain", format!("systemctl: {e}\n")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slot_from_cmdline() {
        assert_eq!(
            booted_slot("root=PARTUUID=x rootwait rauc.slot=B console=ttyS2").as_deref(),
            Some("B")
        );
        assert_eq!(booted_slot("root=PARTUUID=x rootwait"), None);
    }

    #[test]
    fn progress_lines() {
        assert_eq!(
            parse_progress("  0% Installing"),
            Some((0, "Installing".into()))
        );
        assert_eq!(
            parse_progress(" 40% Checking slot rootfs.1"),
            Some((40, "Checking slot rootfs.1".into()))
        );
        assert_eq!(
            parse_progress("100% Installing done."),
            Some((100, "Installing done.".into()))
        );
        assert_eq!(parse_progress("installing `/x`: succeeded"), None);
        assert_eq!(parse_progress("LastError: Failed"), None);
    }

    #[test]
    fn states() {
        assert_eq!(State::default().name(), "idle");
        assert_eq!(State::Installing.name(), "installing");
    }
}
