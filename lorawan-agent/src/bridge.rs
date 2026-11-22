// SPDX-License-Identifier: MIT
//! The panel's Home Assistant page: a Mosquitto bridge from the gateway's local broker to another MQTT broker
//! (typically Home Assistant's Mosquitto add-on), so ChirpStack's application events reach Home Assistant without
//! opening the local broker to the network. This module writes the bridge's configuration into a directory that
//! Mosquitto includes (`include_dir`, set up by lorawan-gateway-config), restarts Mosquitto, and follows the
//! bridge's connection state from its local notification topic. The broker password is never sent back.

use crate::forward;
use crate::mqtt;
use crate::radio::now;
use crate::status::json_str;
use std::fmt::Write as _;
use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::os::unix::fs::OpenOptionsExt;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

/// Its directory is created by tmpfiles.d with mode 2750 root:mosquitto, so the file belongs to Mosquitto's group.
pub const FILE: &str = "/var/lib/lorawan-mqtt-bridge/home-assistant.conf";
const UNIT: &str = "mosquitto";
const CONNECTION: &str = "home-assistant";
/// Mosquitto publishes "1"/"0" here (retained, local broker only) when the bridge connects/disconnects.
pub const STATE_TOPIC: &str = "lorawan-agent/bridge/state";
pub const DEFAULT_PREFIX: &str = "chirpstack/";
const MAX_CREDENTIAL: usize = 128;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Config {
    /// host:port of the remote broker
    pub server: String,
    pub username: String,
    pub password: String,
    pub tls: bool,
    /// Prepended to the topics on the remote broker: "chirpstack/" makes application/... chirpstack/application/...
    pub prefix: String,
    /// Also take downlink commands (application/+/device/+/command/down) from the remote broker.
    pub downlinks: bool,
}

/// The remote broker: host or host:port; without a port, 1883 (8883 with TLS).
pub fn validate_server(s: &str, tls: bool) -> Result<String, String> {
    let s = s.trim();
    if s.is_empty() {
        return Err("the broker address is empty".into());
    }
    let with_port = if s.contains(':') {
        s.to_string()
    } else {
        format!("{s}:{}", if tls { 8883 } else { 1883 })
    };
    forward::validate(&with_port)
}

/// User names and passwords go into Mosquitto's configuration file one per line: printable ASCII, no spaces.
pub fn validate_credential(what: &str, s: &str) -> Result<String, String> {
    if s.len() > MAX_CREDENTIAL {
        return Err(format!(
            "the {what} is longer than {MAX_CREDENTIAL} characters"
        ));
    }
    if !s.bytes().all(|b| (0x21..=0x7e).contains(&b)) {
        return Err(format!(
            "the {what} may only use printable ASCII characters without spaces"
        ));
    }
    Ok(s.to_string())
}

/// Letters, digits, '_', '-' and '/' (no MQTT wildcards, no leading '/'); a '/' is added at the end. Empty: none.
pub fn validate_prefix(s: &str) -> Result<String, String> {
    let s = s.trim();
    if s.is_empty() {
        return Ok(String::new());
    }
    if s.len() > 64
        || s.starts_with('/')
        || s.contains("//")
        || !s
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-/".contains(&b))
    {
        return Err(format!(
            "{s}: the topic prefix may use letters, digits, '_', '-' and '/' (not at the start), up to 64 characters"
        ));
    }
    Ok(if s.ends_with('/') {
        s.to_string()
    } else {
        format!("{s}/")
    })
}

/// The bridge section for Mosquitto. `client_id` identifies the gateway on the remote broker.
pub fn render(c: &Config, client_id: &str) -> String {
    let mut o = String::from(
        "# Written by lorawan-agent (the gateway's panel, Home Assistant card); change it there.\n\
         # See docs/home-assistant.md.\n",
    );
    let _ = writeln!(o, "connection {CONNECTION}");
    let _ = writeln!(o, "address {}", c.server);
    let _ = writeln!(o, "remote_clientid {client_id}");
    if !c.username.is_empty() {
        let _ = writeln!(o, "remote_username {}", c.username);
        if !c.password.is_empty() {
            let _ = writeln!(o, "remote_password {}", c.password);
        }
    }
    if c.tls {
        o.push_str("bridge_capath /etc/ssl/certs\n");
    }
    o.push_str(
        "bridge_protocol_version mqttv311\ntry_private false\ncleansession true\nrestart_timeout 10 120\n\
         notifications true\nnotifications_local_only true\n",
    );
    let _ = writeln!(o, "notification_topic {STATE_TOPIC}");
    let p = if c.prefix.is_empty() {
        String::new()
    } else {
        format!(" \"\" {}", c.prefix)
    };
    let _ = writeln!(o, "topic application/# out 0{p}");
    // The agent's own topics: device alerts and BLE sensors (lorawan/...), and Home Assistant's discovery entries
    // for them, which must arrive without the prefix
    let _ = writeln!(o, "topic lorawan/# out 0{p}");
    o.push_str("topic homeassistant/# out 0\n");
    if c.downlinks {
        let _ = writeln!(o, "topic application/+/device/+/command/down in 0{p}");
    }
    o
}

/// The settings in a file written by render().
pub fn parse(text: &str) -> Option<Config> {
    let mut c = Config::default();
    let mut found = false;
    for line in text.lines() {
        let Some((k, v)) = line.trim().split_once(' ') else {
            continue;
        };
        let v = v.trim();
        match k {
            "address" => {
                c.server = v.to_string();
                found = true;
            }
            "remote_username" => c.username = v.to_string(),
            "remote_password" => c.password = v.to_string(),
            "bridge_capath" | "bridge_cafile" => c.tls = true,
            "topic" => {
                let f: Vec<&str> = v.split_whitespace().collect();
                if f.get(1) == Some(&"in") {
                    c.downlinks = true;
                }
                if f.first() == Some(&"application/#") {
                    c.prefix = f.get(4).map(|s| s.to_string()).unwrap_or_default();
                }
            }
            _ => {}
        }
    }
    found.then_some(c)
}

pub fn configured() -> Option<Config> {
    parse(&fs::read_to_string(FILE).ok()?)
}

/// "lorawan-<host name>", restricted to characters every broker accepts in a client ID.
pub fn client_id() -> String {
    let host = fs::read_to_string("/proc/sys/kernel/hostname").unwrap_or_default();
    let host: String = host
        .trim()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .take(40)
        .collect();
    format!(
        "lorawan-{}",
        if host.is_empty() { "gateway" } else { &host }
    )
}

pub fn form_fields(body: &[u8]) -> Vec<(String, String)> {
    String::from_utf8_lossy(body)
        .split('&')
        .filter_map(|kv| {
            let (k, v) = kv.split_once('=')?;
            Some((k.to_string(), forward::percent_decode(v)))
        })
        .collect()
}

/// The new configuration from the card's form, or None to turn the bridge off (`action=off`). An empty password
/// keeps the stored one (the panel never sees it), unless the user name changed or is empty.
pub fn config_from_form(body: &[u8], old: Option<&Config>) -> Result<Option<Config>, String> {
    let f = form_fields(body);
    let get = |k: &str| {
        f.iter()
            .find(|(n, _)| n == k)
            .map(|(_, v)| v.as_str())
            .unwrap_or("")
    };
    if get("action") == "off" {
        return Ok(None);
    }
    let tls = get("tls") == "on";
    let username = validate_credential("user name", get("username").trim())?;
    let mut password = validate_credential("password", get("password"))?;
    if password.is_empty() && !username.is_empty() {
        if let Some(o) = old.filter(|o| o.username == username) {
            password = o.password.clone();
        }
    }
    Ok(Some(Config {
        server: validate_server(get("server"), tls)?,
        username,
        password,
        tls,
        prefix: validate_prefix(get("prefix"))?,
        downlinks: get("downlinks") == "on",
    }))
}

fn write_config(text: &str) -> std::io::Result<()> {
    let tmp = format!("{FILE}.new"); // Mosquitto only includes *.conf
    let mut f = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o640)
        .open(&tmp)?;
    f.write_all(text.as_bytes())?;
    f.sync_all()?;
    fs::rename(&tmp, FILE)
}

/// Saves (or removes) the bridge and restarts Mosquitto. Local clients (ChirpStack, the forwarders, this agent)
/// reconnect within seconds.
pub fn save(body: &[u8]) -> crate::http::Response {
    use crate::http::Response;
    let old = configured();
    let new = match config_from_form(body, old.as_ref()) {
        Ok(c) => c,
        Err(e) => return Response::new(400, "text/plain", format!("{e}\n")),
    };
    let written = match &new {
        None => match fs::remove_file(FILE) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        },
        Some(c) => write_config(&render(c, &client_id())),
    };
    if let Err(e) = written {
        return Response::new(500, "text/plain", format!("{FILE}: {e}\n"));
    }
    match Command::new("systemctl").args(["restart", UNIT]).status() {
        Ok(s) if s.success() => Response::new(
            200,
            "text/plain",
            if new.is_some() {
                "saved, Mosquitto restarted\n"
            } else {
                "bridge off, Mosquitto restarted\n"
            },
        ),
        Ok(s) => Response::new(
            500,
            "text/plain",
            format!("systemctl restart {UNIT}: {s}\n"),
        ),
        Err(e) => Response::new(500, "text/plain", format!("systemctl: {e}\n")),
    }
}

/// A bridge written by an older agent gets the current topics (the agent's own `lorawan/#` and the discovery
/// entries): rewritten from its settings, with a Mosquitto restart, when the text differs. Run once at start.
pub fn refresh() {
    let Some(c) = configured() else { return };
    let text = render(&c, &client_id());
    if fs::read_to_string(FILE).ok().as_deref() == Some(text.as_str()) {
        return;
    }
    match write_config(&text) {
        Ok(()) => {
            eprintln!("bridge: updated {FILE} to this version's topics");
            let _ = Command::new("systemctl")
                .args(["try-restart", UNIT])
                .status();
        }
        Err(e) => eprintln!("bridge: updating {FILE}: {e}"),
    }
}

/// The bridge's last state message from Mosquitto: connected or not, and since when (Unix time).
pub type Watch = Arc<Mutex<Option<(bool, u64)>>>;

/// Follows STATE_TOPIC on the local broker. Mosquitto keeps no retained messages over a restart (no persistence),
/// so the state starts empty after each save.
pub fn spawn_watch(broker: String) -> Watch {
    let w: Watch = Default::default();
    let shared = w.clone();
    thread::spawn(move || loop {
        let r = (|| -> std::io::Result<()> {
            let mut c =
                mqtt::Client::connect(&broker, "lorawan-agent-bridge", Duration::from_secs(60))?;
            c.subscribe(&[STATE_TOPIC])?;
            loop {
                let p = c.next_publish()?;
                if p.topic == STATE_TOPIC {
                    *shared.lock().unwrap() = Some((p.payload.first() == Some(&b'1'), now()));
                }
            }
        })();
        if r.is_err() {
            // Mosquitto restarting (after a save) forgets the retained state too
            *shared.lock().unwrap() = None;
        }
        thread::sleep(Duration::from_secs(3));
    });
    w
}

pub fn info_json(watch: &Watch, default_password: Option<bool>) -> String {
    let c = configured();
    let mut o = String::from("{\"configured\":");
    o.push_str(if c.is_some() { "true" } else { "false" });
    let c = c.unwrap_or_default();
    o.push_str(",\"server\":");
    json_str(&mut o, &c.server);
    o.push_str(",\"username\":");
    json_str(&mut o, &c.username);
    let _ = write!(
        o,
        ",\"password_set\":{},\"tls\":{},\"downlinks\":{}",
        !c.password.is_empty(),
        c.tls,
        c.downlinks
    );
    o.push_str(",\"prefix\":");
    json_str(
        &mut o,
        if c.server.is_empty() {
            DEFAULT_PREFIX
        } else {
            &c.prefix
        },
    );
    o.push_str(",\"client_id\":");
    json_str(&mut o, &client_id());
    let (state, since) = match *watch.lock().unwrap() {
        Some((true, t)) => ("connected", t),
        Some((false, t)) => ("disconnected", t),
        None => ("unknown", 0),
    };
    o.push_str(",\"state\":");
    json_str(&mut o, state);
    let dp = match default_password {
        Some(true) => "true",
        Some(false) => "false",
        None => "null",
    };
    let _ = write!(
        o,
        ",\"since\":{since},\"default_password\":{dp},\"now\":{}}}",
        now()
    );
    o
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> Config {
        Config {
            server: "homeassistant.local:1883".into(),
            username: "lorawan".into(),
            password: "s3cret!".into(),
            tls: false,
            prefix: "chirpstack/".into(),
            downlinks: true,
        }
    }

    #[test]
    fn server() {
        assert_eq!(validate_server("ha.local", false).unwrap(), "ha.local:1883");
        assert_eq!(validate_server("ha.local", true).unwrap(), "ha.local:8883");
        assert_eq!(
            validate_server(" 10.0.0.2:1884 ", false).unwrap(),
            "10.0.0.2:1884"
        );
        for bad in ["", "a b", "x:0", "a\nconnection x", "$HOME"] {
            assert!(validate_server(bad, false).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn credentials_and_prefix() {
        assert!(validate_credential("password", "a b").is_err());
        assert!(validate_credential("password", "a\nb").is_err());
        assert!(validate_credential("password", &"x".repeat(129)).is_err());
        assert_eq!(validate_credential("password", "P@ss#1").unwrap(), "P@ss#1");
        assert_eq!(validate_prefix("").unwrap(), "");
        assert_eq!(validate_prefix("lorawan").unwrap(), "lorawan/");
        assert_eq!(validate_prefix("home/lora/").unwrap(), "home/lora/");
        for bad in ["/x", "a+b", "a#", "a b", "a//b", "a\"b"] {
            assert!(validate_prefix(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn render_and_parse() {
        let c = cfg();
        let text = render(&c, "lorawan-bobcat");
        assert!(text.contains("connection home-assistant\n"));
        assert!(text.contains("remote_clientid lorawan-bobcat\n"));
        assert!(text.contains("topic application/# out 0 \"\" chirpstack/\n"));
        assert!(text.contains("topic application/+/device/+/command/down in 0 \"\" chirpstack/\n"));
        assert!(text.contains(&format!("notification_topic {STATE_TOPIC}\n")));
        assert!(!text.contains("bridge_capath"));
        assert_eq!(parse(&text), Some(c.clone()));

        let plain = Config {
            username: String::new(),
            password: String::new(),
            tls: true,
            prefix: String::new(),
            downlinks: false,
            ..c
        };
        let text = render(&plain, "x");
        assert!(text.contains("topic application/# out 0\n"));
        assert!(text.contains("bridge_capath /etc/ssl/certs\n"));
        assert!(!text.contains(" in 0"));
        assert!(!text.contains("remote_username") && !text.contains("remote_password"));
        assert_eq!(parse(&text), Some(plain));
        assert_eq!(parse("# nothing\n"), None);
    }

    #[test]
    fn form() {
        let old = cfg();
        // Empty password keeps the stored one for the same user
        let c = config_from_form(
            b"server=homeassistant.local&username=lorawan&password=&prefix=chirpstack&downlinks=on",
            Some(&old),
        )
        .unwrap()
        .unwrap();
        assert_eq!(c, old);
        // ... but not for another user
        let c = config_from_form(b"server=ha.local&username=other&password=", Some(&old))
            .unwrap()
            .unwrap();
        assert_eq!(c.password, "");
        assert!(!c.downlinks);
        assert_eq!(c.prefix, "");
        // New password, TLS, percent-encoded
        let c = config_from_form(
            b"server=broker.example.com&username=u&password=a%40b%23c&tls=on&prefix=x%2Fy",
            None,
        )
        .unwrap()
        .unwrap();
        assert_eq!(c.server, "broker.example.com:8883");
        assert_eq!(c.password, "a@b#c");
        assert_eq!(c.prefix, "x/y/");
        assert!(c.tls);
        assert_eq!(config_from_form(b"action=off", Some(&old)).unwrap(), None);
        assert!(config_from_form(b"server=", None).is_err());
        assert!(config_from_form(b"server=ha.local&password=a%20b", None).is_err());
        assert!(config_from_form(b"server=ha.local%0Aconnection%20x", None).is_err());
    }
}
