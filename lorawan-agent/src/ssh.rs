// SPDX-License-Identifier: MIT
//! The panel's "SSH keys" card: public keys allowed to log in as root, kept in
//! `/var/lib/lorawan-ssh/authorized_keys` (sshd reads it next to `/root/.ssh/authorized_keys`, see
//! `50-lorawan-sshd.conf`). A gateway flashed from a release image has no key of its own builder, so this card is
//! how its owner gets a shell. The file is on the data partition: it survives updates, goes into backups, and a
//! factory reset empties it. Only plain keys are accepted, without `command=` or other options.

use crate::bridge;
use crate::http::Response;
use crate::status::json_str;
use std::fs;
use std::io::Write as _;
use std::os::unix::fs::OpenOptionsExt;

pub const DIR: &str = "/var/lib/lorawan-ssh";
const FILE: &str = "/var/lib/lorawan-ssh/authorized_keys";
/// Keys the image itself carries (`LORAWAN_SSH_AUTHORIZED_KEYS` at build time); only counted.
const IMAGE_FILE: &str = "/root/.ssh/authorized_keys";
const MAX_KEYS: usize = 32;
const MAX_LINE: usize = 16 * 1024;

const TYPES: [&str; 6] = [
    "ssh-ed25519",
    "ssh-rsa",
    "ecdsa-sha2-nistp256",
    "ecdsa-sha2-nistp384",
    "ecdsa-sha2-nistp521",
    "sk-ssh-ed25519@openssh.com",
];
const SK_ECDSA: &str = "sk-ecdsa-sha2-nistp256@openssh.com";

#[derive(Debug, PartialEq)]
pub struct Key {
    pub kind: String,
    pub blob: String,
    pub comment: String,
}

impl Key {
    fn line(&self) -> String {
        if self.comment.is_empty() {
            format!("{} {}", self.kind, self.blob)
        } else {
            format!("{} {} {}", self.kind, self.blob, self.comment)
        }
    }
}

/// One public key as `ssh-keygen` or `~/.ssh/id_*.pub` writes it: type, base64 key, optional comment.
pub fn parse_key(line: &str) -> Result<Key, String> {
    let line = line.trim();
    if line.is_empty() {
        return Err("paste a public key (the line from ~/.ssh/id_ed25519.pub)".into());
    }
    if line.len() > MAX_LINE || line.contains(['\n', '\r']) {
        return Err("one key per line, please".into());
    }
    if line.contains("PRIVATE KEY") {
        return Err("that is a private key: never paste it anywhere; use the .pub file".into());
    }
    let mut parts = line.splitn(3, char::is_whitespace);
    let kind = parts.next().unwrap_or_default();
    if !TYPES.contains(&kind) && kind != SK_ECDSA {
        return Err(format!(
            "not a public key type sshd takes ({kind}): it must start with ssh-ed25519, ssh-rsa or ecdsa-sha2-…, \
             without options in front"
        ));
    }
    let blob = parts.next().unwrap_or_default().trim();
    let b64 = |c: char| c.is_ascii_alphanumeric() || c == '+' || c == '/' || c == '=';
    if blob.len() < 32
        || !blob.chars().all(b64)
        || !blob.len().is_multiple_of(4)
        || !well_formed(blob, kind)
    {
        return Err("the key's base64 part is missing or damaged (pasted only in part?)".into());
    }
    let comment = parts.next().unwrap_or_default().trim();
    if comment.chars().any(char::is_control) {
        return Err("the comment has control characters".into());
    }
    Ok(Key {
        kind: kind.into(),
        blob: blob.into(),
        comment: comment.into(),
    })
}

/// Standard base64 with padding, or None.
fn b64_decode(s: &str) -> Option<Vec<u8>> {
    let val = |c: u8| match c {
        b'A'..=b'Z' => Some(c - b'A'),
        b'a'..=b'z' => Some(c - b'a' + 26),
        b'0'..=b'9' => Some(c - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    };
    let s = s.as_bytes();
    if !s.len().is_multiple_of(4) {
        return None;
    }
    let mut out = Vec::with_capacity(s.len() / 4 * 3);
    for (i, chunk) in s.chunks(4).enumerate() {
        let last = i + 1 == s.len() / 4;
        let pad = chunk.iter().rev().take_while(|&&c| c == b'=').count();
        if pad > 2 || (pad > 0 && !last) {
            return None;
        }
        let mut w = 0u32;
        for &c in &chunk[..4 - pad] {
            w = w << 6 | val(c)? as u32;
        }
        w <<= 6 * pad;
        out.extend_from_slice(&w.to_be_bytes()[1..4 - pad]);
    }
    Some(out)
}

/// An SSH public key blob is a run of length-prefixed fields, the first being its type name: the fields must end
/// exactly where the blob ends, which catches a key cut short or mangled in the paste.
fn well_formed(blob: &str, kind: &str) -> bool {
    let Some(d) = b64_decode(blob) else {
        return false;
    };
    let (mut at, mut fields) = (0usize, Vec::new());
    while at < d.len() {
        let Some(len) = d.get(at..at + 4) else {
            return false;
        };
        let len = u32::from_be_bytes([len[0], len[1], len[2], len[3]]) as usize;
        let Some(field) = d.get(at + 4..at + 4 + len) else {
            return false;
        };
        fields.push(field);
        at += 4 + len;
    }
    fields.len() >= 2 && fields[0] == kind.as_bytes()
}

/// The keys in an authorized_keys text, skipping comments, blank lines and anything not parsable.
pub fn parse_file(text: &str) -> Vec<Key> {
    text.lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .filter_map(|l| parse_key(l).ok())
        .collect()
}

fn keys() -> Vec<Key> {
    parse_file(&fs::read_to_string(FILE).unwrap_or_default())
}

fn write_keys(keys: &[Key]) -> std::io::Result<()> {
    fs::create_dir_all(DIR)?;
    let tmp = format!("{FILE}.new");
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&tmp)?;
    f.write_all(b"# Added on the gateway's panel (SSH keys card). One key per line.\n")?;
    for k in keys {
        writeln!(f, "{}", k.line())?;
    }
    f.sync_all()?;
    fs::rename(&tmp, FILE)
}

/// What anyone on the LAN may see: how many keys there are, not which.
pub fn info_json() -> String {
    let image = parse_file(&fs::read_to_string(IMAGE_FILE).unwrap_or_default()).len();
    format!(
        "{{\"available\":true,\"count\":{},\"image_keys\":{image}}}",
        keys().len()
    )
}

/// The list itself (behind the login): type, comment and the end of the key, to tell them apart.
pub fn list_json() -> String {
    let mut o = String::from("{\"keys\":[");
    for (i, k) in keys().iter().enumerate() {
        if i > 0 {
            o.push(',');
        }
        o.push_str("{\"type\":");
        json_str(&mut o, &k.kind);
        o.push_str(",\"comment\":");
        json_str(&mut o, &k.comment);
        o.push_str(",\"tail\":");
        let tail: String = k
            .blob
            .trim_end_matches('=')
            .chars()
            .rev()
            .take(12)
            .collect();
        json_str(&mut o, &tail.chars().rev().collect::<String>());
        o.push('}');
    }
    o.push_str("]}");
    o
}

/// `action=add&key=<line>` or `action=remove&index=<n>` (the position in the list).
pub fn save(body: &[u8]) -> Response {
    let f = bridge::form_fields(body);
    let get = |k: &str| {
        f.iter()
            .find(|(n, _)| n == k)
            .map(|(_, v)| v.as_str())
            .unwrap_or("")
    };
    let mut list = keys();
    let msg = match get("action") {
        "add" => {
            let key = match parse_key(get("key")) {
                Ok(k) => k,
                Err(e) => return Response::new(400, "text/plain", format!("{e}\n")),
            };
            if list.iter().any(|k| k.blob == key.blob) {
                return Response::new(200, "text/plain", "That key is already there.\n");
            }
            if list.len() >= MAX_KEYS {
                return Response::new(400, "text/plain", format!("at most {MAX_KEYS} keys\n"));
            }
            list.push(key);
            "Key added: ssh root@ this gateway now accepts it."
        }
        "remove" => {
            match get("index").parse::<usize>() {
                Ok(i) if i < list.len() => list.remove(i),
                _ => return Response::new(400, "text/plain", "no such key\n"),
            };
            "Key removed."
        }
        _ => return Response::new(400, "text/plain", "unknown action\n"),
    };
    match write_keys(&list) {
        Ok(()) => Response::new(200, "text/plain", format!("{msg}\n")),
        Err(e) => Response::new(500, "text/plain", format!("saving {FILE}: {e}\n")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // A throwaway key generated for these tests (its private half was deleted).
    const ED: &str = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAII6lbqLa1l7vjkyawdcWys6mOuwUqhFjH4O6y34RxSNT me@laptop";

    #[test]
    fn parses_a_pub_file_line() {
        let k = parse_key(&format!("  {ED}\n")).unwrap();
        assert_eq!(k.kind, "ssh-ed25519");
        assert_eq!(k.comment, "me@laptop");
        assert_eq!(k.line(), ED);
    }

    #[test]
    fn comment_may_have_spaces_or_be_missing() {
        let k = parse_key(&format!("{ED} work key")).unwrap();
        assert_eq!(k.comment, "me@laptop work key");
        let bare = ED.rsplit_once(' ').unwrap().0;
        assert_eq!(parse_key(bare).unwrap().comment, "");
    }

    #[test]
    fn refuses_options_private_keys_and_junk() {
        assert!(parse_key(&format!("command=\"/bin/sh\" {ED}")).is_err());
        assert!(parse_key("-----BEGIN OPENSSH PRIVATE KEY-----").is_err());
        assert!(parse_key("ssh-ed25519 not*base64").is_err());
        assert!(parse_key("ssh-ed25519").is_err());
        assert!(parse_key("").is_err());
        assert!(parse_key(&format!("{ED}\nssh-rsa AAAA")).is_err());
    }

    #[test]
    fn refuses_a_cut_key_or_a_wrong_type() {
        // A '+' in the key turned into a space (a form posted without encoding): the first half is still base64.
        let mangled = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIHoRERjdTS/mxR5a1drKX5AYz/t/ rest";
        assert!(parse_key(mangled).is_err());
        assert!(parse_key(&ED[..ED.len() - 20]).is_err());
        let rsa_named = ED.replacen("ssh-ed25519", "ssh-rsa", 1);
        assert!(parse_key(&rsa_named).is_err());
    }

    #[test]
    fn file_skips_comments_and_bad_lines() {
        let text = format!("# header\n\n{ED}\ngarbage line\n");
        assert_eq!(parse_file(&text).len(), 1);
    }
}
