// SPDX-License-Identifier: MIT
//! The system around the radio: services, network, resources. Read from /proc and /sys where possible; the few
//! tools it runs (systemctl, ip, df, wpa_cli) are in the image anyway.

use std::fs;
use std::path::Path;
use std::process::Command;

/// The gateway stack, in start order. Wi-Fi is added when it is configured.
pub const SERVICES: [&str; 5] = [
    "chirpstack-concentratord",
    "chirpstack-mqtt-forwarder",
    "mosquitto",
    "redis",
    "chirpstack",
];
pub const WIFI_CONFIG: &str = "/etc/wpa_supplicant/wpa_supplicant-wlan0.conf";

#[derive(Clone, Debug, Default)]
pub struct Iface {
    pub name: String,
    pub up: bool,
    pub carrier: bool,
    pub speed_mbps: Option<u32>,
    pub ipv4: Vec<String>,
    pub ipv6: Vec<String>,
    pub default_route: bool,
    pub ssid: Option<String>,
    pub signal_dbm: Option<i32>,
}

#[derive(Clone, Debug, Default)]
pub struct System {
    pub hostname: String,
    pub os: String,
    pub kernel: String,
    pub uptime_s: u64,
    pub load: [f32; 3],
    pub mem_total_kb: u64,
    pub mem_available_kb: u64,
    pub temps: Vec<(String, f32)>,
    pub disk_total_kb: u64,
    pub disk_used_kb: u64,
    pub services: Vec<(String, String)>,
    pub ifaces: Vec<Iface>,
}

fn read(path: impl AsRef<Path>) -> String {
    fs::read_to_string(path)
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

fn run(cmd: &str, args: &[&str]) -> String {
    Command::new(cmd)
        .args(args)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default()
}

/// `systemctl is-active a b c` prints one state per unit, in order.
pub fn service_states(units: &[&str]) -> Vec<(String, String)> {
    let out = run("systemctl", &[&["is-active", "--"][..], units].concat());
    let mut states = out.lines().map(str::to_string);
    units
        .iter()
        .map(|u| {
            (
                u.to_string(),
                states.next().unwrap_or_else(|| "unknown".into()),
            )
        })
        .collect()
}

pub fn units() -> Vec<&'static str> {
    let mut u = SERVICES.to_vec();
    if Path::new(WIFI_CONFIG).exists() {
        u.push("wpa_supplicant@wlan0");
    }
    // Optional: only watched once the panel has a server list for it.
    if Path::new(crate::forward::FILE).exists() {
        u.push(crate::forward::UNIT);
        if crate::forward::configured()
            .iter()
            .any(|s| s == crate::forward::HELIUM)
        {
            u.push(crate::forward::HELIUM_UNIT);
        }
    }
    u
}

/// Interfaces that carry the default route, from /proc/net/route (destination 00000000).
pub fn default_route_ifaces() -> Vec<String> {
    read("/proc/net/route")
        .lines()
        .skip(1)
        .filter_map(|l| {
            let mut f = l.split_whitespace();
            let (iface, dest) = (f.next()?, f.next()?);
            (dest == "00000000").then(|| iface.to_string())
        })
        .collect()
}

/// Cheap check, no processes: some wired or wireless interface is up with a default route.
pub fn network_ok() -> bool {
    default_route_ifaces()
        .iter()
        .any(|i| read(format!("/sys/class/net/{i}/operstate")) == "up")
}

fn addresses(iface: &str, family: &str) -> Vec<String> {
    run("ip", &[family, "addr", "show", "dev", iface])
        .lines()
        .filter_map(|l| {
            let l = l.trim();
            let rest = l
                .strip_prefix("inet6 ")
                .or_else(|| l.strip_prefix("inet "))?;
            if family == "-6" && !l.contains("scope global") {
                return None;
            }
            rest.split_whitespace().next().map(str::to_string)
        })
        .collect()
}

fn wifi_link(iface: &str) -> (Option<String>, Option<i32>) {
    let status = run("wpa_cli", &["-i", iface, "status"]);
    let ssid = status
        .lines()
        .find_map(|l| l.strip_prefix("ssid="))
        .map(str::to_string);
    let signal = run("wpa_cli", &["-i", iface, "signal_poll"])
        .lines()
        .find_map(|l| l.strip_prefix("RSSI=")?.trim().parse().ok());
    (ssid, signal)
}

pub fn ifaces() -> Vec<Iface> {
    let defaults = default_route_ifaces();
    let mut names: Vec<String> = fs::read_dir("/sys/class/net")
        .map(|d| {
            d.filter_map(|e| e.ok()?.file_name().into_string().ok())
                .collect()
        })
        .unwrap_or_default();
    names.retain(|n| n != "lo");
    names.sort();
    names
        .into_iter()
        .map(|name| {
            let base = format!("/sys/class/net/{name}");
            let wireless = Path::new(&format!("{base}/wireless")).exists();
            let carrier = read(format!("{base}/carrier")) == "1";
            let (ssid, signal_dbm) = if wireless && carrier {
                wifi_link(&name)
            } else {
                (None, None)
            };
            Iface {
                up: read(format!("{base}/operstate")) == "up",
                carrier,
                speed_mbps: if wireless {
                    None
                } else {
                    read(format!("{base}/speed"))
                        .parse()
                        .ok()
                        .filter(|s| *s > 0)
                },
                ipv4: addresses(&name, "-4"),
                ipv6: addresses(&name, "-6"),
                default_route: defaults.contains(&name),
                ssid,
                signal_dbm,
                name,
            }
        })
        .collect()
}

fn meminfo() -> (u64, u64) {
    let info = read("/proc/meminfo");
    let get = |key: &str| {
        info.lines()
            .find_map(|l| l.strip_prefix(key)?.split_whitespace().next()?.parse().ok())
            .unwrap_or(0)
    };
    (get("MemTotal:"), get("MemAvailable:"))
}

fn temps() -> Vec<(String, f32)> {
    let mut zones: Vec<_> = fs::read_dir("/sys/class/thermal")
        .map(|d| d.filter_map(|e| e.ok()).map(|e| e.path()).collect())
        .unwrap_or_default();
    zones.retain(|p: &std::path::PathBuf| {
        p.file_name()
            .is_some_and(|n| n.to_string_lossy().starts_with("thermal_zone"))
    });
    zones.sort();
    zones
        .iter()
        .filter_map(|z| {
            let milli: f32 = read(z.join("temp")).parse().ok()?;
            Some((read(z.join("type")), milli / 1000.0))
        })
        .collect()
}

/// `df -Pk /`: the root file system's size and use in KiB.
fn disk() -> (u64, u64) {
    let out = run("df", &["-Pk", "/"]);
    let f: Vec<&str> = out
        .lines()
        .nth(1)
        .unwrap_or_default()
        .split_whitespace()
        .collect();
    let num = |i: usize| f.get(i).and_then(|v| v.parse().ok()).unwrap_or(0);
    (num(1), num(2))
}

pub fn os_name() -> String {
    read("/etc/os-release")
        .lines()
        .find_map(|l| l.strip_prefix("PRETTY_NAME="))
        .map(|v| v.trim_matches('"').to_string())
        .unwrap_or_default()
}

pub fn snapshot() -> System {
    let load: Vec<f32> = read("/proc/loadavg")
        .split_whitespace()
        .take(3)
        .filter_map(|v| v.parse().ok())
        .collect();
    let (mem_total_kb, mem_available_kb) = meminfo();
    let (disk_total_kb, disk_used_kb) = disk();
    System {
        hostname: read("/proc/sys/kernel/hostname"),
        os: os_name(),
        kernel: read("/proc/sys/kernel/osrelease"),
        uptime_s: read("/proc/uptime")
            .split('.')
            .next()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0),
        load: [
            load.first().copied().unwrap_or(0.0),
            load.get(1).copied().unwrap_or(0.0),
            load.get(2).copied().unwrap_or(0.0),
        ],
        mem_total_kb,
        mem_available_kb,
        temps: temps(),
        disk_total_kb,
        disk_used_kb,
        services: service_states(&units()),
        ifaces: ifaces(),
    }
}

/// One concentrator channel from Concentratord's channel plan.
#[derive(Clone, Debug, PartialEq)]
pub struct Channel {
    pub frequency: u32,
    /// "multi-SF", "LoRa SF7 BW250", "FSK 50 kbit/s"
    pub kind: String,
}

pub const CHANNELS_FILE: &str = "/etc/chirpstack-concentratord/channels.toml";

/// The few keys of channels.toml the panel shows, read line by line (the file is Concentratord's example, a
/// flat list under [gateway.concentrator]).
pub fn parse_channel_plan(toml: &str) -> Vec<Channel> {
    let mut out = Vec::new();
    let mut section = String::new();
    let mut in_multi = false;
    let (mut freq, mut bw, mut sf, mut rate) = (0u32, 0u32, 0u32, 0u32);
    let flush = |section: &str, freq: u32, bw: u32, sf: u32, rate: u32, out: &mut Vec<Channel>| {
        if freq == 0 {
            return;
        }
        let kind = match section {
            "gateway.concentrator.lora_std" => format!("LoRa SF{sf} BW{}", bw / 1000),
            "gateway.concentrator.fsk" => format!("FSK {} kbit/s", rate / 1000),
            _ => return,
        };
        out.push(Channel {
            frequency: freq,
            kind,
        });
    };
    for raw in toml.lines() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            flush(&section, freq, bw, sf, rate, &mut out);
            (freq, bw, sf, rate) = (0, 0, 0, 0);
            section = name.trim().to_string();
            continue;
        }
        if in_multi || line.starts_with("multi_sf_channels") {
            let list = line.split_once('=').map(|(_, v)| v).unwrap_or(line);
            for n in list
                .split(|c: char| !c.is_ascii_digit())
                .filter_map(|n| n.parse().ok())
            {
                out.push(Channel {
                    frequency: n,
                    kind: "multi-SF".into(),
                });
            }
            in_multi = !line.contains(']');
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            let v: u32 = v.trim().parse().unwrap_or(0);
            match k.trim() {
                "frequency" => freq = v,
                "bandwidth" => bw = v,
                "spreading_factor" => sf = v,
                "datarate" => rate = v,
                _ => {}
            }
        }
    }
    flush(&section, freq, bw, sf, rate, &mut out);
    out.sort_by_key(|c| c.frequency);
    out
}

pub fn channel_plan() -> Vec<Channel> {
    parse_channel_plan(&fs::read_to_string(CHANNELS_FILE).unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eu868_plan() {
        let toml = "[gateway.concentrator]\n  multi_sf_channels = [\n    868100000,\n    868300000, # c\n    867100000,\n  ]\n\
                    [gateway.concentrator.lora_std]\n  frequency = 868300000\n  bandwidth = 250000\n  spreading_factor = 7\n\
                    [gateway.concentrator.fsk]\n  frequency = 868800000\n  bandwidth = 125000\n  datarate = 50000\n";
        let p = parse_channel_plan(toml);
        let got: Vec<_> = p.iter().map(|c| (c.frequency, c.kind.as_str())).collect();
        assert_eq!(
            got,
            vec![
                (867_100_000, "multi-SF"),
                (868_100_000, "multi-SF"),
                (868_300_000, "multi-SF"),
                (868_300_000, "LoRa SF7 BW250"),
                (868_800_000, "FSK 50 kbit/s"),
            ]
        );
    }
}
