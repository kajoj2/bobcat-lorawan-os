// SPDX-License-Identifier: MIT
//! The panel's "Devices" card (docs/lorawan-agent.md): every LoRaWAN device the local ChirpStack hears, when it was
//! last heard, how often it usually sends, and an alert when it goes quiet. A device is "silent" when nothing came
//! for longer than the threshold (60 min by default) and three of its usual intervals; it is "ok" again with its next
//! uplink. Each change is published (retained) on the local broker as `lorawan/alerts/<DevEUI>`, which the Home
//! Assistant bridge passes on together with a discovery entry (a "problem" binary sensor per device), and optionally
//! posted to a webhook (ntfy or any URL taking a plain text POST).

use crate::bridge;
use crate::http::Response;
use crate::json;
use crate::mqtt::{Client, Publisher};
use crate::radio::now;
use crate::status::json_str;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

pub const DIR: &str = "/var/lib/lorawan-alerts";
const SETTINGS: &str = "/var/lib/lorawan-alerts/settings";
const STATE: &str = "/var/lib/lorawan-alerts/devices.tsv";
pub const TOPIC: &str = "lorawan/alerts";
const UPLINKS: &str = "application/+/device/+/event/up";
const DEFAULT_THRESHOLD_MIN: u64 = 60;
const MAX_DEVICES: usize = 1000;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Device {
    pub dev_eui: String,
    pub name: String,
    pub application: String,
    pub last_seen: u64,
    /// Seconds between the last two uplinks, smoothed (0: not known yet)
    pub interval: u64,
    pub rssi: Option<i32>,
    pub snr: Option<f32>,
    pub uplinks: u64,
    pub silent: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub threshold_min: u64,
    pub webhook: String,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            threshold_min: DEFAULT_THRESHOLD_MIN,
            webhook: String::new(),
        }
    }
}

#[derive(Default)]
pub struct Devices {
    pub list: BTreeMap<String, Device>,
    pub settings: Settings,
    pub mqtt_connected: bool,
    dirty: bool,
}

pub type Shared = Arc<Mutex<Devices>>;

/// When a device counts as silent: after the threshold, and never before three of its usual intervals.
pub fn silent_after(d: &Device, threshold_min: u64) -> u64 {
    (threshold_min * 60).max(d.interval.saturating_mul(3))
}

/// What an uplink changed for its device.
pub struct Recorded {
    pub device: Device,
    /// It was silent until now
    pub back: bool,
    /// First uplink seen from it
    pub new: bool,
}

/// Records an uplink from ChirpStack's JSON event.
pub fn record_uplink(devs: &mut Devices, payload: &[u8], at: u64) -> Option<Recorded> {
    let v = json::parse(payload).ok()?;
    let info = v.get("deviceInfo");
    let eui = info.get("devEui").str()?.to_ascii_lowercase();
    if eui.len() != 16 || !eui.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let new = !devs.list.contains_key(&eui);
    if new && devs.list.len() >= MAX_DEVICES {
        return None;
    }
    let best = v.get("rxInfo").arr().iter().max_by(|a, b| {
        a.get("rssi")
            .num()
            .unwrap_or(-999.0)
            .total_cmp(&b.get("rssi").num().unwrap_or(-999.0))
    });
    let d = devs.list.entry(eui.clone()).or_insert_with(|| Device {
        dev_eui: eui,
        ..Default::default()
    });
    if d.last_seen > 0 && at > d.last_seen {
        let gap = at - d.last_seen;
        // Retransmissions and bursts come seconds apart: they say nothing about the usual interval.
        if gap >= 10 {
            d.interval = if d.interval == 0 {
                gap
            } else {
                (d.interval * 3 + gap) / 4
            };
        }
    }
    d.name = info.get("deviceName").str().unwrap_or_default().to_string();
    d.application = info
        .get("applicationName")
        .str()
        .unwrap_or_default()
        .to_string();
    d.last_seen = at;
    d.uplinks += 1;
    d.rssi = best.and_then(|r| r.get("rssi").num()).map(|n| n as i32);
    d.snr = best.and_then(|r| r.get("snr").num()).map(|n| n as f32);
    let back = d.silent;
    d.silent = false;
    let device = d.clone();
    devs.dirty = true;
    Some(Recorded { device, back, new })
}

/// Devices that just went silent.
pub fn newly_silent(devs: &mut Devices, at: u64) -> Vec<Device> {
    let t = devs.settings.threshold_min;
    let mut out = Vec::new();
    for d in devs.list.values_mut() {
        if !d.silent && d.last_seen > 0 && at.saturating_sub(d.last_seen) > silent_after(d, t) {
            d.silent = true;
            out.push(d.clone());
        }
    }
    if !out.is_empty() {
        devs.dirty = true;
    }
    out
}

// --- Persistence ---------------------------------------------------------------------------------------------

pub fn parse_settings(text: &str) -> Settings {
    let mut s = Settings::default();
    for line in text.lines() {
        match line.split_once('=') {
            Some(("threshold_min", v)) => {
                if let Ok(n) = v.trim().parse::<u64>() {
                    s.threshold_min = n.clamp(5, 7 * 24 * 60);
                }
            }
            Some(("webhook", v)) => s.webhook = v.trim().to_string(),
            _ => {}
        }
    }
    s
}

fn render_settings(s: &Settings) -> String {
    format!("threshold_min={}\nwebhook={}\n", s.threshold_min, s.webhook)
}

/// One device per line: DevEUI, last seen, interval, uplinks, silent, name, application (tabs replaced).
pub fn render_state(list: &BTreeMap<String, Device>) -> String {
    let clean = |s: &str| s.replace(['\t', '\n', '\r'], " ");
    list.values()
        .map(|d| {
            format!(
                "{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
                d.dev_eui,
                d.last_seen,
                d.interval,
                d.uplinks,
                u8::from(d.silent),
                clean(&d.name),
                clean(&d.application)
            )
        })
        .collect()
}

pub fn parse_state(text: &str) -> BTreeMap<String, Device> {
    let mut m = BTreeMap::new();
    for line in text.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        if f.len() < 7 || f[0].len() != 16 {
            continue;
        }
        let d = Device {
            dev_eui: f[0].to_string(),
            last_seen: f[1].parse().unwrap_or(0),
            interval: f[2].parse().unwrap_or(0),
            uplinks: f[3].parse().unwrap_or(0),
            silent: f[4] == "1",
            name: f[5].to_string(),
            application: f[6].to_string(),
            ..Default::default()
        };
        m.insert(d.dev_eui.clone(), d);
    }
    m
}

fn write_atomic(path: &str, text: &str) -> std::io::Result<()> {
    fs::create_dir_all(DIR)?;
    let tmp = format!("{path}.tmp");
    fs::write(&tmp, text)?;
    fs::rename(&tmp, path)
}

pub fn load() -> Shared {
    Arc::new(Mutex::new(Devices {
        list: parse_state(&fs::read_to_string(STATE).unwrap_or_default()),
        settings: parse_settings(&fs::read_to_string(SETTINGS).unwrap_or_default()),
        ..Default::default()
    }))
}

// --- Notifications -------------------------------------------------------------------------------------------

fn label(d: &Device) -> &str {
    if d.name.is_empty() {
        &d.dev_eui
    } else {
        &d.name
    }
}

pub fn alert_json(d: &Device, at: u64) -> String {
    let mut o = String::from("{\"state\":");
    json_str(&mut o, if d.silent { "silent" } else { "ok" });
    o.push_str(",\"dev_eui\":");
    json_str(&mut o, &d.dev_eui);
    o.push_str(",\"device\":");
    json_str(&mut o, &d.name);
    o.push_str(",\"application\":");
    json_str(&mut o, &d.application);
    let _ = write!(
        o,
        ",\"last_seen\":{},\"silent_for\":{},\"interval\":{}}}",
        d.last_seen,
        if d.silent {
            at.saturating_sub(d.last_seen)
        } else {
            0
        },
        d.interval
    );
    o
}

/// Home Assistant's MQTT discovery entry for a device's alert: a "problem" binary sensor, read from the alert as it
/// arrives on Home Assistant's broker (with the bridge's topic prefix).
pub fn discovery(d: &Device, prefix: &str) -> (String, String) {
    let id = format!("lorawan_{}", d.dev_eui);
    let mut o = String::from("{\"name\":\"Silent\",\"unique_id\":");
    json_str(&mut o, &format!("{id}_silent"));
    o.push_str(",\"state_topic\":");
    json_str(&mut o, &format!("{prefix}{TOPIC}/{}", d.dev_eui));
    o.push_str(
        ",\"value_template\":\"{{ 'ON' if value_json.state == 'silent' else 'OFF' }}\",\"device_class\":\"problem\",\
         \"json_attributes_topic\":",
    );
    json_str(&mut o, &format!("{prefix}{TOPIC}/{}", d.dev_eui));
    o.push_str(",\"device\":{\"identifiers\":[");
    json_str(&mut o, &id);
    o.push_str("],\"name\":");
    json_str(&mut o, label(d));
    o.push_str(",\"model\":\"LoRaWAN device\"}}");
    (format!("homeassistant/binary_sensor/{id}/silent/config"), o)
}

fn webhook(url: &str, d: &Device) {
    if url.is_empty() {
        return;
    }
    let title = if d.silent {
        format!("{} is silent", label(d))
    } else {
        format!("{} is back", label(d))
    };
    let body = if d.silent {
        format!(
            "No uplink from {} ({}, {}) for {}.",
            label(d),
            d.dev_eui,
            if d.application.is_empty() {
                "-"
            } else {
                &d.application
            },
            crate::status::human_duration(now().saturating_sub(d.last_seen))
        )
    } else {
        format!("{} ({}) sends again.", label(d), d.dev_eui)
    };
    // curl does the HTTPS; the URL and texts go as arguments, never through a shell.
    let r = Command::new("curl")
        .args(["-fsS", "-m", "15", "-o", "/dev/null", "-H"])
        .arg(format!("Title: {title}"))
        .args(["-H", "Tags: warning", "--data-binary"])
        .arg(&body)
        .arg("--")
        .arg(url)
        .stdin(Stdio::null())
        .status();
    if !matches!(r, Ok(s) if s.success()) {
        eprintln!("devices: webhook for {} failed", d.dev_eui);
    }
}

fn announce(publisher: &mut Publisher, d: &Device, settings: &Settings, at: u64) {
    let topic = format!("{TOPIC}/{}", d.dev_eui);
    if let Err(e) = publisher.publish(&topic, alert_json(d, at).as_bytes(), true) {
        eprintln!("devices: publishing {topic}: {e}");
    }
    let url = settings.webhook.clone();
    let d = d.clone();
    thread::spawn(move || webhook(&url, &d));
}

/// Follows ChirpStack's uplinks on the local broker, checks for silent devices every minute, saves the list every
/// 5 minutes.
pub fn spawn(shared: Shared, broker: String) {
    {
        let shared = shared.clone();
        let broker = broker.clone();
        thread::spawn(move || {
            let mut publisher = Publisher::new(
                &broker,
                &format!("lorawan-agent-alerts-{}", std::process::id()),
            );
            let mut announced: Option<String> = None;
            let mut tick = 0u64;
            loop {
                thread::sleep(Duration::from_secs(60));
                tick += 1;
                let at = now();
                let (silent, settings, save, all) = {
                    let mut d = shared.lock().unwrap();
                    let s = newly_silent(&mut d, at);
                    let save = d.dirty && (tick.is_multiple_of(5) || !s.is_empty());
                    if save {
                        d.dirty = false;
                    }
                    (
                        s,
                        d.settings.clone(),
                        save.then(|| render_state(&d.list)),
                        d.list.clone(),
                    )
                };
                for d in &silent {
                    announce(&mut publisher, d, &settings, at);
                }
                if let Some(text) = save {
                    let _ = write_atomic(STATE, &text);
                }
                // Discovery entries for Home Assistant when a bridge is set (again when its prefix changes)
                let prefix = bridge::configured().map(|c| c.prefix);
                if let Some(p) = &prefix {
                    if announced.as_deref() != Some(p.as_str()) || tick.is_multiple_of(60) {
                        for d in all.values() {
                            let (t, cfg) = discovery(d, p);
                            let _ = publisher.publish(&t, cfg.as_bytes(), true);
                            let _ = publisher.publish(
                                &format!("{TOPIC}/{}", d.dev_eui),
                                alert_json(d, at).as_bytes(),
                                true,
                            );
                        }
                    }
                }
                announced = prefix;
                publisher.idle();
            }
        });
    }
    thread::spawn(move || {
        let mut publisher =
            Publisher::new(&broker, &format!("lorawan-agent-up-{}", std::process::id()));
        let mut pause = Duration::from_secs(2);
        loop {
            let r: std::io::Result<()> = Client::connect(
                &broker,
                &format!("lorawan-agent-devices-{}", std::process::id()),
                Duration::from_secs(60),
            )
            .and_then(|mut c| {
                c.subscribe(&[UPLINKS])?;
                shared.lock().unwrap().mqtt_connected = true;
                pause = Duration::from_secs(2);
                loop {
                    let p = c.next_publish()?;
                    let at = now();
                    let (rec, settings) = {
                        let mut d = shared.lock().unwrap();
                        (record_uplink(&mut d, &p.payload, at), d.settings.clone())
                    };
                    if let Some(Recorded {
                        device: dev,
                        back,
                        new,
                    }) = rec
                    {
                        if back {
                            announce(&mut publisher, &dev, &settings, at);
                        }
                        if new {
                            if let Some(b) = bridge::configured() {
                                let (t, cfg) = discovery(&dev, &b.prefix);
                                let _ = publisher.publish(&t, cfg.as_bytes(), true);
                            }
                            let _ = publisher.publish(
                                &format!("{TOPIC}/{}", dev.dev_eui),
                                alert_json(&dev, at).as_bytes(),
                                true,
                            );
                        }
                    }
                }
            });
            if let Err(e) = r {
                eprintln!("devices: broker: {e}");
            }
            shared.lock().unwrap().mqtt_connected = false;
            thread::sleep(pause);
            pause = (pause * 2).min(Duration::from_secs(30));
        }
    });
}

// --- Panel ---------------------------------------------------------------------------------------------------

/// The webhook's host only: the full URL often carries a secret (an ntfy topic, a token).
fn url_host(url: &str) -> &str {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    rest.split(['/', '?', '#']).next().unwrap_or_default()
}

pub fn info_json(shared: &Shared) -> String {
    let d = shared.lock().unwrap();
    let at = now();
    let mut o = String::new();
    let _ = write!(
        o,
        "{{\"now\":{at},\"mqtt_connected\":{},\"threshold_min\":{},\"webhook\":",
        d.mqtt_connected, d.settings.threshold_min
    );
    json_str(&mut o, url_host(&d.settings.webhook));
    o.push_str(",\"devices\":[");
    let mut list: Vec<&Device> = d.list.values().collect();
    list.sort_by_key(|x| std::cmp::Reverse(x.last_seen));
    for (i, x) in list.iter().enumerate() {
        if i > 0 {
            o.push(',');
        }
        o.push_str("{\"dev_eui\":");
        json_str(&mut o, &x.dev_eui);
        o.push_str(",\"name\":");
        json_str(&mut o, &x.name);
        o.push_str(",\"application\":");
        json_str(&mut o, &x.application);
        let _ = write!(
            o,
            ",\"last_seen\":{},\"interval\":{},\"uplinks\":{},\"silent\":{},\"silent_after\":{},\"rssi\":{},\"snr\":{}}}",
            x.last_seen,
            x.interval,
            x.uplinks,
            x.silent,
            silent_after(x, d.settings.threshold_min),
            x.rssi.map_or("null".into(), |v| v.to_string()),
            x.snr.map_or("null".into(), |v| format!("{v:.1}"))
        );
    }
    o.push_str("]}");
    o
}

pub fn save(shared: &Shared, body: &[u8]) -> Response {
    let f = bridge::form_fields(body);
    let get = |k: &str| {
        f.iter()
            .find(|(n, _)| n == k)
            .map(|(_, v)| v.trim().to_string())
    };
    let mut d = shared.lock().unwrap();
    if let Some(eui) = get("forget") {
        let eui = eui.to_ascii_lowercase();
        if d.list.remove(&eui).is_none() {
            return Response::new(404, "text/plain", "no such device\n");
        }
        d.dirty = true;
        let _ = write_atomic(STATE, &render_state(&d.list));
        return Response::new(
            200,
            "text/plain",
            "Removed from the list (it comes back with its next uplink).\n",
        );
    }
    let mut s = d.settings.clone();
    if let Some(t) = get("threshold_min") {
        match t.parse::<u64>() {
            Ok(n) if (5..=7 * 24 * 60).contains(&n) => s.threshold_min = n,
            _ => {
                return Response::new(
                    400,
                    "text/plain",
                    "threshold: 5 minutes to 7 days, in minutes\n",
                )
            }
        }
    }
    if get("webhook_clear").is_some() {
        s.webhook.clear();
    } else if let Some(u) = get("webhook").filter(|u| !u.is_empty()) {
        if !(u.starts_with("https://") || u.starts_with("http://"))
            || u.len() > 512
            || u.chars().any(|c| c.is_whitespace() || c.is_control())
        {
            return Response::new(
                400,
                "text/plain",
                "webhook: an http:// or https:// URL without spaces\n",
            );
        }
        s.webhook = u;
    }
    if let Err(e) = write_atomic(SETTINGS, &render_settings(&s)) {
        return Response::new(500, "text/plain", format!("saving: {e}\n"));
    }
    d.settings = s;
    Response::new(200, "text/plain", "Saved.\n")
}

/// Sends a test notification through the webhook.
pub fn test_webhook(shared: &Shared) -> Response {
    let url = shared.lock().unwrap().settings.webhook.clone();
    if url.is_empty() {
        return Response::new(400, "text/plain", "no webhook set\n");
    }
    let d = Device {
        dev_eui: "0000000000000000".into(),
        name: "Test from the gateway".into(),
        ..Default::default()
    };
    webhook(&url, &d);
    Response::new(200, "text/plain", "Sent (check the receiving app).\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    const UP: &str = r#"{"deduplicationId":"x","time":"2026-11-22T10:00:00Z","deviceInfo":{"tenantName":"ChirpStack",
        "applicationName":"sensors","applicationId":"bd75","deviceName":"xiao","devEui":"5945A04C076F6287"},
        "fCnt":3,"rxInfo":[{"rssi":-80,"snr":7.5},{"rssi":-59,"snr":9.25}]}"#;

    #[test]
    fn uplinks_and_silence() {
        let mut devs = Devices::default();
        let r = record_uplink(&mut devs, UP.as_bytes(), 1000).unwrap();
        assert!(!r.back && r.new);
        let d = r.device;
        assert_eq!(d.dev_eui, "5945a04c076f6287");
        assert_eq!(
            (d.name.as_str(), d.application.as_str()),
            ("xiao", "sensors")
        );
        assert_eq!((d.rssi, d.snr), (Some(-59), Some(9.25)));
        record_uplink(&mut devs, UP.as_bytes(), 1003); // a retransmission: no interval
        record_uplink(&mut devs, UP.as_bytes(), 1603);
        assert_eq!(devs.list["5945a04c076f6287"].interval, 600);
        // Threshold 60 min > 3 intervals (30 min): silent after an hour
        assert!(newly_silent(&mut devs, 1603 + 3600).is_empty());
        let s = newly_silent(&mut devs, 1603 + 3601);
        assert_eq!(s.len(), 1);
        assert!(
            newly_silent(&mut devs, 1603 + 9000).is_empty(),
            "announced once"
        );
        let r = record_uplink(&mut devs, UP.as_bytes(), 1603 + 9000).unwrap();
        assert!(r.back && !r.new, "back after being silent");
        assert!(record_uplink(&mut devs, b"{\"deviceInfo\":{\"devEui\":\"zz\"}}", 1).is_none());
        assert!(record_uplink(&mut devs, b"not json", 1).is_none());
    }

    #[test]
    fn long_intervals_stretch_the_threshold() {
        let d = Device {
            interval: 6 * 3600,
            ..Default::default()
        };
        assert_eq!(silent_after(&d, 60), 18 * 3600);
        assert_eq!(silent_after(&Device::default(), 60), 3600);
    }

    #[test]
    fn state_and_settings_round_trip() {
        let mut devs = Devices::default();
        record_uplink(&mut devs, UP.as_bytes(), 1000);
        record_uplink(&mut devs, UP.as_bytes(), 1600);
        devs.list.get_mut("5945a04c076f6287").unwrap().name = "a\tb".into();
        let back = parse_state(&render_state(&devs.list));
        let d = &back["5945a04c076f6287"];
        assert_eq!(
            (d.last_seen, d.interval, d.uplinks, d.name.as_str()),
            (1600, 600, 2, "a b")
        );
        let s = parse_settings("threshold_min=1\nwebhook=https://ntfy.sh/x\n");
        assert_eq!(s.threshold_min, 5);
        assert_eq!(parse_settings(&render_settings(&s)), s);
        assert_eq!(parse_settings(""), Settings::default());
    }

    #[test]
    fn alert_and_discovery() {
        let d = Device {
            dev_eui: "5945a04c076f6287".into(),
            name: "xiao".into(),
            silent: true,
            last_seen: 100,
            ..Default::default()
        };
        let a = alert_json(&d, 400);
        assert!(a.starts_with("{\"state\":\"silent\""), "{a}");
        assert!(a.contains("\"silent_for\":300"));
        assert!(json::parse(a.as_bytes()).is_ok());
        let (t, cfg) = discovery(&d, "chirpstack/");
        assert_eq!(
            t,
            "homeassistant/binary_sensor/lorawan_5945a04c076f6287/silent/config"
        );
        let v = json::parse(cfg.as_bytes()).unwrap();
        assert_eq!(
            v.get("state_topic").str(),
            Some("chirpstack/lorawan/alerts/5945a04c076f6287")
        );
        assert_eq!(v.at(&["device", "name"]).str(), Some("xiao"));
    }

    #[test]
    fn webhook_host_only() {
        assert_eq!(url_host("https://ntfy.sh/secret-topic"), "ntfy.sh");
        assert_eq!(url_host("http://10.0.0.2:8080/hook?t=1"), "10.0.0.2:8080");
        assert_eq!(url_host(""), "");
    }
}
