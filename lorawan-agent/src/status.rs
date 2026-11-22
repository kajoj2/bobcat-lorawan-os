// SPDX-License-Identifier: MIT
//! One status report: health verdict, radio, system. Rendered as JSON for the web panel and as text for
//! `lorawan-diag`.

use crate::radio::Radio;
use crate::sys::{Channel, System};
use std::fmt::Write as _;

pub const STATE_DIR: &str = "/var/lib/lorawan-agent";
/// Stats come every 30 s; this many seconds without any means the radio path is broken.
pub const STATS_STALE_S: u64 = 120;
/// After the agent starts, no stats yet is not a fault for this long (Concentratord and the forwarder start up).
pub const STARTUP_GRACE_S: u64 = 180;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Ok,
    Starting,
    NoNetwork,
    Fault,
}

impl Level {
    pub fn name(self) -> &'static str {
        match self {
            Level::Ok => "ok",
            Level::Starting => "starting",
            Level::NoNetwork => "no-network",
            Level::Fault => "fault",
        }
    }
}

pub struct Health {
    pub level: Level,
    pub problems: Vec<String>,
}

/// The verdict the LED shows. `services`: (unit, state); `network`: a default route on a live interface.
pub fn health(
    radio: &Radio,
    services: &[(String, String)],
    network: bool,
    agent_started: u64,
    now: u64,
) -> Health {
    let mut problems = Vec::new();
    for (unit, state) in services {
        if state != "active" {
            problems.push(format!("{unit} is {state}"));
        }
    }
    if !radio.mqtt_connected {
        problems.push(format!(
            "no connection to Mosquitto ({})",
            radio.mqtt_error.as_deref().unwrap_or("connecting")
        ));
    }
    let in_grace = now.saturating_sub(agent_started) < STARTUP_GRACE_S;
    match radio.last_stats_at {
        Some(t) if now.saturating_sub(t) <= STATS_STALE_S => {}
        Some(t) => problems.push(format!(
            "no stats from the concentrator for {} s",
            now.saturating_sub(t)
        )),
        None if !in_grace => problems.push("no stats from the concentrator yet".into()),
        None => {}
    }
    let level = if !problems.is_empty() {
        if in_grace {
            Level::Starting
        } else {
            Level::Fault
        }
    } else if !network {
        problems.push("no network: no interface with a default route".into());
        Level::NoNetwork
    } else if radio.last_stats_at.is_none() {
        Level::Starting
    } else {
        Level::Ok
    };
    Health { level, problems }
}

pub fn registered_gateway() -> Option<String> {
    std::fs::read_to_string(format!("{STATE_DIR}/registered"))
        .ok()
        .map(|s| s.trim().to_string())
}

pub struct Status {
    pub now: u64,
    pub agent_version: &'static str,
    pub health: Health,
    pub radio: Radio,
    pub system: System,
    pub registered: Option<String>,
    /// Concentratord's channel plan
    pub channels: Vec<Channel>,
    pub agent_running: bool,
}

// --- JSON -----------------------------------------------------------------------------------------------------

pub fn json_str(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

fn opt_str(out: &mut String, s: Option<&str>) {
    match s {
        Some(s) => json_str(out, s),
        None => out.push_str("null"),
    }
}

fn opt_num<T: std::fmt::Display>(out: &mut String, v: Option<T>) {
    match v {
        Some(v) => {
            let _ = write!(out, "{v}");
        }
        None => out.push_str("null"),
    }
}

fn list<T>(out: &mut String, items: &[T], mut f: impl FnMut(&mut String, &T)) {
    out.push('[');
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        f(out, item);
    }
    out.push(']');
}

fn pairs(out: &mut String, items: &[(String, String)]) {
    out.push('{');
    for (i, (k, v)) in items.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        json_str(out, k);
        out.push(':');
        json_str(out, v);
    }
    out.push('}');
}

fn counts_map<K: std::fmt::Display>(out: &mut String, m: &std::collections::BTreeMap<K, u64>) {
    out.push('{');
    for (i, (k, v)) in m.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        json_str(out, &k.to_string());
        let _ = write!(out, ":{v}");
    }
    out.push('}');
}

impl Status {
    pub fn to_json(&self) -> String {
        let (r, s) = (&self.radio, &self.system);
        let mut o = String::with_capacity(4096);
        let _ = write!(o, "{{\"now\":{},\"agent_version\":", self.now);
        json_str(&mut o, self.agent_version);
        o.push_str(",\"health\":{\"level\":");
        json_str(&mut o, self.health.level.name());
        o.push_str(",\"problems\":");
        list(&mut o, &self.health.problems, |o, p| json_str(o, p));
        o.push_str("},\"gateway\":{\"id\":");
        opt_str(&mut o, r.gateway_id.as_deref());
        o.push_str(",\"registered\":");
        opt_str(&mut o, self.registered.as_deref());
        o.push_str(",\"online\":");
        opt_num(&mut o, r.online);
        let _ = write!(
            o,
            ",\"mqtt_connected\":{},\"mqtt_error\":",
            r.mqtt_connected
        );
        opt_str(&mut o, r.mqtt_error.as_deref());
        o.push_str(",\"versions\":");
        pairs(&mut o, &r.versions);
        o.push_str("},\"radio\":{\"last_stats_at\":");
        opt_num(&mut o, r.last_stats_at);
        for (name, c) in [("last", r.last), ("total", r.total)] {
            let _ = write!(
                o,
                ",\"{name}\":{{\"rx\":{},\"rx_ok\":{},\"tx\":{},\"tx_emitted\":{}}}",
                c.rx, c.rx_ok, c.tx, c.tx_emitted
            );
        }
        for (name, m) in [
            ("rx_per_frequency", &r.rx_per_frequency),
            ("tx_per_frequency", &r.tx_per_frequency),
        ] {
            let _ = write!(o, ",\"{name}\":");
            counts_map(&mut o, m);
        }
        for (name, m) in [
            ("rx_per_modulation", &r.rx_per_modulation),
            ("tx_per_modulation", &r.tx_per_modulation),
            ("tx_per_status", &r.tx_per_status),
            ("downlink_results", &r.acks),
        ] {
            let _ = write!(o, ",\"{name}\":");
            counts_map(&mut o, m);
        }
        o.push_str(",\"duty_cycle\":");
        match &r.duty_cycle {
            Some(d) => {
                o.push_str("{\"regulation\":");
                json_str(&mut o, d.regulation);
                let _ = write!(o, ",\"window_s\":{},\"bands\":", d.window_s);
                list(&mut o, &d.bands, |o, b| {
                    o.push_str("{\"name\":");
                    json_str(o, &b.name);
                    let _ = write!(
                        o,
                        ",\"frequency_min\":{},\"frequency_max\":{},\"load_max_ms\":{},\"load_tracked_ms\":{}}}",
                        b.frequency_min, b.frequency_max, b.load_max_ms, b.load_tracked_ms
                    );
                });
                o.push('}');
            }
            None => o.push_str("null"),
        }
        o.push_str(",\"history\":");
        let hours: Vec<_> = r.history.iter().copied().collect();
        list(&mut o, &hours, |o, h| {
            let _ = write!(
                o,
                "{{\"start\":{},\"rx\":{},\"rx_ok\":{},\"tx\":{}}}",
                h.start, h.rx, h.rx_ok, h.tx
            );
        });
        o.push_str(",\"channels\":");
        list(&mut o, &self.channels, |o, c| {
            let _ = write!(o, "{{\"frequency\":{},\"kind\":", c.frequency);
            json_str(o, &c.kind);
            o.push('}');
        });
        o.push_str(",\"frames\":");
        let frames: Vec<_> = r.frames.iter().cloned().collect();
        list(&mut o, &frames, |o, f| {
            let _ = write!(
                o,
                "{{\"at\":{},\"frequency\":{},\"channel\":{},\"rf_chain\":{},\"rssi\":{},\"snr\":{:.1},\"crc_status\":{},\"size\":{},\"modulation\":",
                f.at, f.frequency, f.channel, f.rf_chain, f.rssi, f.snr, f.crc_status, f.size
            );
            json_str(o, &f.modulation);
            o.push_str(",\"code_rate\":");
            json_str(o, f.code_rate);
            o.push_str(",\"kind\":");
            json_str(o, f.kind);
            o.push_str(",\"device\":");
            json_str(o, &f.device);
            o.push_str(",\"f_cnt\":");
            opt_num(o, f.f_cnt);
            o.push('}');
        });
        o.push_str("},\"system\":{\"hostname\":");
        json_str(&mut o, &s.hostname);
        o.push_str(",\"os\":");
        json_str(&mut o, &s.os);
        o.push_str(",\"kernel\":");
        json_str(&mut o, &s.kernel);
        let _ = write!(
            o,
            ",\"uptime_s\":{},\"load\":[{:.2},{:.2},{:.2}],\"mem_total_kb\":{},\"mem_available_kb\":{},\"disk_total_kb\":{},\"disk_used_kb\":{}",
            s.uptime_s, s.load[0], s.load[1], s.load[2], s.mem_total_kb, s.mem_available_kb, s.disk_total_kb, s.disk_used_kb
        );
        o.push_str(",\"temps\":");
        list(&mut o, &s.temps, |o, (name, t)| {
            o.push_str("{\"name\":");
            json_str(o, name);
            let _ = write!(o, ",\"celsius\":{t:.1}}}");
        });
        o.push_str(",\"services\":");
        list(&mut o, &s.services, |o, (unit, state)| {
            o.push_str("{\"unit\":");
            json_str(o, unit);
            o.push_str(",\"state\":");
            json_str(o, state);
            o.push('}');
        });
        o.push_str(",\"interfaces\":");
        list(&mut o, &s.ifaces, |o, i| {
            o.push_str("{\"name\":");
            json_str(o, &i.name);
            let _ = write!(
                o,
                ",\"up\":{},\"carrier\":{},\"default_route\":{},\"speed_mbps\":",
                i.up, i.carrier, i.default_route
            );
            opt_num(o, i.speed_mbps);
            o.push_str(",\"ipv4\":");
            list(o, &i.ipv4, |o, a| json_str(o, a));
            o.push_str(",\"ipv6\":");
            list(o, &i.ipv6, |o, a| json_str(o, a));
            o.push_str(",\"ssid\":");
            opt_str(o, i.ssid.as_deref());
            o.push_str(",\"signal_dbm\":");
            opt_num(o, i.signal_dbm);
            o.push('}');
        });
        o.push_str("}}");
        o
    }

    pub fn to_text(&self) -> String {
        let (r, s) = (&self.radio, &self.system);
        let mut o = String::new();
        let ago = |t: u64| format!("{} s ago", self.now.saturating_sub(t));
        let _ = writeln!(
            o,
            "LoRaWAN gateway: {}  ({} {})",
            s.hostname, s.os, s.kernel
        );
        let _ = writeln!(o, "Health: {}", self.health.level.name().to_uppercase());
        for p in &self.health.problems {
            let _ = writeln!(o, "  - {p}");
        }
        if !self.agent_running {
            let _ = writeln!(o, "  (lorawan-agent is not running: no radio data below)");
        }
        let _ = writeln!(o, "\nGateway");
        let _ = writeln!(
            o,
            "  ID           {}",
            r.gateway_id
                .as_deref()
                .unwrap_or("unknown (no message from the forwarder yet)")
        );
        let _ = writeln!(
            o,
            "  ChirpStack   {}",
            match &self.registered {
                Some(id) => format!("registered ({id})"),
                None => "not registered yet (lorawan-agent-register.service)".into(),
            }
        );
        let _ = writeln!(
            o,
            "  MQTT         {}",
            if r.mqtt_connected {
                "connected".to_string()
            } else {
                format!("down: {}", r.mqtt_error.as_deref().unwrap_or("connecting"))
            }
        );
        let _ = writeln!(
            o,
            "  State        {}",
            match r.online {
                Some(true) => "online",
                Some(false) => "offline",
                None => "unknown",
            }
        );
        for (k, v) in &r.versions {
            let label = k.trim_end_matches("_version").replace('_', " ");
            let value = v
                .trim()
                .trim_start_matches("Version:")
                .trim()
                .trim_end_matches(';');
            let _ = writeln!(o, "  {label:<12} {value}");
        }
        let _ = writeln!(o, "\nRadio");
        match r.last_stats_at {
            Some(t) => {
                let _ = writeln!(
                    o,
                    "  last stats   {}: rx {} (CRC OK {}), tx {} (sent {})",
                    ago(t),
                    r.last.rx,
                    r.last.rx_ok,
                    r.last.tx,
                    r.last.tx_emitted
                );
            }
            None => {
                let _ = writeln!(o, "  no stats received yet");
            }
        }
        let _ = writeln!(
            o,
            "  since start  rx {} (CRC OK {}), tx {} (sent {})",
            r.total.rx, r.total.rx_ok, r.total.tx, r.total.tx_emitted
        );
        if !r.frames.is_empty() {
            let _ = writeln!(o, "  last frames:");
            for f in r.frames.iter().take(10) {
                let crc = match f.crc_status {
                    2 => "ok",
                    1 => "BAD",
                    _ => "none",
                };
                let _ = writeln!(
                    o,
                    "    {:>9}  {:.1} MHz {:<10} {:>4} dBm {:>5.1} dB  crc {crc:<4} {:<15} {} {}",
                    ago(f.at),
                    f64::from(f.frequency) / 1e6,
                    f.modulation,
                    f.rssi,
                    f.snr,
                    f.kind,
                    f.device,
                    f.f_cnt.map(|c| format!("fcnt {c}")).unwrap_or_default()
                );
            }
        }
        if !self.channels.is_empty() || !r.rx_per_frequency.is_empty() {
            let _ = writeln!(o, "\nChannels (frames since the agent started)");
            let mut freqs: Vec<u32> = self.channels.iter().map(|c| c.frequency).collect();
            freqs.extend(r.rx_per_frequency.keys().chain(r.tx_per_frequency.keys()));
            freqs.sort_unstable();
            freqs.dedup();
            for f in freqs {
                let kinds: Vec<&str> = self
                    .channels
                    .iter()
                    .filter(|c| c.frequency == f)
                    .map(|c| c.kind.as_str())
                    .collect();
                let _ = writeln!(
                    o,
                    "  {:.1} MHz  {:<28} rx {:>6}  tx {:>6}",
                    f64::from(f) / 1e6,
                    if kinds.is_empty() {
                        "(not in the plan)".to_string()
                    } else {
                        kinds.join(", ")
                    },
                    r.rx_per_frequency.get(&f).copied().unwrap_or(0),
                    r.tx_per_frequency.get(&f).copied().unwrap_or(0)
                );
            }
        }
        if !r.rx_per_modulation.is_empty() || !r.tx_per_modulation.is_empty() {
            let _ = writeln!(o, "\nData rates");
            let mut mods: Vec<&String> = r
                .rx_per_modulation
                .keys()
                .chain(r.tx_per_modulation.keys())
                .collect();
            mods.sort();
            mods.dedup();
            for m in mods {
                let _ = writeln!(
                    o,
                    "  {m:<16} rx {:>6}  tx {:>6}",
                    r.rx_per_modulation.get(m).copied().unwrap_or(0),
                    r.tx_per_modulation.get(m).copied().unwrap_or(0)
                );
            }
        }
        if !r.acks.is_empty() || !r.tx_per_status.is_empty() {
            let _ = writeln!(o, "\nDownlinks");
            for (k, v) in &r.acks {
                let _ = writeln!(o, "  {k:<20} {v}");
            }
            for (k, v) in &r.tx_per_status {
                let _ = writeln!(o, "  concentrator {k:<7} {v}");
            }
        }
        if let Some(d) = &r.duty_cycle {
            let _ = writeln!(
                o,
                "\nDuty cycle ({}, window {} s)",
                d.regulation, d.window_s
            );
            if d.bands.is_empty() {
                let _ = writeln!(o, "  no transmissions tracked yet");
            }
            for b in &d.bands {
                let pct = if b.load_max_ms > 0 {
                    100.0 * b.load_tracked_ms as f64 / b.load_max_ms as f64
                } else {
                    0.0
                };
                let _ = writeln!(
                    o,
                    "  {:<4} {:.3}-{:.3} MHz  {:>6.1} s of {:>6.1} s ({pct:.1} %)",
                    b.name,
                    f64::from(b.frequency_min) / 1e6,
                    f64::from(b.frequency_max) / 1e6,
                    b.load_tracked_ms as f64 / 1000.0,
                    b.load_max_ms as f64 / 1000.0
                );
            }
        }
        let _ = writeln!(o, "\nServices");
        for (unit, state) in &s.services {
            let _ = writeln!(
                o,
                "  {:<4} {unit}",
                if state == "active" { "ok" } else { "!!" }
            );
        }
        let _ = writeln!(o, "\nNetwork");
        for i in &s.ifaces {
            let mut line = format!(
                "  {:<6} {}",
                i.name,
                if i.carrier {
                    "link"
                } else if i.up {
                    "up, no link"
                } else {
                    "down"
                }
            );
            if let Some(sp) = i.speed_mbps {
                let _ = write!(line, " {sp} Mb/s");
            }
            if let Some(ssid) = &i.ssid {
                let _ = write!(line, " \"{ssid}\"");
            }
            if let Some(dbm) = i.signal_dbm {
                let _ = write!(line, " {dbm} dBm");
            }
            if i.default_route {
                line.push_str(" [default route]");
            }
            let _ = writeln!(o, "{line}");
            for a in i.ipv4.iter().chain(&i.ipv6) {
                let _ = writeln!(o, "           {a}");
            }
        }
        let _ = writeln!(o, "\nSystem");
        let _ = writeln!(o, "  uptime       {}", human_duration(s.uptime_s));
        let _ = writeln!(
            o,
            "  load         {:.2} {:.2} {:.2}",
            s.load[0], s.load[1], s.load[2]
        );
        let _ = writeln!(
            o,
            "  memory       {} MiB free of {} MiB",
            s.mem_available_kb / 1024,
            s.mem_total_kb / 1024
        );
        let _ = writeln!(
            o,
            "  disk /       {} MiB used of {} MiB",
            s.disk_used_kb / 1024,
            s.disk_total_kb / 1024
        );
        for (name, t) in &s.temps {
            let _ = writeln!(o, "  {name:<12} {t:.1} °C");
        }
        let _ = writeln!(
            o,
            "\nlorawan-agent {}, ChirpStack: http://<this gateway>:8080",
            self.agent_version
        );
        o
    }
}

pub fn human_duration(s: u64) -> String {
    let (d, h, m) = (s / 86400, s / 3600 % 24, s / 60 % 60);
    if d > 0 {
        format!("{d} d {h} h {m} min")
    } else if h > 0 {
        format!("{h} h {m} min")
    } else {
        format!("{m} min {} s", s % 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn active(units: &[&str]) -> Vec<(String, String)> {
        units
            .iter()
            .map(|u| (u.to_string(), "active".to_string()))
            .collect()
    }

    #[test]
    fn verdicts() {
        let services = active(&["chirpstack"]);
        let mut radio = Radio {
            mqtt_connected: true,
            last_stats_at: Some(1000),
            ..Default::default()
        };
        assert_eq!(health(&radio, &services, true, 0, 1030).level, Level::Ok);
        assert_eq!(
            health(&radio, &services, false, 0, 1030).level,
            Level::NoNetwork
        );
        // stats stopped
        assert_eq!(
            health(&radio, &services, true, 0, 1000 + STATS_STALE_S + 1).level,
            Level::Fault
        );
        // a service down
        let down = vec![("chirpstack".to_string(), "failed".to_string())];
        assert_eq!(health(&radio, &down, true, 0, 1030).level, Level::Fault);
        // just started: no stats yet is not a fault
        radio.last_stats_at = None;
        assert_eq!(
            health(&radio, &services, true, 1000, 1010).level,
            Level::Starting
        );
        assert_eq!(
            health(&radio, &services, true, 1000, 1000 + STARTUP_GRACE_S).level,
            Level::Fault
        );
    }

    #[test]
    fn json_escapes() {
        let mut s = String::new();
        json_str(&mut s, "a\"b\\c\n\u{1}");
        assert_eq!(s, r#""a\"b\\c\n\u0001""#);
    }

    #[test]
    fn durations() {
        assert_eq!(human_duration(59), "0 min 59 s");
        assert_eq!(human_duration(3 * 3600 + 120), "3 h 2 min");
        assert_eq!(human_duration(2 * 86400 + 3600), "2 d 1 h 0 min");
    }
}

/// "2026-11-22" for a Unix time (UTC).
pub fn utc_date(t: u64) -> String {
    // Days to civil date (Howard Hinnant's algorithm)
    let z = (t / 86400) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

#[cfg(test)]
mod date_tests {
    #[test]
    fn dates() {
        assert_eq!(super::utc_date(0), "1970-01-01");
        assert_eq!(super::utc_date(1_795_392_000), "2026-11-23");
        assert_eq!(super::utc_date(951_782_400), "2000-02-29");
    }
}
