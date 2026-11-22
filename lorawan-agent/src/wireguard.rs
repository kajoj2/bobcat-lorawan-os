// SPDX-License-Identifier: MIT
//! The panel's "Remote access (WireGuard)" card (docs/remote-access.md): a WireGuard tunnel to a server or VPN you
//! run, so the gateway's panel, ChirpStack and SSH are reachable behind NAT. The card generates the gateway's key
//! (only the public key leaves it), takes the rest of a wg-quick configuration, and `lorawan-wireguard.service`
//! brings the tunnel up at boot. Only plain WireGuard settings are accepted: wg-quick would run PreUp/PostUp lines
//! as shell commands, and DNS needs a resolvconf the image does not have.

use crate::bridge;
use crate::http::Response;
use crate::status::json_str;
use std::fmt::Write as _;
use std::fs;
use std::io::Write as _;
use std::os::unix::fs::OpenOptionsExt;
use std::process::{Command, Stdio};

pub const DIR: &str = "/var/lib/lorawan-wireguard";
const CONF: &str = "/var/lib/lorawan-wireguard/wg0.conf";
const KEY: &str = "/var/lib/lorawan-wireguard/private.key";
const FIREWALL: &str = "/var/lib/lorawan-firewall/wireguard.nft";
const UNIT: &str = "lorawan-wireguard.service";
const IFACE: &str = "wg0";

const INTERFACE_KEYS: [&str; 4] = ["PrivateKey", "Address", "ListenPort", "MTU"];
const PEER_KEYS: [&str; 5] = [
    "PublicKey",
    "PresharedKey",
    "AllowedIPs",
    "Endpoint",
    "PersistentKeepalive",
];

pub fn available() -> bool {
    std::path::Path::new("/usr/bin/wg-quick").exists()
}

fn key_ok(k: &str) -> bool {
    k.len() == 44
        && k.ends_with('=')
        && k.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"+/=".contains(&b))
}

fn value_ok(v: &str) -> bool {
    !v.is_empty()
        && v.len() <= 512
        && v.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b" .,:/[]-_+=".contains(&b))
}

/// The configuration to store: the card's text checked line by line, with the gateway's own private key put in
/// (the card never sees it). Returns the text and the ListenPort, if any.
pub fn validate(text: &str, private_key: &str) -> Result<(String, Option<u16>), String> {
    let mut out = String::from(
        "# Written by lorawan-agent (the panel's Remote access card); change it there.\n",
    );
    let mut section = "";
    let (mut interfaces, mut peers, mut port) = (0, 0, None);
    let mut peer_has_key = true;
    for (n, raw) in text.lines().enumerate() {
        let line = raw.split('#').next().unwrap_or_default().trim();
        if line.is_empty() {
            continue;
        }
        let at = |m: &str| format!("line {}: {m}", n + 1);
        match line {
            "[Interface]" => {
                interfaces += 1;
                section = "Interface";
                let _ = writeln!(out, "\n[Interface]\nPrivateKey = {private_key}");
                continue;
            }
            "[Peer]" => {
                if !peer_has_key {
                    return Err(format!("peer {peers} has no PublicKey"));
                }
                peers += 1;
                peer_has_key = false;
                section = "Peer";
                out.push_str("\n[Peer]\n");
                continue;
            }
            _ => {}
        }
        let Some((k, v)) = line.split_once('=') else {
            return Err(at("expected Key = value"));
        };
        let (k, v) = (k.trim(), v.trim());
        match section {
            "Interface" if k == "PrivateKey" => continue, // the gateway's own key is used
            "Interface" if INTERFACE_KEYS.contains(&k) => {}
            "Peer" if PEER_KEYS.contains(&k) => {}
            "" => return Err(at("settings must come after [Interface] or [Peer]")),
            _ if k == "DNS" => {
                return Err(at(
                    "DNS is not supported here (no resolvconf on the gateway); leave it out",
                ))
            }
            _ => return Err(at(&format!("{k} is not accepted in [{section}]"))),
        }
        if !value_ok(v) {
            return Err(at(&format!("{k}: unexpected characters")));
        }
        match k {
            "PublicKey" | "PresharedKey" if !key_ok(v) => {
                return Err(at(&format!("{k}: not a WireGuard key")))
            }
            "ListenPort" => {
                port = Some(
                    v.parse::<u16>()
                        .map_err(|_| at("ListenPort: a port number"))?,
                )
            }
            "MTU" | "PersistentKeepalive" if v.parse::<u16>().is_err() => {
                return Err(at(&format!("{k}: a number")))
            }
            _ => {}
        }
        if k == "PublicKey" {
            peer_has_key = true;
        }
        let _ = writeln!(out, "{k} = {v}");
    }
    if interfaces != 1 {
        return Err("exactly one [Interface] section is needed".into());
    }
    if peers == 0 {
        return Err("at least one [Peer] is needed".into());
    }
    if !peer_has_key {
        return Err(format!("peer {peers} has no PublicKey"));
    }
    if !out.contains("\nAddress = ") {
        return Err("[Interface] needs an Address (the gateway's address in the tunnel)".into());
    }
    Ok((out, port))
}

/// The card's view of a stored configuration: everything but the private key.
pub fn shareable(conf: &str) -> String {
    conf.lines()
        .filter(|l| !l.trim_start().starts_with("PrivateKey") && !l.starts_with("# Written by"))
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

fn wg(args: &[&str], input: Option<&str>) -> Result<String, String> {
    let mut child = Command::new("wg")
        .args(args)
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("wg: {e}"))?;
    if let (Some(i), Some(mut s)) = (input, child.stdin.take()) {
        let _ = s.write_all(i.as_bytes());
    }
    let o = child.wait_with_output().map_err(|e| format!("wg: {e}"))?;
    if !o.status.success() {
        return Err(String::from_utf8_lossy(&o.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&o.stdout).trim().to_string())
}

fn write_private(path: &str, text: &str) -> std::io::Result<()> {
    fs::create_dir_all(DIR)?;
    let tmp = format!("{path}.tmp");
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&tmp)?;
    f.write_all(text.as_bytes())?;
    f.sync_all()?;
    fs::rename(&tmp, path)
}

/// The gateway's private key, made on first use.
fn private_key() -> Result<String, String> {
    if let Ok(k) = fs::read_to_string(KEY) {
        if key_ok(k.trim()) {
            return Ok(k.trim().to_string());
        }
    }
    let k = wg(&["genkey"], None)?;
    write_private(KEY, &format!("{k}\n")).map_err(|e| format!("saving the key: {e}"))?;
    Ok(k)
}

fn active() -> bool {
    Command::new("systemctl")
        .args(["is-active", "--quiet", UNIT])
        .status()
        .is_ok_and(|s| s.success())
}

pub fn info_json() -> String {
    if !available() {
        return "{\"available\":false}".into();
    }
    let conf = fs::read_to_string(CONF).ok();
    let public = fs::read_to_string(KEY)
        .ok()
        .and_then(|k| wg(&["pubkey"], Some(k.trim())).ok())
        .unwrap_or_default();
    let mut o = String::from("{\"available\":true,\"configured\":");
    o.push_str(if conf.is_some() { "true" } else { "false" });
    let _ = write!(
        o,
        ",\"active\":{},\"public_key\":",
        conf.is_some() && active()
    );
    json_str(&mut o, &public);
    o.push_str(",\"config\":");
    json_str(&mut o, &conf.as_deref().map(shareable).unwrap_or_default());
    // Peers: public key, endpoint, last handshake (Unix time, 0 = never), bytes in/out
    o.push_str(",\"peers\":[");
    if let Ok(dump) = wg(&["show", IFACE, "dump"], None) {
        for (i, l) in dump.lines().skip(1).enumerate() {
            let f: Vec<&str> = l.split('\t').collect();
            if f.len() < 7 {
                continue;
            }
            if i > 0 {
                o.push(',');
            }
            o.push_str("{\"public_key\":");
            json_str(&mut o, f[0]);
            o.push_str(",\"endpoint\":");
            json_str(&mut o, if f[2] == "(none)" { "" } else { f[2] });
            let _ = write!(
                o,
                ",\"handshake\":{},\"rx\":{},\"tx\":{}}}",
                f[4].parse::<u64>().unwrap_or(0),
                f[5].parse::<u64>().unwrap_or(0),
                f[6].parse::<u64>().unwrap_or(0)
            );
        }
    }
    o.push_str("]}");
    o
}

fn systemctl(args: &[&str]) -> bool {
    Command::new("systemctl")
        .args(args)
        .status()
        .is_ok_and(|s| s.success())
}

fn set_firewall(port: Option<u16>) {
    match port {
        Some(p) => {
            let _ = fs::create_dir_all("/var/lib/lorawan-firewall");
            let _ = fs::write(
                FIREWALL,
                format!("# lorawan-agent: WireGuard's ListenPort\nudp dport {p} accept\n"),
            );
        }
        None => {
            let _ = fs::remove_file(FIREWALL);
        }
    }
    systemctl(&["try-reload-or-restart", "lorawan-firewall.service"]);
}

/// `action=key` (make the key, answer the public key), `action=off`, or `config=` to save and connect.
pub fn save(body: &[u8]) -> Response {
    if !available() {
        return Response::new(404, "text/plain", "no WireGuard on this system\n");
    }
    let f = bridge::form_fields(body);
    let get = |k: &str| f.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
    match get("action").as_deref() {
        Some("key") => {
            return match private_key().and_then(|k| wg(&["pubkey"], Some(&k))) {
                Ok(p) => Response::new(200, "text/plain", format!("{p}\n")),
                Err(e) => Response::new(500, "text/plain", format!("{e}\n")),
            }
        }
        Some("off") => {
            systemctl(&["stop", UNIT]);
            let _ = fs::remove_file(CONF);
            set_firewall(None);
            return Response::new(
                200,
                "text/plain",
                "Tunnel off (the gateway keeps its key).\n",
            );
        }
        _ => {}
    }
    let Some(text) = get("config") else {
        return Response::new(400, "text/plain", "no configuration\n");
    };
    let key = match private_key() {
        Ok(k) => k,
        Err(e) => return Response::new(500, "text/plain", format!("{e}\n")),
    };
    let (conf, port) = match validate(&text, &key) {
        Ok(c) => c,
        Err(e) => return Response::new(400, "text/plain", format!("{e}\n")),
    };
    if let Err(e) = write_private(CONF, &conf) {
        return Response::new(500, "text/plain", format!("saving: {e}\n"));
    }
    set_firewall(port);
    if systemctl(&["restart", UNIT]) {
        Response::new(200, "text/plain", "Saved, tunnel up.\n")
    } else {
        Response::new(
            500,
            "text/plain",
            "Saved, but the tunnel did not come up: journalctl -u lorawan-wireguard on the gateway says why.\n",
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PK: &str = "aGVsbG8gd29ybGQgaGVsbG8gd29ybGQgaGVsbG8gd28=";
    const OK: &str = "[Interface]\nPrivateKey = whatever\nAddress = 10.8.0.5/32\n\n[Peer]\n\
        PublicKey = aGVsbG8gd29ybGQgaGVsbG8gd29ybGQgaGVsbG8gd28=\nEndpoint = vpn.example.org:51820\n\
        AllowedIPs = 10.8.0.0/24\nPersistentKeepalive = 25 # NAT\n";

    #[test]
    fn accepts_plain_settings() {
        let (c, port) = validate(OK, "OWNKEYOWNKEYOWNKEYOWNKEYOWNKEYOWNKEYOWNKEY0=").unwrap();
        assert_eq!(port, None);
        assert!(c.contains("PrivateKey = OWNKEY"), "{c}");
        assert!(!c.contains("whatever"));
        assert!(c.contains("Endpoint = vpn.example.org:51820\n"));
        assert!(!shareable(&c).contains("PrivateKey"));
        let (_, port) =
            validate(&OK.replace("Address", "ListenPort = 51820\nAddress"), PK).unwrap();
        assert_eq!(port, Some(51820));
    }

    #[test]
    fn refuses_commands_and_junk() {
        for (bad, why) in [
            (
                OK.replace("Address", "PostUp = rm -rf /\nAddress"),
                "PostUp",
            ),
            (OK.replace("Address", "DNS = 1.1.1.1\nAddress"), "DNS"),
            (OK.replace("vpn.example.org", "vpn;reboot"), "characters"),
            (
                OK.replace("PublicKey = aGVs", "PublicKey = xx"),
                "not a WireGuard key",
            ),
            (OK.replace("[Peer]", ""), "not accepted in [Interface]"),
            ("Address = 1.2.3.4/32\n".into(), "after [Interface]"),
            (OK.replace("Address = 10.8.0.5/32\n", ""), "Address"),
            (
                format!("{OK}\n[Peer]\nAllowedIPs = 10.9.0.0/24\n"),
                "no PublicKey",
            ),
        ] {
            let e = validate(&bad, PK).unwrap_err();
            assert!(e.contains(why), "{why}: {e}");
        }
    }
}
