// SPDX-License-Identifier: MIT
//! Prometheus metrics at `/metrics` (docs/lorawan-agent.md): the concentrator's counters as the panel shows them,
//! per channel and data rate, downlink results, duty cycle, the devices' last uplinks, the gateway's health and
//! system numbers. ChirpStack's own metrics are passed through at `/metrics/chirpstack` (its monitoring endpoint on
//! 127.0.0.1:8070).

use crate::devices;
use crate::http::{self, Response};
use crate::radio::{now, Radio};
use crate::status::{Health, Level};
use crate::sys::System;
use std::fmt::Write as _;
use std::time::Duration;

const CHIRPSTACK_METRICS: &str = "127.0.0.1:8070";

/// A label value as Prometheus' text format wants it.
fn lv(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

struct Out(String);

impl Out {
    fn head(&mut self, name: &str, kind: &str, help: &str) {
        let _ = writeln!(self.0, "# HELP {name} {help}\n# TYPE {name} {kind}");
    }
    fn val(&mut self, name: &str, labels: &[(&str, &str)], v: impl std::fmt::Display) {
        self.0.push_str(name);
        if !labels.is_empty() {
            self.0.push('{');
            for (i, (k, l)) in labels.iter().enumerate() {
                if i > 0 {
                    self.0.push(',');
                }
                let _ = write!(self.0, "{k}=\"{}\"", lv(l));
            }
            self.0.push('}');
        }
        let _ = writeln!(self.0, " {v}");
    }
    fn one(&mut self, name: &str, kind: &str, help: &str, v: impl std::fmt::Display) {
        self.head(name, kind, help);
        self.val(name, &[], v);
    }
}

pub fn render(radio: &Radio, system: &System, health: &Health, devs: &devices::Devices) -> String {
    let mut o = Out(String::new());
    let t = now();
    let gw = radio.gateway_id.clone().unwrap_or_default();
    let g: &[(&str, &str)] = &[("gateway", gw.as_str())];

    o.head(
        "lorawan_gateway_info",
        "gauge",
        "The gateway, its software and its state (1).",
    );
    let level = match health.level {
        Level::Ok => "ok",
        Level::Starting => "starting",
        Level::NoNetwork => "no-network",
        Level::Fault => "fault",
    };
    o.val(
        "lorawan_gateway_info",
        &[
            ("gateway", &gw),
            ("os", &system.os),
            ("kernel", &system.kernel),
            ("health", level),
        ],
        1,
    );
    o.one(
        "lorawan_gateway_healthy",
        "gauge",
        "1 when the panel shows the gateway as fine.",
        u8::from(health.level == Level::Ok),
    );
    o.one(
        "lorawan_forwarder_online",
        "gauge",
        "1 when the MQTT Forwarder reports online.",
        u8::from(radio.online == Some(true)),
    );
    if let Some(at) = radio.last_stats_at {
        o.one(
            "lorawan_stats_age_seconds",
            "gauge",
            "Seconds since the concentrator's last stats.",
            t.saturating_sub(at),
        );
    }

    for (name, help, v) in [
        (
            "lorawan_rx_frames_total",
            "Frames received (since the agent started).",
            radio.total.rx,
        ),
        (
            "lorawan_rx_ok_frames_total",
            "Frames received with a good CRC.",
            radio.total.rx_ok,
        ),
        (
            "lorawan_tx_requests_total",
            "Downlinks asked for.",
            radio.total.tx,
        ),
        (
            "lorawan_tx_emitted_total",
            "Downlinks transmitted.",
            radio.total.tx_emitted,
        ),
    ] {
        o.head(name, "counter", help);
        o.val(name, g, v);
    }

    o.head(
        "lorawan_rx_frequency_frames_total",
        "counter",
        "Frames received per frequency (Hz).",
    );
    for (f, n) in &radio.rx_per_frequency {
        o.val(
            "lorawan_rx_frequency_frames_total",
            &[("gateway", &gw), ("frequency", &f.to_string())],
            n,
        );
    }
    o.head(
        "lorawan_tx_frequency_frames_total",
        "counter",
        "Downlinks transmitted per frequency (Hz).",
    );
    for (f, n) in &radio.tx_per_frequency {
        o.val(
            "lorawan_tx_frequency_frames_total",
            &[("gateway", &gw), ("frequency", &f.to_string())],
            n,
        );
    }
    o.head(
        "lorawan_rx_modulation_frames_total",
        "counter",
        "Frames received per modulation (data rate).",
    );
    for (m, n) in &radio.rx_per_modulation {
        o.val(
            "lorawan_rx_modulation_frames_total",
            &[("gateway", &gw), ("modulation", m)],
            n,
        );
    }
    o.head(
        "lorawan_tx_status_total",
        "counter",
        "Downlink results from the concentrator (OK, TOO_LATE, ...).",
    );
    for (s, n) in &radio.tx_per_status {
        o.val(
            "lorawan_tx_status_total",
            &[("gateway", &gw), ("status", s)],
            n,
        );
    }
    o.head(
        "lorawan_tx_ack_total",
        "counter",
        "Downlink acknowledgements by result.",
    );
    for (s, n) in &radio.acks {
        o.val(
            "lorawan_tx_ack_total",
            &[("gateway", &gw), ("status", s)],
            n,
        );
    }
    if let Some(dc) = &radio.duty_cycle {
        o.head(
            "lorawan_duty_cycle_ratio",
            "gauge",
            "Transmit time used in the band's window, of what it allows (0..1).",
        );
        for b in &dc.bands {
            if b.load_max_ms > 0 {
                o.val(
                    "lorawan_duty_cycle_ratio",
                    &[("gateway", &gw), ("band", &b.name)],
                    format!("{:.4}", b.load_tracked_ms as f64 / b.load_max_ms as f64),
                );
            }
        }
    }

    o.head(
        "lorawan_device_last_uplink_seconds",
        "gauge",
        "Seconds since the device's last uplink.",
    );
    for d in devs.list.values() {
        o.val(
            "lorawan_device_last_uplink_seconds",
            &[
                ("dev_eui", &d.dev_eui),
                ("device", &d.name),
                ("application", &d.application),
            ],
            t.saturating_sub(d.last_seen),
        );
    }
    o.head(
        "lorawan_device_silent",
        "gauge",
        "1 when the device counts as silent (the Devices card's alert).",
    );
    for d in devs.list.values() {
        o.val(
            "lorawan_device_silent",
            &[("dev_eui", &d.dev_eui), ("device", &d.name)],
            u8::from(d.silent),
        );
    }
    o.head(
        "lorawan_device_rssi_dbm",
        "gauge",
        "RSSI of the device's last uplink (best gateway).",
    );
    for d in devs.list.values() {
        if let Some(r) = d.rssi {
            o.val(
                "lorawan_device_rssi_dbm",
                &[("dev_eui", &d.dev_eui), ("device", &d.name)],
                r,
            );
        }
    }
    o.head(
        "lorawan_device_snr_db",
        "gauge",
        "SNR of the device's last uplink (best gateway).",
    );
    for d in devs.list.values() {
        if let Some(s) = d.snr {
            o.val(
                "lorawan_device_snr_db",
                &[("dev_eui", &d.dev_eui), ("device", &d.name)],
                s,
            );
        }
    }

    o.head(
        "lorawan_service_up",
        "gauge",
        "1 when the systemd unit is active.",
    );
    for (unit, state) in &system.services {
        o.val(
            "lorawan_service_up",
            &[("unit", unit)],
            u8::from(state == "active"),
        );
    }
    o.one(
        "node_uptime_seconds",
        "gauge",
        "Seconds since boot.",
        system.uptime_s,
    );
    o.one(
        "node_load1",
        "gauge",
        "Load average, 1 minute.",
        system.load[0],
    );
    o.one(
        "node_memory_total_bytes",
        "gauge",
        "Memory.",
        system.mem_total_kb * 1024,
    );
    o.one(
        "node_memory_available_bytes",
        "gauge",
        "Memory available.",
        system.mem_available_kb * 1024,
    );
    o.one(
        "node_root_size_bytes",
        "gauge",
        "Data file system size.",
        system.disk_total_kb * 1024,
    );
    o.one(
        "node_root_used_bytes",
        "gauge",
        "Data file system used.",
        system.disk_used_kb * 1024,
    );
    o.head("node_temperature_celsius", "gauge", "Thermal zones.");
    for (zone, c) in &system.temps {
        o.val("node_temperature_celsius", &[("zone", zone)], c);
    }
    o.0
}

/// ChirpStack's own metrics (when its monitoring endpoint is on), passed through.
pub fn chirpstack() -> Response {
    match http::request(
        CHIRPSTACK_METRICS,
        "GET",
        "/metrics",
        &[],
        b"",
        Duration::from_secs(5),
    ) {
        Ok(r) if r.status == 200 => Response::new(200, "text/plain; version=0.0.4", r.body),
        Ok(r) => Response::new(
            502,
            "text/plain",
            format!("ChirpStack answered {}\n", r.status),
        ),
        Err(e) => Response::new(502, "text/plain", format!("ChirpStack's metrics: {e}\n")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::radio::Counts;

    #[test]
    fn text_format() {
        let mut r = Radio {
            gateway_id: Some("0016c001f150193b".into()),
            total: Counts {
                rx: 5,
                rx_ok: 4,
                tx: 1,
                tx_emitted: 1,
            },
            ..Default::default()
        };
        r.rx_per_frequency.insert(868_100_000, 3);
        r.tx_per_status.insert("OK".into(), 1);
        let s = System {
            os: "LoRaWAN \"Gateway\" OS".into(),
            services: vec![
                ("chirpstack".into(), "active".into()),
                ("redis".into(), "failed".into()),
            ],
            temps: vec![("cpu".into(), 41.5)],
            ..Default::default()
        };
        let h = Health {
            level: Level::Ok,
            problems: vec![],
        };
        let mut devs = devices::Devices::default();
        devs.list.insert(
            "5945a04c076f6287".into(),
            devices::Device {
                dev_eui: "5945a04c076f6287".into(),
                name: "xiao".into(),
                rssi: Some(-60),
                ..Default::default()
            },
        );
        let m = render(&r, &s, &h, &devs);
        assert!(
            m.contains("lorawan_rx_frames_total{gateway=\"0016c001f150193b\"} 5\n"),
            "{m}"
        );
        assert!(m.contains("lorawan_rx_frequency_frames_total{gateway=\"0016c001f150193b\",frequency=\"868100000\"} 3\n"));
        assert!(m.contains("os=\"LoRaWAN \\\"Gateway\\\" OS\""));
        assert!(m.contains("lorawan_service_up{unit=\"redis\"} 0\n"));
        assert!(m.contains(
            "lorawan_device_rssi_dbm{dev_eui=\"5945a04c076f6287\",device=\"xiao\"} -60\n"
        ));
        assert!(m.contains("lorawan_gateway_healthy 1\n"));
        // Every sample line belongs to a declared metric
        for line in m.lines().filter(|l| !l.starts_with('#')) {
            let name = line.split(['{', ' ']).next().unwrap();
            assert!(m.contains(&format!("# TYPE {name} ")), "undeclared: {line}");
        }
    }
}
