// SPDX-License-Identifier: MIT
//! The panel's "Backup" card (docs/lorawan-agent.md): the gateway's settings and ChirpStack's data in one file to
//! download, and restoring such a file (then the gateway restarts). The work is done by `lorawan-backup` through
//! two systemd units, so it runs with the rights it needs and outside the panel's sandbox.

use crate::http::{Request, Response};
use std::fs;
use std::io::{self, Write};
use std::process::{Command, Stdio};

const TOOL: &str = "/usr/libexec/lorawan-backup";
const BACKUP: &str = "/var/lib/lorawan-update/backup.tar.gz";
const RESTORE: &str = "/var/lib/lorawan-update/restore.tar.gz";
const MAX_RESTORE: u64 = 256 << 20;

pub fn available() -> bool {
    std::path::Path::new(TOOL).exists()
}

fn file_name() -> String {
    let host: String = fs::read_to_string("/proc/sys/kernel/hostname")
        .unwrap_or_default()
        .trim()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect();
    let day = crate::status::utc_date(crate::radio::now());
    format!(
        "lorawan-backup-{}-{day}.tar.gz",
        if host.is_empty() { "gateway" } else { &host }
    )
}

/// Makes a backup (lorawan-backup.service) and sends it.
pub fn download() -> Response {
    if !available() {
        return Response::new(404, "text/plain", "no backup tool on this system\n");
    }
    let _ = fs::remove_file(BACKUP);
    match Command::new("systemctl")
        .args(["start", "lorawan-backup.service"])
        .status()
    {
        Ok(s) if s.success() => {}
        _ => {
            return Response::new(
                500,
                "text/plain",
                "the backup failed: journalctl -u lorawan-backup on the gateway says why\n",
            )
        }
    }
    let Ok(mut f) = fs::File::open(BACKUP) else {
        return Response::new(500, "text/plain", "the backup file is missing\n");
    };
    let _ = fs::remove_file(BACKUP); // stays readable through the open file
    Response::streamed(
        "application/gzip",
        Box::new(move |w: &mut dyn Write| io::copy(&mut f, w).map(|_| ())),
    )
    .with_header(
        "Content-Disposition",
        format!("attachment; filename=\"{}\"", file_name()),
    )
}

/// Receives a backup, checks it, and starts the restore (the gateway restarts afterwards).
pub fn restore(req: &Request) -> Response {
    if !available() {
        return Response::new(404, "text/plain", "no backup tool on this system\n");
    }
    let Some(body) = req.upload.as_ref() else {
        return Response::new(
            400,
            "text/plain",
            "send the backup file as the request body\n",
        );
    };
    if req.upload_len == 0 || req.upload_len > MAX_RESTORE {
        return Response::new(413, "text/plain", "a backup is at most 256 MiB\n");
    }
    let saved = (|| -> io::Result<u64> {
        let mut f = fs::File::create(RESTORE)?;
        let n = io::copy(&mut *body.lock().unwrap(), &mut f)?;
        f.sync_all()?;
        Ok(n)
    })();
    match saved {
        Ok(n) if n == req.upload_len => {}
        Ok(_) | Err(_) => {
            let _ = fs::remove_file(RESTORE);
            return Response::new(400, "text/plain", "upload incomplete\n");
        }
    }
    let check = Command::new(TOOL)
        .args(["check", RESTORE])
        .stdin(Stdio::null())
        .output();
    match check {
        Ok(o) if o.status.success() => {}
        Ok(o) => {
            let _ = fs::remove_file(RESTORE);
            let msg = String::from_utf8_lossy(&o.stderr).trim().to_string();
            return Response::new(
                400,
                "text/plain",
                format!("{}\n", msg.trim_start_matches("lorawan-backup: ")),
            );
        }
        Err(e) => {
            let _ = fs::remove_file(RESTORE);
            return Response::new(500, "text/plain", format!("{TOOL}: {e}\n"));
        }
    }
    match Command::new("systemctl").args(["start", "--no-block", "lorawan-restore.service"]).status() {
        Ok(s) if s.success() => Response::new(
            202,
            "text/plain",
            "Restoring: the gateway restarts in a moment and comes back with the backup's settings.\n",
        ),
        _ => Response::new(500, "text/plain", "could not start lorawan-restore.service\n"),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn name() {
        let n = super::file_name();
        assert!(
            n.starts_with("lorawan-backup-") && n.ends_with(".tar.gz"),
            "{n}"
        );
    }
}
