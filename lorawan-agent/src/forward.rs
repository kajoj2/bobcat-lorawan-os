// SPDX-License-Identifier: MIT
//! The panel's forwarding page: share the gateway with community networks such as The Things Network, next to the
//! local ChirpStack. chirpstack-udp-forwarder does the work (Semtech UDP); this module writes its server list,
//! restarts it, and reads its counters to show whether each server answers. Changing the list needs no login for
//! now; the card warns while ChirpStack still has its default password.
//!
//! Helium is one more entry on that list: the Helium gateway (gateway-rs, `helium-gateway.service`) listens for
//! Semtech UDP on localhost and connects to the Helium network as a data-only hotspot (docs/helium.md). Choosing
//! it starts that service too; the card shows the hotspot's address and name for onboarding.

use crate::http::{self, Response};
use crate::radio::now;
use crate::status::json_str;
use std::collections::HashMap;
use std::fmt::Write as _;
use std::fs;
use std::path::Path;
use std::process::Command;
use std::sync::Mutex;
use std::time::Duration;

pub const FILE: &str = "/var/lib/lorawan-forward/servers.toml";
pub const UNIT: &str = "chirpstack-udp-forwarder";
const METRICS: &str = "127.0.0.1:9800";
const MAX_SERVERS: usize = 4;
/// The forwarder sends a keepalive (PULL_DATA) every 10 s; a server that answered within this time is up.
const ANSWER_WINDOW: u64 = 45;

/// The Helium gateway's Semtech UDP address (its settings.toml) and its service.
pub const HELIUM: &str = "127.0.0.1:1680";
pub const HELIUM_UNIT: &str = "helium-gateway";
const HELIUM_BIN: &str = "/usr/bin/helium_gateway";

/// Servers offered on the panel, with a name to show. Anything else is entered as host:port.
pub const PRESETS: [(&str, &str); 4] = [
    (
        "eu1.cloud.thethings.network:1700",
        "The Things Network, Europe (eu1)",
    ),
    (
        "nam1.cloud.thethings.network:1700",
        "The Things Network, North America (nam1)",
    ),
    (
        "au1.cloud.thethings.network:1700",
        "The Things Network, Australia (au1)",
    ),
    (HELIUM, "Helium (data-only hotspot, onboard it first)"),
];

fn helium_installed() -> bool {
    Path::new(HELIUM_BIN).exists()
}

pub fn label(server: &str) -> &'static str {
    PRESETS
        .iter()
        .find(|(s, _)| *s == server)
        .map(|(_, l)| *l)
        .unwrap_or("")
}

/// "host:port" with a DNS name or an IPv4 address; anything else is refused (the value goes into a TOML file,
/// and the forwarder replaces "$NAME" in it with environment variables).
pub fn validate(s: &str) -> Result<String, String> {
    let s = s.trim().to_ascii_lowercase();
    let (host, port) = s
        .rsplit_once(':')
        .ok_or_else(|| format!("{s}: expected host:port, e.g. eu1.cloud.thethings.network:1700"))?;
    let port_ok = port.parse::<u16>().is_ok_and(|p| p > 0);
    let host_ok = !host.is_empty()
        && host.len() <= 253
        && host.split('.').all(|l| {
            !l.is_empty()
                && l.len() <= 63
                && !l.starts_with('-')
                && !l.ends_with('-')
                && l.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        });
    if !port_ok {
        return Err(format!("{s}: the port must be 1..65535"));
    }
    if !host_ok {
        return Err(format!("{s}: not a valid host name"));
    }
    Ok(s)
}

/// The forwarder's server list, appended to its main configuration file.
pub fn render(servers: &[String]) -> String {
    let mut o = String::from(
        "# Written by lorawan-agent (the gateway's panel); change it there. See docs/forwarding.md.\n",
    );
    for s in servers {
        let _ = write!(
            o,
            "\n[[udp_forwarder.servers]]\n  server = \"{s}\"\n  keepalive_interval_secs = 10\n  \
             keepalive_max_failures = 12\n\n  [udp_forwarder.servers.filters]\n    forward_crc_ok = true\n    \
             forward_crc_invalid = false\n    forward_crc_missing = false\n"
        );
    }
    o
}

/// The servers in a file written by render().
pub fn parse(toml: &str) -> Vec<String> {
    toml.lines()
        .filter_map(|l| {
            let (k, v) = l.split_once('=')?;
            (k.trim() == "server").then(|| v.trim().trim_matches('"').to_string())
        })
        .collect()
}

pub fn configured() -> Vec<String> {
    parse(&fs::read_to_string(FILE).unwrap_or_default())
}

fn percent_decode(s: &str) -> String {
    let hex = |c: u8| (c as char).to_digit(16).map(|d| d as u8);
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match (
            b[i],
            b.get(i + 1).copied().and_then(hex),
            b.get(i + 2).copied().and_then(hex),
        ) {
            (b'+', _, _) => out.push(b' '),
            (b'%', Some(h), Some(l)) => {
                out.push(h << 4 | l);
                i += 2;
            }
            (c, _, _) => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The `server` values of a form body (`server=a&server=b`), checked; duplicates dropped.
pub fn servers_from_form(body: &[u8]) -> Result<Vec<String>, String> {
    let form = String::from_utf8_lossy(body);
    let mut servers: Vec<String> = Vec::new();
    for kv in form.split('&') {
        let Some((k, v)) = kv.split_once('=') else {
            continue;
        };
        if k != "server" {
            continue;
        }
        let v = percent_decode(v);
        if v.trim().is_empty() {
            continue;
        }
        let s = validate(&v)?;
        if !servers.contains(&s) {
            servers.push(s);
        }
    }
    if servers.len() > MAX_SERVERS {
        return Err(format!("at most {MAX_SERVERS} servers"));
    }
    Ok(servers)
}

/// Saves the list and (re)starts the forwarder, or stops it for an empty list.
pub fn save(body: &[u8]) -> Response {
    let servers = match servers_from_form(body) {
        Ok(s) => s,
        Err(e) => return Response::new(400, "text/plain", format!("{e}\n")),
    };
    if servers.iter().any(|s| s == HELIUM) && !helium_installed() {
        return Response::new(
            400,
            "text/plain",
            "the Helium gateway is not in this image\n",
        );
    }
    let written = if servers.is_empty() {
        match fs::remove_file(FILE) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    } else {
        let tmp = format!("{FILE}.new");
        Path::new(FILE)
            .parent()
            .map_or(Ok(()), fs::create_dir_all)
            .and_then(|_| fs::write(&tmp, render(&servers)))
            .and_then(|_| fs::rename(&tmp, FILE))
    };
    if let Err(e) = written {
        return Response::new(500, "text/plain", format!("{FILE}: {e}\n"));
    }
    // The Helium gateway first, so it listens before the forwarder sends to it.
    if servers.iter().any(|s| s == HELIUM) {
        match Command::new("systemctl")
            .args(["restart", HELIUM_UNIT])
            .status()
        {
            Ok(s) if s.success() => {}
            Ok(s) => {
                return Response::new(
                    500,
                    "text/plain",
                    format!("systemctl restart {HELIUM_UNIT}: {s}\n"),
                )
            }
            Err(e) => return Response::new(500, "text/plain", format!("systemctl: {e}\n")),
        }
    } else if helium_installed() {
        // Not running in the usual case; a failure here changes nothing for the other servers.
        let _ = Command::new("systemctl")
            .args(["stop", HELIUM_UNIT])
            .status();
    }
    let action = if servers.is_empty() {
        "stop"
    } else {
        "restart"
    };
    match Command::new("systemctl").args([action, UNIT]).status() {
        Ok(s) if s.success() => Response::new(
            200,
            "text/plain",
            if servers.is_empty() {
                "forwarding off\n"
            } else {
                "saved, forwarder restarted\n"
            },
        ),
        Ok(s) => Response::new(500, "text/plain", format!("systemctl {action}: {s}\n")),
        Err(e) => Response::new(500, "text/plain", format!("systemctl: {e}\n")),
    }
}

/// Datagram counts for one server, since the forwarder started.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Counters {
    pub keepalives: u64,
    pub keepalive_acks: u64,
    pub uplinks: u64,
    pub uplink_acks: u64,
    pub downlinks: u64,
}

/// The forwarder's Prometheus text, per server: `udp_sent_count_total{server="..",type="PULL_DATA"} 3`.
pub fn parse_metrics(text: &str) -> HashMap<String, Counters> {
    let mut m: HashMap<String, Counters> = HashMap::new();
    for line in text.lines() {
        let Some((name, rest)) = line.split_once('{') else {
            continue;
        };
        let Some((labels, value)) = rest.rsplit_once('}') else {
            continue;
        };
        let Ok(value) = value.trim().parse::<f64>() else {
            continue;
        };
        let label = |key: &str| {
            labels.split(',').find_map(|kv| {
                let (k, v) = kv.split_once('=')?;
                (k.trim() == key).then(|| v.trim().trim_matches('"').to_string())
            })
        };
        let (Some(server), Some(typ)) = (label("server"), label("type")) else {
            continue;
        };
        let c = m.entry(server).or_default();
        let v = value as u64;
        match (name.trim_end_matches("_total"), typ.as_str()) {
            ("udp_sent_count", "PULL_DATA") => c.keepalives = v,
            ("udp_sent_count", "PUSH_DATA_RXPK") => c.uplinks = v,
            ("udp_received_count", "PULL_ACK") => c.keepalive_acks = v,
            ("udp_received_count", "PUSH_ACK") => c.uplink_acks = v,
            ("udp_received_count", "PULL_RESP") => c.downlinks = v,
            _ => {}
        }
    }
    m
}

fn metrics() -> HashMap<String, Counters> {
    match http::request(METRICS, "GET", "/metrics", &[], b"", Duration::from_secs(2)) {
        Ok(r) if r.status == 200 => parse_metrics(&String::from_utf8_lossy(&r.body)),
        _ => HashMap::new(),
    }
}

/// When each server last answered a keepalive (the counters only ever grow while the forwarder runs).
#[derive(Default)]
pub struct Tracker(Mutex<HashMap<String, (u64, u64)>>);

impl Tracker {
    /// Unix time of the last answer seen from this server, if any.
    fn last_answer(&self, server: &str, c: &Counters, t: u64) -> Option<u64> {
        let mut m = self.0.lock().unwrap();
        let e = m.entry(server.to_string()).or_insert((0, 0));
        if c.keepalive_acks < e.0 {
            *e = (0, 0); // forwarder restarted
        }
        if c.keepalive_acks > e.0 {
            // Seen for the first time now: assume it is recent only if the server keeps up with the keepalives.
            let recent = e.0 > 0 || c.keepalives.saturating_sub(c.keepalive_acks) <= 2;
            *e = (c.keepalive_acks, if recent { t } else { e.1 });
        }
        (e.1 > 0).then_some(e.1)
    }
}

/// What `helium_gateway info key name router` prints (pretty JSON from serde): the hotspot's address (its public
/// key), its three-word name, and whether it holds a session with Helium's packet router.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HeliumInfo {
    pub key: String,
    pub name: String,
    pub router_connected: bool,
}

/// The string value of `"field": "..."`, or the bare value (true, 12) of `"field": true`; the first one found.
fn json_field<'a>(json: &'a str, field: &str) -> Option<&'a str> {
    let at = json.find(&format!("\"{field}\""))? + field.len() + 2;
    let rest = json[at..].trim_start().strip_prefix(':')?.trim_start();
    if let Some(r) = rest.strip_prefix('"') {
        r.find('"').map(|end| &r[..end])
    } else {
        let end = rest.find([',', '}', '\n']).unwrap_or(rest.len());
        Some(rest[..end].trim())
    }
}

pub fn parse_helium_info(json: &str) -> Option<HeliumInfo> {
    Some(HeliumInfo {
        key: json_field(json, "key")?.to_string(),
        name: json_field(json, "name").unwrap_or_default().to_string(),
        router_connected: json_field(json, "connected") == Some("true"),
    })
}

fn helium_info() -> Option<HeliumInfo> {
    let out = Command::new(HELIUM_BIN)
        .args([
            "-c",
            "/etc/helium_gateway/settings.toml",
            "info",
            "key",
            "name",
            "router",
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    parse_helium_info(&String::from_utf8_lossy(&out.stdout))
}

pub fn state(c: Option<&Counters>, last_answer: Option<u64>, t: u64) -> &'static str {
    match (c, last_answer) {
        (_, Some(a)) if t.saturating_sub(a) <= ANSWER_WINDOW => "connected",
        (Some(c), _) if c.keepalives >= 3 => "no-answer",
        _ => "starting",
    }
}

/// `default_password`: whether ChirpStack still accepts admin/admin (None: it did not answer).
pub fn info_json(tracker: &Tracker, default_password: Option<bool>) -> String {
    let servers = configured();
    let active = crate::sys::service_states(&[UNIT])
        .pop()
        .map(|(_, s)| s)
        .unwrap_or_default();
    let counters = if active == "active" {
        metrics()
    } else {
        HashMap::new()
    };
    let t = now();
    let mut o = String::from("{\"service\":");
    json_str(&mut o, &active);
    o.push_str(",\"servers\":[");
    for (i, s) in servers.iter().enumerate() {
        if i > 0 {
            o.push(',');
        }
        let c = counters.get(s);
        let last = c.and_then(|c| tracker.last_answer(s, c, t));
        let c0 = Counters::default();
        let cc = c.unwrap_or(&c0);
        o.push_str("{\"server\":");
        json_str(&mut o, s);
        o.push_str(",\"label\":");
        json_str(&mut o, label(s));
        o.push_str(",\"state\":");
        json_str(
            &mut o,
            if active == "active" {
                state(c, last, t)
            } else {
                "stopped"
            },
        );
        let _ = write!(
            o,
            ",\"last_answer\":{},\"uplinks\":{},\"uplink_acks\":{},\"downlinks\":{}}}",
            last.unwrap_or(0),
            cc.uplinks,
            cc.uplink_acks,
            cc.downlinks
        );
    }
    o.push_str("],\"helium\":");
    helium_json(&mut o, &servers);
    o.push_str(",\"presets\":[");
    let presets = PRESETS
        .iter()
        .filter(|(s, _)| *s != HELIUM || helium_installed());
    for (i, (s, l)) in presets.enumerate() {
        if i > 0 {
            o.push(',');
        }
        o.push_str("{\"server\":");
        json_str(&mut o, s);
        o.push_str(",\"label\":");
        json_str(&mut o, l);
        o.push('}');
    }
    let dp = match default_password {
        Some(true) => "true",
        Some(false) => "false",
        None => "null",
    };
    let _ = write!(o, "],\"default_password\":{dp},\"now\":{t}}}");
    o
}

/// null while Helium is not chosen; otherwise its service state and, while it runs, what it reports.
fn helium_json(o: &mut String, servers: &[String]) {
    if !servers.iter().any(|s| s == HELIUM) {
        o.push_str("null");
        return;
    }
    let active = crate::sys::service_states(&[HELIUM_UNIT])
        .pop()
        .map(|(_, s)| s)
        .unwrap_or_default();
    // Only while it runs: started by hand, the tool would create the key file itself, owned by root.
    let info = if active == "active" {
        helium_info()
    } else {
        None
    };
    o.push_str("{\"service\":");
    json_str(o, &active);
    match info {
        Some(i) => {
            o.push_str(",\"key\":");
            json_str(o, &i.key);
            o.push_str(",\"name\":");
            json_str(o, &i.name);
            let _ = write!(o, ",\"router_connected\":{}}}", i.router_connected);
        }
        None => o.push_str(",\"key\":null,\"name\":null,\"router_connected\":false}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validation() {
        assert_eq!(
            validate(" EU1.cloud.thethings.network:1700 ").unwrap(),
            "eu1.cloud.thethings.network:1700"
        );
        assert_eq!(validate("10.0.0.5:1700").unwrap(), "10.0.0.5:1700");
        for bad in [
            "",
            "host",
            "host:0",
            "host:70000",
            ":1700",
            "a..b:1700",
            "-a.b:1700",
            "a\"b:1700",
            "$HOME:1700",
            "a b:1700",
            "a\nserver=x:1700",
        ] {
            assert!(validate(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn form() {
        let s = servers_from_form(
            b"server=eu1.cloud.thethings.network%3A1700&server=&server=10.0.0.5:1700&server=EU1.cloud.thethings.network:1700&x=1",
        )
        .unwrap();
        assert_eq!(s, ["eu1.cloud.thethings.network:1700", "10.0.0.5:1700"]);
        assert_eq!(servers_from_form(b"").unwrap(), Vec::<String>::new());
        assert!(servers_from_form(b"server=a%22b%3A1").is_err());
        assert!(
            servers_from_form(b"server=a:1&server=b:1&server=c:1&server=d:1&server=e:1").is_err()
        );
    }

    #[test]
    fn render_and_parse() {
        let servers = vec![
            "eu1.cloud.thethings.network:1700".to_string(),
            "10.0.0.5:1700".into(),
        ];
        let toml = render(&servers);
        assert_eq!(parse(&toml), servers);
        assert_eq!(toml.matches("[[udp_forwarder.servers]]").count(), 2);
        assert!(!toml.contains('$'));
    }

    #[test]
    fn metrics_text() {
        let text = "# HELP udp_sent_count Number of UDP datagrams sent.\n\
# TYPE udp_sent_count counter\n\
udp_sent_count_total{server=\"eu1.cloud.thethings.network:1700\",type=\"PULL_DATA\"} 12\n\
udp_sent_count_total{server=\"eu1.cloud.thethings.network:1700\",type=\"PUSH_DATA_RXPK\"} 5\n\
udp_received_count_total{server=\"eu1.cloud.thethings.network:1700\",type=\"PULL_ACK\"} 11\n\
udp_received_count_total{server=\"eu1.cloud.thethings.network:1700\",type=\"PUSH_ACK\"} 9\n\
udp_received_count_total{server=\"eu1.cloud.thethings.network:1700\",type=\"PULL_RESP\"} 1\n\
udp_sent_bytes_total{server=\"eu1.cloud.thethings.network:1700\",type=\"PULL_DATA\"} 144\n\
# EOF\n";
        let m = parse_metrics(text);
        assert_eq!(
            m["eu1.cloud.thethings.network:1700"],
            Counters {
                keepalives: 12,
                keepalive_acks: 11,
                uplinks: 5,
                uplink_acks: 9,
                downlinks: 1
            }
        );
    }

    #[test]
    fn answer_tracking() {
        let tr = Tracker::default();
        let c = |k, a| Counters {
            keepalives: k,
            keepalive_acks: a,
            ..Default::default()
        };
        // No answer yet
        assert_eq!(tr.last_answer("s", &c(1, 0), 100), None);
        assert_eq!(state(Some(&c(1, 0)), None, 100), "starting");
        assert_eq!(state(Some(&c(5, 0)), None, 140), "no-answer");
        // Keeps up: answered now
        assert_eq!(tr.last_answer("s", &c(6, 5), 150), Some(150));
        assert_eq!(state(Some(&c(6, 5)), Some(150), 160), "connected");
        // No new answer for a minute
        assert_eq!(tr.last_answer("s", &c(12, 5), 210), Some(150));
        assert_eq!(state(Some(&c(12, 5)), Some(150), 210), "no-answer");
        // An old backlog of answers on the first look (agent restarted) is not "recent"
        let tr2 = Tracker::default();
        assert_eq!(tr2.last_answer("s", &c(100, 20), 100), None);
        assert_eq!(tr2.last_answer("s", &c(101, 21), 110), Some(110));
    }

    #[test]
    fn helium_info_json() {
        let out = r#"{
  "name": "brave-orange-lion",
  "key": "13jL4pMh8DCA3NXdWLSq8gtPqDPWz9sJ7AqHiJXzJeZKa5kxwXj",
  "router": {
    "uri": "http://mainnet-router.helium.io:8080/",
    "connected": true,
    "session_key": "14cS..."
  }
}
"#;
        assert_eq!(
            parse_helium_info(out),
            Some(HeliumInfo {
                key: "13jL4pMh8DCA3NXdWLSq8gtPqDPWz9sJ7AqHiJXzJeZKa5kxwXj".into(),
                name: "brave-orange-lion".into(),
                router_connected: true,
            })
        );
        let off = out.replace("\"connected\": true", "\"connected\": false");
        assert!(!parse_helium_info(&off).unwrap().router_connected);
        assert_eq!(parse_helium_info("{}"), None);
        assert!(validate(HELIUM).is_ok());
    }

    #[test]
    fn decoding() {
        assert_eq!(percent_decode("a%3A1700"), "a:1700");
        assert_eq!(percent_decode("a+b"), "a b");
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("%zz"), "%zz");
    }
}
