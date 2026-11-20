// SPDX-License-Identifier: MIT
//! The panel's eMMC page: what is on the internal storage, backups of it, and installing this system onto it. The
//! work is done by the board's helper (`bobcat-emmc`, meta-bobcat); without it the page stays hidden.

use crate::http::Response;
use crate::status::json_str;
use std::fmt::Write as _;
use std::io::{self, Write};
use std::path::Path;
use std::process::{Command, Stdio};

const TOOL: &str = "/usr/sbin/bobcat-emmc";
const INSTALL_UNIT: &str = "bobcat-emmc-install.service";

pub fn available() -> bool {
    Path::new(TOOL).exists()
}

/// `bobcat-emmc info` as key=value pairs, in order (`part` repeats).
fn info() -> Vec<(String, String)> {
    let Ok(out) = Command::new(TOOL)
        .arg("info")
        .stderr(Stdio::null())
        .output()
    else {
        return Vec::new();
    };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| l.split_once('='))
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

fn get<'a>(info: &'a [(String, String)], key: &str) -> &'a str {
    info.iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.as_str())
        .unwrap_or("")
}

fn num(s: &str) -> u64 {
    s.parse().unwrap_or(0)
}

pub fn info_json() -> String {
    if !available() {
        return "{\"available\":false}".into();
    }
    let info = info();
    let mut o = String::from("{\"available\":true");
    for (key, name) in [
        ("emmc", "emmc"),
        ("emmc_content", "content"),
        ("emmc_loader", "loader"),
        ("boot", "boot"),
    ] {
        let _ = write!(o, ",\"{name}\":");
        json_str(&mut o, get(&info, key));
    }
    let _ = write!(
        o,
        ",\"emmc_bytes\":{},\"backup_used_bytes\":{},\"parts\":[",
        num(get(&info, "emmc_bytes")),
        num(get(&info, "backup_used_bytes"))
    );
    let parts: Vec<&str> = info
        .iter()
        .filter(|(k, _)| k == "part")
        .map(|(_, v)| v.as_str())
        .collect();
    for (i, p) in parts.iter().enumerate() {
        // n:name:size:fs:used
        let f: Vec<&str> = p.split(':').collect();
        if f.len() != 5 {
            continue;
        }
        if i > 0 {
            o.push(',');
        }
        let _ = write!(o, "{{\"n\":{},\"name\":", num(f[0]));
        json_str(&mut o, f[1]);
        let _ = write!(o, ",\"size\":{},\"fs\":", num(f[2]));
        json_str(&mut o, f[3]);
        let _ = write!(o, ",\"used\":{}}}", num(f[4]));
    }
    o.push_str("],\"install\":{\"state\":");
    json_str(&mut o, get(&info, "install_state"));
    let _ = write!(
        o,
        ",\"step\":{},\"steps\":{},\"message\":",
        num(get(&info, "install_step")),
        num(get(&info, "install_steps"))
    );
    json_str(&mut o, get(&info, "install_message"));
    o.push_str("}}");
    o
}

/// The download name of a backup item; None for anything that is not one.
pub fn backup_filename(item: &str) -> Option<String> {
    match item {
        "head" | "full" | "boot0" | "boot1" => Some(format!("emmc-{item}.img.zst")),
        "tail" => Some("emmc-tail.img".into()),
        "restore" => Some("RESTORE.txt".into()),
        p if p.len() >= 2
            && p.len() <= 3
            && p.starts_with('p')
            && p[1..].bytes().all(|b| b.is_ascii_digit()) =>
        {
            Some(format!("emmc-{p}.img.zst"))
        }
        _ => None,
    }
}

/// Streams `bobcat-emmc backup ITEM` to the client.
pub fn backup(item: &str) -> Response {
    let Some(filename) = backup_filename(item) else {
        return Response::new(404, "text/plain", "no such backup item\n");
    };
    if !available() {
        return Response::new(404, "text/plain", "no eMMC tool on this system\n");
    }
    let content_type = if item == "restore" {
        "text/plain; charset=utf-8"
    } else {
        "application/octet-stream"
    };
    let item = item.to_string();
    Response::streamed(
        content_type,
        Box::new(move |w: &mut dyn Write| {
            let mut child = Command::new(TOOL)
                .args(["backup", &item])
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()?;
            let mut out = child.stdout.take().expect("piped");
            let copied = io::copy(&mut out, w);
            if copied.is_err() {
                // The browser went away: stop reading the eMMC.
                let _ = child.kill();
            }
            let status = child.wait()?;
            copied?;
            if !status.success() {
                // Too late for an HTTP error; a cut-off download is the signal (and its size is wrong).
                return Err(io::Error::other(format!(
                    "bobcat-emmc backup {item}: {status}"
                )));
            }
            Ok(())
        }),
    )
    .with_header(
        "Content-Disposition",
        format!("attachment; filename=\"{filename}\""),
    )
}

/// Starts the install (systemd runs it, so it survives the panel and the request).
pub fn start_install(body: &[u8]) -> Response {
    let form = String::from_utf8_lossy(body);
    if !form.split('&').any(|kv| kv.trim() == "confirm=ERASE") {
        return Response::new(400, "text/plain", "type ERASE to confirm\n");
    }
    if !available() {
        return Response::new(404, "text/plain", "no eMMC tool on this system\n");
    }
    let info = info();
    if get(&info, "boot") != "sd" {
        return Response::new(
            409,
            "text/plain",
            "the gateway must run from the SD card to install onto the eMMC\n",
        );
    }
    if get(&info, "install_state") == "running" {
        return Response::new(409, "text/plain", "an install is already running\n");
    }
    match Command::new("systemctl")
        .args(["start", "--no-block", INSTALL_UNIT])
        .status()
    {
        Ok(s) if s.success() => Response::new(202, "text/plain", "started\n"),
        Ok(s) => Response::new(500, "text/plain", format!("systemctl start: {s}\n")),
        Err(e) => Response::new(500, "text/plain", format!("systemctl: {e}\n")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filenames() {
        assert_eq!(
            backup_filename("head").as_deref(),
            Some("emmc-head.img.zst")
        );
        assert_eq!(backup_filename("p4").as_deref(), Some("emmc-p4.img.zst"));
        assert_eq!(backup_filename("p12").as_deref(), Some("emmc-p12.img.zst"));
        assert_eq!(backup_filename("tail").as_deref(), Some("emmc-tail.img"));
        assert_eq!(backup_filename("restore").as_deref(), Some("RESTORE.txt"));
        for bad in ["p", "p123", "px", "../x", "p1;rm", "", "info"] {
            assert_eq!(backup_filename(bad), None, "{bad}");
        }
    }

    #[test]
    fn install_needs_confirmation() {
        assert_eq!(start_install(b"confirm=erase").status, 400);
        assert_eq!(start_install(b"").status, 400);
    }
}
