// SPDX-License-Identifier: MIT
//! BLE sensors to MQTT (docs/ble.md): `lorawan-agent ble` (lorawan-ble.service, switched on from the panel) listens
//! to Bluetooth LE advertisements on the board's own Bluetooth (hci0) and publishes what common sensors send in the
//! open: BTHome v2 (unencrypted), the pvvx and ATC1441 custom firmwares of Xiaomi thermometers, Xiaomi's MiBeacon
//! (older, unencrypted devices) and RuuviTag (data format 5). Readings go to the local broker as
//! `lorawan/ble/<MAC>` (JSON), with Home Assistant discovery entries that the Home Assistant bridge passes on.
//! Phones, headphones and other advertisers are ignored. Scanning is passive: the gateway sends nothing over the air.
//!
//! The radio is driven through a raw HCI socket (the kernel's Bluetooth socket API), so there is no dependency on
//! BlueZ's D-Bus interface; bluetoothd may run alongside.

use crate::bridge;
use crate::mqtt::Publisher;
use crate::radio::now;
use crate::status::json_str;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::io;
use std::thread;
use std::time::{Duration, Instant};

pub const DIR: &str = "/var/lib/lorawan-ble";
pub const ENABLED: &str = "/var/lib/lorawan-ble/enabled";
pub const SENSORS: &str = "/run/lorawan-ble/sensors.json";
pub const TOPIC: &str = "lorawan/ble";
const UNIT: &str = "lorawan-ble.service";

// --- Decoding ------------------------------------------------------------------------------------------------

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Reading {
    pub kind: &'static str,
    pub values: Vec<(&'static str, f64)>,
}

/// Unit, Home Assistant device class and number of decimals of each value name.
pub fn describe(name: &str) -> (&'static str, &'static str, usize) {
    match name {
        "temperature" | "dewpoint" => ("°C", "temperature", 2),
        "humidity" | "moisture" => (
            "%",
            if name == "humidity" {
                "humidity"
            } else {
                "moisture"
            },
            2,
        ),
        "battery" => ("%", "battery", 0),
        "voltage" => ("V", "voltage", 3),
        "pressure" => ("hPa", "atmospheric_pressure", 2),
        "illuminance" => ("lx", "illuminance", 2),
        "co2" => ("ppm", "carbon_dioxide", 0),
        "tvoc" => ("µg/m³", "volatile_organic_compounds", 0),
        "pm25" => ("µg/m³", "pm25", 0),
        "pm10" => ("µg/m³", "pm10", 0),
        "power" => ("W", "power", 2),
        "energy" => ("kWh", "energy", 3),
        "current" => ("A", "current", 3),
        "distance" => ("mm", "distance", 0),
        "uv_index" => ("", "", 1),
        "count" => ("", "", 0),
        _ => ("", "", 0),
    }
}

fn u16le(b: &[u8], i: usize) -> Option<u16> {
    Some(u16::from_le_bytes([*b.get(i)?, *b.get(i + 1)?]))
}
fn u16be(b: &[u8], i: usize) -> Option<u16> {
    Some(u16::from_be_bytes([*b.get(i)?, *b.get(i + 1)?]))
}

/// BTHome v2 service data (after the 0xFCD2 UUID).
pub fn bthome(d: &[u8]) -> Option<Reading> {
    let info = *d.first()?;
    if info & 1 != 0 || info >> 5 != 2 {
        return None; // encrypted, or not version 2
    }
    // (object id, size, factor, signed, name); unknown ids end the parse (their size is not known)
    const T: &[(u8, usize, f64, bool, &str)] = &[
        (0x00, 1, 1.0, false, ""),
        (0x01, 1, 1.0, false, "battery"),
        (0x02, 2, 0.01, true, "temperature"),
        (0x03, 2, 0.01, false, "humidity"),
        (0x04, 3, 0.01, false, "pressure"),
        (0x05, 3, 0.01, false, "illuminance"),
        (0x08, 2, 0.01, true, "dewpoint"),
        (0x09, 1, 1.0, false, "count"),
        (0x0A, 3, 0.001, false, "energy"),
        (0x0B, 3, 0.01, false, "power"),
        (0x0C, 2, 0.001, false, "voltage"),
        (0x0D, 2, 1.0, false, "pm25"),
        (0x0E, 2, 1.0, false, "pm10"),
        (0x12, 2, 1.0, false, "co2"),
        (0x13, 2, 1.0, false, "tvoc"),
        (0x14, 2, 0.01, false, "moisture"),
        (0x2E, 1, 1.0, false, "humidity"),
        (0x2F, 1, 1.0, false, "moisture"),
        (0x3A, 1, 1.0, false, ""),
        (0x3D, 2, 1.0, false, "count"),
        (0x3E, 4, 1.0, false, "count"),
        (0x40, 2, 1.0, false, "distance"),
        (0x43, 2, 0.001, false, "current"),
        (0x45, 2, 0.1, true, "temperature"),
        (0x46, 1, 0.1, false, "uv_index"),
        (0x4A, 2, 0.1, false, "voltage"),
        (0xF0, 2, 1.0, false, ""),
        (0xF1, 4, 1.0, false, ""),
        (0xF2, 3, 1.0, false, ""),
    ];
    let mut r = Reading {
        kind: "BTHome",
        values: Vec::new(),
    };
    let mut i = 1;
    while i < d.len() {
        let id = d[i];
        // Binary sensors 0x0F..0x2D (one byte) other than the numeric 0x12..0x14
        let (size, factor, signed, name) = match T.iter().find(|t| t.0 == id) {
            Some(&(_, s, f, sg, n)) => (s, f, sg, n),
            None if (0x0F..=0x2D).contains(&id) => (1, 1.0, false, ""),
            None => break,
        };
        let raw = d.get(i + 1..i + 1 + size)?;
        let mut v: i64 = 0;
        for (k, b) in raw.iter().enumerate() {
            v |= i64::from(*b) << (8 * k);
        }
        if signed && raw.last().is_some_and(|b| b & 0x80 != 0) {
            v -= 1 << (8 * size);
        }
        if !name.is_empty() && !r.values.iter().any(|(n, _)| *n == name) {
            r.values.push((name, v as f64 * factor));
        }
        i += 1 + size;
    }
    (!r.values.is_empty()).then_some(r)
}

/// Service data 0x181A: the pvvx firmware's custom format (15 bytes) or ATC1441's (13 bytes).
pub fn env_sensing(d: &[u8]) -> Option<Reading> {
    match d.len() {
        15 => Some(Reading {
            kind: "pvvx",
            values: vec![
                ("temperature", f64::from(u16le(d, 6)? as i16) / 100.0),
                ("humidity", f64::from(u16le(d, 8)?) / 100.0),
                ("voltage", f64::from(u16le(d, 10)?) / 1000.0),
                ("battery", f64::from(d[12])),
            ],
        }),
        13 => Some(Reading {
            kind: "ATC",
            values: vec![
                ("temperature", f64::from(u16be(d, 6)? as i16) / 10.0),
                ("humidity", f64::from(d[8])),
                ("battery", f64::from(d[9])),
                ("voltage", f64::from(u16be(d, 10)?) / 1000.0),
            ],
        }),
        _ => None,
    }
}

/// Xiaomi MiBeacon (service data 0xFE95), unencrypted objects only.
pub fn mibeacon(d: &[u8]) -> Option<Reading> {
    let fc = u16le(d, 0)?;
    if fc & 0x0008 != 0 || fc & 0x0040 == 0 {
        return None; // encrypted (needs the device's bind key), or no object
    }
    let mut i = 5;
    if fc & 0x0010 != 0 {
        i += 6; // MAC
    }
    if fc & 0x0020 != 0 {
        let cap = *d.get(i)?;
        i += 1 + if cap & 0x20 != 0 { 2 } else { 0 };
    }
    let id = u16le(d, i)?;
    let len = usize::from(*d.get(i + 2)?);
    let v = d.get(i + 3..i + 3 + len)?;
    let s16 = |k: usize| u16le(v, k).map(|x| f64::from(x as i16) / 10.0);
    let values = match id {
        0x1004 => vec![("temperature", s16(0)?)],
        0x1006 => vec![("humidity", f64::from(u16le(v, 0)?) / 10.0)],
        0x100A => vec![("battery", f64::from(*v.first()?))],
        0x100D => vec![
            ("temperature", s16(0)?),
            ("humidity", f64::from(u16le(v, 2)?) / 10.0),
        ],
        _ => return None,
    };
    Some(Reading {
        kind: "MiBeacon",
        values,
    })
}

/// RuuviTag manufacturer data (company 0x0499), data format 5.
pub fn ruuvi(d: &[u8]) -> Option<Reading> {
    if d.first() != Some(&5) || d.len() < 24 {
        return None;
    }
    let mut values = Vec::new();
    let t = u16be(d, 1)? as i16;
    if t != i16::MIN {
        values.push(("temperature", f64::from(t) * 0.005));
    }
    let h = u16be(d, 3)?;
    if h != u16::MAX {
        values.push(("humidity", f64::from(h) * 0.0025));
    }
    let p = u16be(d, 5)?;
    if p != u16::MAX {
        values.push(("pressure", (f64::from(p) + 50_000.0) / 100.0));
    }
    let pw = u16be(d, 13)?;
    if pw >> 5 != 0x7FF {
        values.push(("voltage", f64::from((pw >> 5) + 1600) / 1000.0));
    }
    (!values.is_empty()).then_some(Reading {
        kind: "Ruuvi",
        values,
    })
}

/// A sensor reading from one advertisement's data (its AD structures), and the advertised name.
pub fn decode_adv(data: &[u8]) -> (Option<Reading>, String) {
    let mut reading = None;
    let mut name = String::new();
    let mut i = 0;
    while i < data.len() {
        let len = usize::from(data[i]);
        if len == 0 || i + 1 + len > data.len() {
            break;
        }
        let kind = data[i + 1];
        let body = &data[i + 2..i + 1 + len];
        match kind {
            0x08 | 0x09 => {
                name = String::from_utf8_lossy(body)
                    .chars()
                    .filter(|c| !c.is_control())
                    .collect()
            }
            0x16 if body.len() >= 2 && reading.is_none() => {
                let rest = &body[2..];
                reading = match u16le(body, 0) {
                    Some(0xFCD2) => bthome(rest),
                    Some(0x181A) => env_sensing(rest),
                    Some(0xFE95) => mibeacon(rest),
                    _ => None,
                };
            }
            0xFF if body.len() >= 2 && reading.is_none() && u16le(body, 0) == Some(0x0499) => {
                reading = ruuvi(&body[2..]);
            }
            _ => {}
        }
        i += 1 + len;
    }
    (reading, name)
}

/// The advertising reports of one HCI LE Meta event (after the event code and length): (address, data, RSSI).
/// Fields come grouped per field for all reports (Bluetooth Core, LE Advertising Report event).
pub fn parse_reports(p: &[u8]) -> Vec<([u8; 6], Vec<u8>, i8)> {
    let mut out = Vec::new();
    if p.first() != Some(&0x02) {
        return out;
    }
    let Some(&n) = p.get(1) else { return out };
    let n = usize::from(n);
    let base = 2;
    let addrs = base + 2 * n;
    let lens = addrs + 6 * n;
    let mut data = lens + n;
    let Some(ls) = p.get(lens..lens + n) else {
        return out;
    };
    let total: usize = ls.iter().map(|&l| usize::from(l)).sum();
    let rssis = data + total;
    if p.len() < rssis + n {
        return out;
    }
    for (k, &l) in ls.iter().enumerate() {
        let mut a = [0u8; 6];
        a.copy_from_slice(&p[addrs + 6 * k..addrs + 6 * k + 6]);
        a.reverse(); // little-endian on the wire
        let l = usize::from(l);
        out.push((a, p[data..data + l].to_vec(), p[rssis + k] as i8));
        data += l;
    }
    out
}

pub fn mac_str(a: &[u8; 6]) -> String {
    a.iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(":")
}

// --- Raw HCI socket ------------------------------------------------------------------------------------------

mod hci {
    use std::fs::File;
    use std::io::{self, Read, Write};
    use std::os::fd::{FromRawFd, OwnedFd};

    const AF_BLUETOOTH: i32 = 31;
    const SOCK_RAW: i32 = 3;
    const SOCK_CLOEXEC: i32 = 0o2_000_000;
    const BTPROTO_HCI: i32 = 1;
    const SOL_HCI: i32 = 0;
    const HCI_FILTER: i32 = 2;
    const HCIDEVUP: u64 = 0x4004_48c9;

    extern "C" {
        fn socket(domain: i32, ty: i32, protocol: i32) -> i32;
        fn bind(fd: i32, addr: *const u8, len: u32) -> i32;
        fn setsockopt(fd: i32, level: i32, name: i32, val: *const u8, len: u32) -> i32;
        fn ioctl(fd: i32, req: u64, ...) -> i32;
    }

    pub struct Hci(File);

    fn check(r: i32) -> io::Result<i32> {
        if r < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(r)
        }
    }

    impl Hci {
        /// hci<dev>, brought up if it is down, filtered to LE Meta and command answers.
        pub fn open(dev: u16) -> io::Result<Hci> {
            // SAFETY: plain libc calls on a descriptor this function owns; buffers outlive the calls.
            unsafe {
                let fd = check(socket(AF_BLUETOOTH, SOCK_RAW | SOCK_CLOEXEC, BTPROTO_HCI))?;
                let h = Hci(File::from(OwnedFd::from_raw_fd(fd)));
                if ioctl(fd, HCIDEVUP, i32::from(dev)) < 0 {
                    let e = io::Error::last_os_error();
                    if e.raw_os_error() != Some(114) {
                        return Err(e); // EALREADY: up already
                    }
                }
                let mut addr = [0u8; 6];
                addr[0..2].copy_from_slice(&(AF_BLUETOOTH as u16).to_ne_bytes());
                addr[2..4].copy_from_slice(&dev.to_ne_bytes());
                check(bind(fd, addr.as_ptr(), 6))?;
                // struct hci_filter: type mask (events), event mask (2 x 32 bit), opcode
                let mut f = [0u8; 16];
                f[0..4].copy_from_slice(&(1u32 << 4).to_ne_bytes());
                f[4..8].copy_from_slice(&((1u32 << 0x0E) | (1u32 << 0x0F)).to_ne_bytes());
                f[8..12].copy_from_slice(&(1u32 << (0x3E - 32)).to_ne_bytes());
                check(setsockopt(fd, SOL_HCI, HCI_FILTER, f.as_ptr(), 16))?;
                Ok(h)
            }
        }

        pub fn command(&self, ogf: u16, ocf: u16, params: &[u8]) -> io::Result<()> {
            let op = (ogf << 10) | ocf;
            let mut p = vec![0x01, op as u8, (op >> 8) as u8, params.len() as u8];
            p.extend_from_slice(params);
            // One packet per write: a raw HCI socket takes whole packets
            (&self.0).write_all(&p)
        }

        pub fn read(&self, buf: &mut [u8]) -> io::Result<usize> {
            (&self.0).read(buf)
        }
    }
}

fn start_scan(h: &hci::Hci) -> io::Result<()> {
    h.command(0x08, 0x000C, &[0, 0])?; // scanning off (in case it was on with other parameters)
    thread::sleep(Duration::from_millis(100));
    // Passive, interval 100 ms, window 50 ms (units of 0.625 ms), public address, accept all
    h.command(0x08, 0x000B, &[0x00, 0xA0, 0x00, 0x50, 0x00, 0x00, 0x00])?;
    thread::sleep(Duration::from_millis(100));
    h.command(0x08, 0x000C, &[1, 0]) // on, no duplicate filter (each reading counts)
}

// --- Publishing ----------------------------------------------------------------------------------------------

#[derive(Clone, Debug, Default)]
pub struct Sensor {
    pub mac: String,
    pub name: String,
    pub reading: Reading,
    pub rssi: i8,
    pub seen: u64,
    pub published: Option<Instant>,
    pub announced: Option<String>,
}

pub fn state_json(s: &Sensor) -> String {
    let mut o = String::from("{\"mac\":");
    json_str(&mut o, &s.mac);
    o.push_str(",\"type\":");
    json_str(&mut o, s.reading.kind);
    o.push_str(",\"name\":");
    json_str(&mut o, &s.name);
    let _ = write!(o, ",\"rssi\":{},\"seen\":{}", s.rssi, s.seen);
    for (k, v) in &s.reading.values {
        let _ = write!(o, ",\"{k}\":{:.*}", describe(k).2, v);
    }
    o.push('}');
    o
}

fn key(mac: &str) -> String {
    mac.replace(':', "").to_ascii_lowercase()
}

/// Home Assistant discovery entries (topic, config) for each value of a sensor.
pub fn discovery(s: &Sensor, prefix: &str) -> Vec<(String, String)> {
    let k = key(&s.mac);
    let label = if s.name.is_empty() {
        format!("{} {}", s.reading.kind, &s.mac[9..])
    } else {
        s.name.clone()
    };
    s.reading
        .values
        .iter()
        .map(|(v, _)| {
            let (unit, class, _) = describe(v);
            let mut o = String::from("{\"name\":");
            json_str(&mut o, &v.replace('_', " "));
            o.push_str(",\"unique_id\":");
            json_str(&mut o, &format!("ble_{k}_{v}"));
            o.push_str(",\"state_topic\":");
            json_str(&mut o, &format!("{prefix}{TOPIC}/{k}"));
            let _ = write!(
                o,
                ",\"value_template\":\"{{{{ value_json.{v} }}}}\",\"state_class\":\"measurement\""
            );
            if !unit.is_empty() {
                o.push_str(",\"unit_of_measurement\":");
                json_str(&mut o, unit);
            }
            if !class.is_empty() {
                o.push_str(",\"device_class\":");
                json_str(&mut o, class);
            }
            o.push_str(",\"device\":{\"identifiers\":[");
            json_str(&mut o, &format!("ble_{k}"));
            o.push_str("],\"connections\":[[\"bluetooth\",");
            json_str(&mut o, &s.mac);
            o.push_str("]],\"name\":");
            json_str(&mut o, &label);
            o.push_str(",\"model\":");
            json_str(&mut o, s.reading.kind);
            o.push_str("}}");
            (format!("homeassistant/sensor/ble_{k}/{v}/config"), o)
        })
        .collect()
}

fn write_sensors(list: &BTreeMap<String, Sensor>) {
    let mut o = format!("{{\"now\":{},\"sensors\":[", now());
    for (i, s) in list.values().enumerate() {
        if i > 0 {
            o.push(',');
        }
        o.push_str(&state_json(s));
    }
    o.push_str("]}");
    let tmp = format!("{SENSORS}.tmp");
    if fs::write(&tmp, o).is_ok() {
        let _ = fs::rename(&tmp, SENSORS);
    }
}

/// `lorawan-agent ble`: scans and publishes until stopped.
pub fn run(broker: &str) -> Result<(), String> {
    let h = hci::Hci::open(0).map_err(|e| format!("hci0: {e}"))?;
    start_scan(&h).map_err(|e| format!("starting the scan: {e}"))?;
    eprintln!("lorawan-ble: scanning on hci0");
    let mut publisher = Publisher::new(broker, &format!("lorawan-ble-{}", std::process::id()));
    let mut sensors: BTreeMap<String, Sensor> = BTreeMap::new();
    let mut last_write = Instant::now();
    let mut last_rescan = Instant::now();
    let mut buf = [0u8; 300];
    loop {
        let n = h.read(&mut buf).map_err(|e| format!("reading hci0: {e}"))?;
        // Some controllers stop scanning on their own after a while; start it again every 10 minutes.
        if last_rescan.elapsed() > Duration::from_secs(600) {
            let _ = start_scan(&h);
            last_rescan = Instant::now();
        }
        if n < 3 || buf[0] != 0x04 || buf[1] != 0x3E {
            continue;
        }
        for (addr, data, rssi) in parse_reports(&buf[3..n]) {
            let (reading, name) = decode_adv(&data);
            let Some(reading) = reading else { continue };
            let mac = mac_str(&addr);
            let prefix = bridge::configured().map(|c| c.prefix);
            let s = sensors.entry(mac.clone()).or_insert_with(|| Sensor {
                mac: mac.clone(),
                ..Default::default()
            });
            if !name.is_empty() {
                s.name = name;
            }
            let changed = s.reading != reading;
            s.reading = reading;
            s.rssi = rssi;
            s.seen = now();
            // At most every 10 s per sensor; at least once a minute while it is heard
            let due = s.published.is_none_or(|t| {
                let e = t.elapsed();
                (changed && e > Duration::from_secs(10)) || e > Duration::from_secs(60)
            });
            if !due {
                continue;
            }
            if let Some(p) = &prefix {
                let sig = format!(
                    "{p}|{}",
                    s.reading
                        .values
                        .iter()
                        .map(|(k, _)| *k)
                        .collect::<Vec<_>>()
                        .join(",")
                );
                if s.announced.as_deref() != Some(&sig) {
                    for (t, cfg) in discovery(s, p) {
                        let _ = publisher.publish(&t, cfg.as_bytes(), true);
                    }
                    s.announced = Some(sig);
                }
            }
            if let Err(e) = publisher.publish(
                &format!("{TOPIC}/{}", key(&mac)),
                state_json(s).as_bytes(),
                false,
            ) {
                eprintln!("lorawan-ble: publishing: {e}");
            }
            s.published = Some(Instant::now());
        }
        if last_write.elapsed() > Duration::from_secs(5) {
            // Forget sensors not heard for a day
            let cut = now().saturating_sub(86_400);
            sensors.retain(|_, s| s.seen > cut);
            write_sensors(&sensors);
            publisher.idle();
            last_write = Instant::now();
        }
    }
}

// --- Panel ---------------------------------------------------------------------------------------------------

pub fn available() -> bool {
    std::path::Path::new("/sys/class/bluetooth/hci0").exists()
}

pub fn info_json() -> String {
    let enabled = std::path::Path::new(ENABLED).exists();
    let running = std::process::Command::new("systemctl")
        .args(["is-active", "--quiet", UNIT])
        .status()
        .is_ok_and(|s| s.success());
    let sensors = if running {
        fs::read_to_string(SENSORS).unwrap_or_default()
    } else {
        String::new()
    };
    format!(
        "{{\"available\":{},\"enabled\":{enabled},\"running\":{running},\"data\":{}}}",
        available(),
        if sensors.starts_with('{') {
            sensors.as_str()
        } else {
            "null"
        }
    )
}

/// `enabled=on` or not: switches the scanner on or off (it stays so across restarts).
pub fn save(body: &[u8]) -> crate::http::Response {
    use crate::http::Response;
    if !available() {
        return Response::new(404, "text/plain", "no Bluetooth on this gateway\n");
    }
    let on = bridge::form_fields(body)
        .iter()
        .any(|(k, v)| k == "enabled" && v == "on");
    let r = if on {
        fs::create_dir_all(DIR).and_then(|_| fs::write(ENABLED, b""))
    } else {
        fs::remove_file(ENABLED).or_else(|e| {
            if e.kind() == io::ErrorKind::NotFound {
                Ok(())
            } else {
                Err(e)
            }
        })
    };
    if let Err(e) = r {
        return Response::new(500, "text/plain", format!("saving: {e}\n"));
    }
    let ok = std::process::Command::new("systemctl")
        .args([if on { "restart" } else { "stop" }, UNIT])
        .status()
        .is_ok_and(|s| s.success());
    match (on, ok) {
        (true, true) => Response::new(200, "text/plain", "Scanning for BLE sensors.\n"),
        (false, _) => Response::new(200, "text/plain", "BLE sensors off.\n"),
        (true, false) => Response::new(
            500,
            "text/plain",
            "could not start lorawan-ble (journalctl -u lorawan-ble)\n",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn val(r: &Reading, k: &str) -> f64 {
        r.values
            .iter()
            .find(|(n, _)| *n == k)
            .map(|x| x.1)
            .unwrap_or(f64::NAN)
    }

    #[test]
    fn bthome_v2() {
        // From the BTHome format page: packet id 9, temperature 23.45 °C, humidity 50.55 %, battery 97 %
        let r = bthome(&[
            0x40, 0x00, 0x09, 0x02, 0x29, 0x09, 0x03, 0xBF, 0x13, 0x01, 0x61,
        ])
        .unwrap();
        assert!((val(&r, "temperature") - 23.45).abs() < 1e-9);
        assert!((val(&r, "humidity") - 50.55).abs() < 1e-9);
        assert_eq!(val(&r, "battery"), 97.0);
        // Negative temperature, a binary sensor in between, then an unknown id ends the parse
        let r = bthome(&[
            0x40, 0x45, 0x9C, 0xFF, 0x21, 0x01, 0x0C, 0xB8, 0x0B, 0x77, 0x01,
        ])
        .unwrap();
        assert!((val(&r, "temperature") + 10.0).abs() < 1e-9);
        assert!((val(&r, "voltage") - 3.0).abs() < 1e-9);
        assert_eq!(r.values.len(), 2);
        assert!(bthome(&[0x41, 0x02, 0x29, 0x09]).is_none(), "encrypted");
        assert!(bthome(&[0x40, 0x02, 0x29]).is_none(), "cut off");
    }

    #[test]
    fn xiaomi_custom_firmwares() {
        // pvvx: MAC (reversed), 21.53 °C, 45.12 %, 2958 mV, 87 %, count, flags
        let r = env_sensing(&[
            0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x69, 0x08, 0xA0, 0x11, 0x8E, 0x0B, 87, 3, 0,
        ])
        .unwrap();
        assert_eq!(r.kind, "pvvx");
        assert!((val(&r, "temperature") - 21.53).abs() < 1e-9);
        assert!((val(&r, "humidity") - 45.12).abs() < 1e-9);
        assert!((val(&r, "voltage") - 2.958).abs() < 1e-9);
        // ATC1441: MAC, -1.5 °C, 60 %, 80 %, 2900 mV, count
        let r =
            env_sensing(&[0xA4, 0xC1, 0x38, 1, 2, 3, 0xFF, 0xF1, 60, 80, 0x0B, 0x54, 9]).unwrap();
        assert_eq!(r.kind, "ATC");
        assert!((val(&r, "temperature") + 1.5).abs() < 1e-9);
        assert_eq!(val(&r, "battery"), 80.0);
    }

    #[test]
    fn mibeacon_plain_and_encrypted() {
        // fc 0x5050 (object, MAC), product, counter, MAC, object 0x100D: 23.4 °C, 55.0 %
        let mut d = vec![
            0x50, 0x50, 0xAA, 0x01, 0x07, 1, 2, 3, 4, 5, 6, 0x0D, 0x10, 0x04, 0xEA, 0x00, 0x26,
            0x02,
        ];
        let r = mibeacon(&d).unwrap();
        assert!((val(&r, "temperature") - 23.4).abs() < 1e-9);
        assert!((val(&r, "humidity") - 55.0).abs() < 1e-9);
        d[0] |= 0x08;
        assert!(mibeacon(&d).is_none());
    }

    #[test]
    fn ruuvi_format_5() {
        // The format's own test vector ("valid data")
        let d = [
            0x05, 0x12, 0xFC, 0x53, 0x94, 0xC3, 0x7C, 0x00, 0x04, 0xFF, 0xFC, 0x04, 0x0C, 0xAC,
            0x36, 0x42, 0x00, 0xCD, 0xCB, 0xB8, 0x33, 0x4C, 0x88, 0x4F,
        ];
        let r = ruuvi(&d).unwrap();
        assert!((val(&r, "temperature") - 24.3).abs() < 1e-9);
        assert!((val(&r, "humidity") - 53.49).abs() < 1e-9);
        assert!((val(&r, "pressure") - 1000.44).abs() < 1e-9);
        assert!((val(&r, "voltage") - 2.977).abs() < 1e-9);
    }

    #[test]
    fn advertisements_and_reports() {
        // Flags, name "ATC_123", service data 0x181A with an ATC payload
        let mut adv = vec![0x02, 0x01, 0x06, 0x08, 0x09];
        adv.extend_from_slice(b"ATC_123");
        adv.extend_from_slice(&[
            0x10, 0x16, 0x1A, 0x18, 0xA4, 0xC1, 0x38, 1, 2, 3, 0x00, 0xD7, 50, 90, 0x0B, 0xB8, 1,
        ]);
        let (r, name) = decode_adv(&adv);
        assert_eq!(name, "ATC_123");
        assert_eq!(r.unwrap().kind, "ATC");
        assert_eq!(
            decode_adv(&[0x02, 0x01, 0x06, 0x03, 0xFF, 0x4C, 0x00]).0,
            None,
            "a phone"
        );
        // One report: event type, address type, address (LE), length, data, RSSI
        let mut ev = vec![0x02, 1, 0x00, 0x00, 6, 5, 4, 3, 2, 1, adv.len() as u8];
        ev.extend_from_slice(&adv);
        ev.push(0xC4);
        let reps = parse_reports(&ev);
        assert_eq!(reps.len(), 1);
        assert_eq!(mac_str(&reps[0].0), "01:02:03:04:05:06");
        assert_eq!(reps[0].2, -60);
        assert_eq!(reps[0].1, adv);
        assert!(parse_reports(&ev[..ev.len() - 2]).is_empty(), "cut off");
    }

    #[test]
    fn state_and_discovery() {
        let s = Sensor {
            mac: "A4:C1:38:01:02:03".into(),
            reading: Reading {
                kind: "pvvx",
                values: vec![("temperature", 21.534), ("battery", 87.0)],
            },
            rssi: -70,
            seen: 5,
            ..Default::default()
        };
        let j = state_json(&s);
        assert!(
            j.contains("\"temperature\":21.53,") && j.contains("\"battery\":87}"),
            "{j}"
        );
        assert!(crate::json::parse(j.as_bytes()).is_ok());
        let d = discovery(&s, "chirpstack/");
        assert_eq!(d.len(), 2);
        assert_eq!(
            d[0].0,
            "homeassistant/sensor/ble_a4c138010203/temperature/config"
        );
        let v = crate::json::parse(d[0].1.as_bytes()).unwrap();
        assert_eq!(
            v.get("state_topic").str(),
            Some("chirpstack/lorawan/ble/a4c138010203")
        );
        assert_eq!(
            v.get("value_template").str(),
            Some("{{ value_json.temperature }}")
        );
        assert_eq!(v.get("unit_of_measurement").str(), Some("°C"));
        assert_eq!(v.at(&["device", "name"]).str(), Some("pvvx 01:02:03"));
    }
}
