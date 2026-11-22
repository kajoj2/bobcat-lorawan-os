// SPDX-License-Identifier: MIT
//! The setup wizard (`/setup`): a freshly flashed gateway opens a setup access point, a phone joins it, the
//! wizard sets Wi-Fi, the community networks and, where the board can, the install onto the eMMC.
//!
//! The privileged steps are systemd units around one script (`lorawan-setup`, recipe lorawan-setup):
//! `lorawan-setup-ap.service` runs hostapd with an open network and a DHCP server on wlan0 (192.168.4.1), and
//! `lorawan-setup-wifi.service` installs the Wi-Fi configuration the agent prepared, joins the network and goes
//! back to the access point when that fails. The agent decides when the access point starts (no network two
//! minutes after boot, or the board's button held for 5 s), answers every DNS question on the setup network with
//! its own address (so phones open the wizard as a captive portal) and serves the page.

use crate::forward::percent_decode;
use crate::http::Response;
use crate::status::json_str;
use crate::sys;
use std::fmt::Write as _;
use std::fs::{self, File};
use std::io::{self, Read};
use std::net::{IpAddr, Ipv4Addr, UdpSocket};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

pub const IFACE: &str = "wlan0";
pub const AP_ADDR: Ipv4Addr = Ipv4Addr::new(192, 168, 4, 1);
const AP_UNIT: &str = "lorawan-setup-ap.service";
const WIFI_UNIT: &str = "lorawan-setup-wifi.service";
/// Written by the script while the access point runs (and gone when it stops).
const RUN_DIR: &str = "/run/lorawan-setup";
/// The agent's state directory (StateDirectory=lorawan-setup); the script reads the request from here.
const STATE_DIR: &str = "/var/lib/lorawan-setup";
const WPA_CONF: &str = sys::WIFI_CONFIG;
/// No network for this long (and the system up at least this long) starts the access point.
const OFFLINE_START: u64 = 120;
const BUTTON_HOLD: Duration = Duration::from_secs(5);
/// Held this long: factory reset (lorawan-setup-reset.service).
const BUTTON_RESET: Duration = Duration::from_secs(15);
const RESET_UNIT: &str = "lorawan-setup-reset.service";

fn read(path: impl AsRef<Path>) -> String {
    fs::read_to_string(path)
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

pub fn wifi_present() -> bool {
    Path::new(&format!("/sys/class/net/{IFACE}/wireless")).exists()
}

/// Whether hostapd's configuration exists, i.e. the access point is up or coming up. No process, cheap.
pub fn ap_active() -> bool {
    Path::new(&format!("{RUN_DIR}/hostapd.conf")).exists()
}

pub fn done() -> bool {
    Path::new(&format!("{STATE_DIR}/done")).exists()
}

/// A Wi-Fi network is configured (by the wizard, by hand, or kept from an earlier system by an update).
pub fn wifi_configured() -> bool {
    Path::new(WPA_CONF).exists()
}

/// Requests from phones on the setup network.
pub fn via_ap(peer: Option<IpAddr>) -> bool {
    match peer.map(|p| p.to_canonical()) {
        Some(IpAddr::V4(a)) => a != AP_ADDR && a.octets()[..3] == AP_ADDR.octets()[..3],
        _ => false,
    }
}

/// Requests on the setup network for any other host name are a phone's captive portal check (or a page it
/// opened before joining): send them to the wizard.
pub fn captive_redirect(peer: Option<IpAddr>, host: Option<&str>) -> Option<Response> {
    if !via_ap(peer) || !ap_active() {
        return None;
    }
    let host = host.unwrap_or("").to_ascii_lowercase();
    let host = host.rsplit_once(':').map_or(host.as_str(), |(h, p)| {
        if p.bytes().all(|b| b.is_ascii_digit()) {
            h
        } else {
            host.as_str()
        }
    });
    let name = read("/proc/sys/kernel/hostname").to_ascii_lowercase();
    let ours = [AP_ADDR.to_string(), name.clone(), format!("{name}.local")];
    if ours.iter().any(|o| o == host) {
        return None;
    }
    Some(
        Response::new(
            302,
            "text/html",
            "<a href=\"http://192.168.4.1/setup\">Setup</a>\n",
        )
        .with_header("Location", "http://192.168.4.1/setup"),
    )
}

fn unit_active(unit: &str) -> bool {
    Command::new("systemctl")
        .args(["is-active", "--quiet", unit])
        .status()
        .is_ok_and(|s| s.success())
}

fn systemctl(args: &[&str]) -> Result<(), String> {
    match Command::new("systemctl").args(args).status() {
        Ok(s) if s.success() => Ok(()),
        Ok(s) => Err(format!("systemctl {}: {s}", args.join(" "))),
        Err(e) => Err(format!("systemctl: {e}")),
    }
}

pub fn start_ap() -> Result<(), String> {
    systemctl(&["start", "--no-block", AP_UNIT])
}

// --- Wi-Fi configuration --------------------------------------------------------------------------------------

/// SSID: 1..32 bytes; written in hex, so any byte would do, but control characters are refused (they are
/// never meant, and the panel shows the name).
pub fn check_ssid(ssid: &str) -> Result<(), String> {
    if ssid.is_empty() || ssid.len() > 32 {
        return Err("the network name must be 1 to 32 bytes".into());
    }
    if ssid.chars().any(char::is_control) {
        return Err("the network name has a control character".into());
    }
    Ok(())
}

/// WPA passphrase: 8..63 printable ASCII characters (IEEE 802.11i), or empty for an open network.
pub fn check_psk(psk: &str) -> Result<(), String> {
    if psk.is_empty() {
        return Ok(());
    }
    if !(8..=63).contains(&psk.len()) || !psk.bytes().all(|b| (0x20..0x7f).contains(&b)) {
        return Err(
            "the password must be 8 to 63 characters (letters, digits, punctuation, spaces)".into(),
        );
    }
    Ok(())
}

fn hex(s: &str) -> String {
    s.bytes().map(|b| format!("{b:02x}")).collect()
}

/// wpa_supplicant's configuration for one network. The SSID is written in hex; the passphrase quoted
/// (wpa_supplicant takes everything up to the last quote, so quotes inside are fine; no newlines get here).
pub fn render_wpa(ssid: &str, psk: &str) -> String {
    let mut o = String::from(
        "# Written by the setup wizard (lorawan-agent, http://<gateway>/setup). See docs/setup.md.\n\
         ctrl_interface=/var/run/wpa_supplicant\n\
         network={\n",
    );
    let _ = writeln!(o, "\tssid={}", hex(ssid));
    // Hidden networks are found too.
    o.push_str("\tscan_ssid=1\n");
    if psk.is_empty() {
        o.push_str("\tkey_mgmt=NONE\n");
    } else {
        let _ = writeln!(o, "\tpsk=\"{psk}\"");
    }
    o.push_str("}\n");
    o
}

/// The configured network's name, from `ssid="..."` or `ssid=<hex>`.
pub fn parse_wpa_ssid(conf: &str) -> Option<String> {
    let v = conf
        .lines()
        .map(str::trim)
        .find_map(|l| l.strip_prefix("ssid="))?;
    if let Some(q) = v.strip_prefix('"') {
        return Some(q.rsplit_once('"').map_or(q, |(s, _)| s).to_string());
    }
    let bytes: Option<Vec<u8>> = (0..v.len() / 2)
        .map(|i| u8::from_str_radix(v.get(2 * i..2 * i + 2)?, 16).ok())
        .collect();
    Some(String::from_utf8_lossy(&bytes?).into_owned())
}

fn form_value(form: &str, key: &str) -> Option<String> {
    form.split('&').find_map(|kv| {
        let (k, v) = kv.split_once('=')?;
        (k == key).then(|| percent_decode(v))
    })
}

/// `ssid=...&psk=...`: writes the configuration for the script and starts the join (which takes the access
/// point down a few seconds later, after this answer has reached the phone).
pub fn save_wifi(body: &[u8]) -> Response {
    let form = String::from_utf8_lossy(body);
    let ssid = form_value(&form, "ssid").unwrap_or_default();
    let psk = form_value(&form, "psk").unwrap_or_default();
    if let Err(e) = check_ssid(&ssid).and_then(|_| check_psk(&psk)) {
        return Response::new(400, "text/plain", format!("{e}\n"));
    }
    if !wifi_present() {
        return Response::new(404, "text/plain", "this gateway has no Wi-Fi\n");
    }
    if unit_active(WIFI_UNIT) {
        return Response::new(
            409,
            "text/plain",
            "the gateway is already joining a network\n",
        );
    }
    let path = format!("{STATE_DIR}/wifi.conf.new");
    let written = fs::create_dir_all(STATE_DIR)
        .and_then(|_| {
            let _ = fs::remove_file(&path);
            fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&path)
        })
        .and_then(|mut f| io::Write::write_all(&mut f, render_wpa(&ssid, &psk).as_bytes()))
        .and_then(|_| fs::write(format!("{STATE_DIR}/wifi-ssid"), &ssid))
        .and_then(|_| {
            fs::write(
                format!("{STATE_DIR}/wifi-result"),
                "state=queued\nmessage=\naddress=\n",
            )
        });
    if let Err(e) = written {
        return Response::new(500, "text/plain", format!("{path}: {e}\n"));
    }
    match systemctl(&["start", "--no-block", WIFI_UNIT]) {
        Ok(()) => Response::new(202, "text/plain", "joining\n"),
        Err(e) => Response::new(500, "text/plain", format!("{e}\n")),
    }
}

/// The wizard is finished: remembered, and the access point goes away.
pub fn finish() -> Response {
    if let Err(e) =
        fs::create_dir_all(STATE_DIR).and_then(|_| fs::write(format!("{STATE_DIR}/done"), ""))
    {
        return Response::new(500, "text/plain", format!("{STATE_DIR}/done: {e}\n"));
    }
    if ap_active() {
        let _ = systemctl(&["stop", "--no-block", AP_UNIT]);
    }
    Response::new(200, "text/plain", "done\n")
}

// --- Scan ------------------------------------------------------------------------------------------------------

#[derive(Debug, PartialEq)]
pub struct Network {
    pub ssid: String,
    pub signal: i32,
    pub secure: bool,
}

/// `iw` writes non-printable SSID bytes as `\xNN`.
fn unescape_iw(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\' && b.get(i + 1) == Some(&b'x') {
            if let Some(v) = s
                .get(i + 2..i + 4)
                .and_then(|h| u8::from_str_radix(h, 16).ok())
            {
                out.push(v);
                i += 4;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `iw dev wlan0 scan` output: one network per name (the strongest), strongest first, hidden ones left out.
pub fn parse_scan(text: &str) -> Vec<Network> {
    let mut all: Vec<Network> = Vec::new();
    let mut cur: Option<Network> = None;
    let flush = |n: Option<Network>, all: &mut Vec<Network>| {
        let Some(n) = n else { return };
        if n.ssid.is_empty() || n.ssid.chars().all(|c| c == '\0') {
            return;
        }
        match all.iter_mut().find(|o| o.ssid == n.ssid) {
            Some(o) if o.signal < n.signal => *o = n,
            Some(_) => {}
            None => all.push(n),
        }
    };
    for line in text.lines() {
        if line.starts_with("BSS ") {
            flush(cur.take(), &mut all);
            cur = Some(Network {
                ssid: String::new(),
                signal: -100,
                secure: false,
            });
            continue;
        }
        let Some(n) = cur.as_mut() else { continue };
        let l = line.trim();
        if let Some(v) = l.strip_prefix("SSID: ") {
            n.ssid = unescape_iw(v);
        } else if l == "SSID:" {
            n.ssid.clear();
        } else if let Some(v) = l.strip_prefix("signal: ") {
            n.signal = v
                .split_whitespace()
                .next()
                .and_then(|s| s.parse::<f32>().ok())
                .map_or(-100, |f| f.round() as i32);
        } else if l.starts_with("RSN:") || l.starts_with("WPA:") || l.contains("Privacy") {
            n.secure = true;
        }
    }
    flush(cur.take(), &mut all);
    all.sort_by_key(|a| std::cmp::Reverse(a.signal));
    all
}

/// A fresh scan where the radio allows one, else the one the script made before the access point started.
pub fn scan_json() -> String {
    let mut args = vec!["dev", IFACE, "scan"];
    if ap_active() {
        args.push("ap-force");
    }
    let fresh = Command::new("iw")
        .args(&args)
        .stderr(Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();
    let mut nets = parse_scan(&fresh);
    if nets.is_empty() {
        nets = parse_scan(&fs::read_to_string(format!("{RUN_DIR}/scan.txt")).unwrap_or_default());
    }
    let mut o = String::from("{\"networks\":[");
    for (i, n) in nets.iter().take(40).enumerate() {
        if i > 0 {
            o.push(',');
        }
        o.push_str("{\"ssid\":");
        json_str(&mut o, &n.ssid);
        let _ = write!(o, ",\"signal\":{},\"secure\":{}}}", n.signal, n.secure);
    }
    o.push_str("]}");
    o
}

// --- State -----------------------------------------------------------------------------------------------------

fn kv<'a>(text: &'a str, key: &str) -> &'a str {
    text.lines()
        .find_map(|l| l.strip_prefix(key)?.strip_prefix('='))
        .unwrap_or("")
}

pub fn info_json(peer: Option<IpAddr>, storage: bool) -> String {
    let mut o = String::new();
    let _ = write!(
        o,
        "{{\"done\":{},\"via_ap\":{},\"wifi_present\":{},\"storage\":{storage},\"host\":",
        done(),
        via_ap(peer),
        wifi_present()
    );
    json_str(&mut o, &read("/proc/sys/kernel/hostname"));
    let wired = sys::default_route_ifaces()
        .iter()
        .any(|i| !Path::new(&format!("/sys/class/net/{i}/wireless")).exists());
    let _ = write!(
        o,
        ",\"wired\":{wired},\"ap\":{{\"active\":{},\"ssid\":",
        ap_active()
    );
    json_str(&mut o, &read(format!("{RUN_DIR}/ssid")));
    let _ = write!(
        o,
        ",\"address\":\"{AP_ADDR}\"}},\"wifi\":{{\"configured\":{},\"ssid\":",
        wifi_configured()
    );
    json_str(
        &mut o,
        &fs::read_to_string(WPA_CONF)
            .ok()
            .and_then(|c| parse_wpa_ssid(&c))
            .unwrap_or_default(),
    );
    let link = sys::ifaces().into_iter().find(|i| i.name == IFACE);
    o.push_str(",\"connected\":");
    json_str(
        &mut o,
        link.as_ref().and_then(|l| l.ssid.as_deref()).unwrap_or(""),
    );
    o.push_str(",\"ipv4\":");
    json_str(
        &mut o,
        link.as_ref()
            .and_then(|l| l.ipv4.first())
            .map_or("", |a| a.split('/').next().unwrap_or("")),
    );
    let _ = write!(o, ",\"joining\":{},\"requested\":", unit_active(WIFI_UNIT));
    json_str(&mut o, &read(format!("{STATE_DIR}/wifi-ssid")));
    let result = read(format!("{STATE_DIR}/wifi-result"));
    for key in ["state", "message", "address"] {
        let _ = write!(o, ",\"{key}\":");
        json_str(&mut o, kv(&result, key));
    }
    o.push_str("}}");
    o
}

// --- Background: when the access point starts --------------------------------------------------------------------

fn uptime() -> u64 {
    read("/proc/uptime")
        .split('.')
        .next()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}

/// Whether the access point should start now: no network for OFFLINE_START seconds after boot, not while a
/// join is running. Once per boot for a gateway that has a network configured; again and again (whenever it
/// is offline) for one that was never set up, so a gateway plugged in and left alone can still be set up later.
pub fn should_start(
    offline_for: u64,
    uptime: u64,
    started_this_boot: bool,
    configured: bool,
) -> bool {
    uptime >= OFFLINE_START && offline_for >= OFFLINE_START && (!started_this_boot || !configured)
}

/// Watches the network and the button; starts the access point when needed.
pub fn spawn() {
    if !wifi_present() {
        eprintln!("setup: no {IFACE}, no setup access point");
        return;
    }
    thread::spawn(|| {
        let mut offline_since: Option<Instant> = None;
        loop {
            thread::sleep(Duration::from_secs(10));
            if sys::network_ok() {
                offline_since = None;
                continue;
            }
            let since = *offline_since.get_or_insert_with(Instant::now);
            if ap_active() || unit_active(WIFI_UNIT) {
                continue;
            }
            let started = Path::new(&format!("{RUN_DIR}/started")).exists();
            let configured = wifi_configured() || done();
            if should_start(since.elapsed().as_secs(), uptime(), started, configured) {
                eprintln!("setup: no network, starting the setup access point");
                if let Err(e) = start_ap() {
                    eprintln!("setup: {e}");
                }
                offline_since = None;
            }
        }
    });
    thread::spawn(button);
    thread::spawn(dns);
}

/// The event device of the board's button(s): an `adc-keys` or `gpio-keys` input.
fn button_device(devices: &str) -> Option<String> {
    devices.split("\n\n").find_map(|block| {
        let name = block
            .lines()
            .find_map(|l| l.strip_prefix("N: Name="))?
            .trim_matches('"');
        if !(name.contains("adc-keys") || name.contains("gpio-keys")) {
            return None;
        }
        let handlers = block.lines().find_map(|l| l.strip_prefix("H: Handlers="))?;
        handlers
            .split_whitespace()
            .find(|h| h.starts_with("event"))
            .map(|h| format!("/dev/input/{h}"))
    })
}

#[derive(Debug, PartialEq)]
pub enum ButtonAction {
    SetupAp,
    FactoryReset,
}

/// What a press of this length does once released.
pub fn button_action(held: Duration) -> Option<ButtonAction> {
    if held >= BUTTON_RESET {
        Some(ButtonAction::FactoryReset)
    } else if held >= BUTTON_HOLD {
        Some(ButtonAction::SetupAp)
    } else {
        None
    }
}

/// Held 5 s and released: the setup access point; held 15 s: factory reset.
fn button() {
    let Some(dev) =
        button_device(&fs::read_to_string("/proc/bus/input/devices").unwrap_or_default())
    else {
        eprintln!("setup: no button input device");
        return;
    };
    let mut f = match File::open(&dev) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("setup: {dev}: {e}");
            return;
        }
    };
    eprintln!(
        "setup: button on {dev}: hold 5 s for the setup access point, 15 s for a factory reset"
    );
    // struct input_event on 64-bit Linux: 16 bytes time, u16 type, u16 code, i32 value
    let mut ev = [0u8; 24];
    let mut pressed: Option<Instant> = None;
    while f.read_exact(&mut ev).is_ok() {
        let kind = u16::from_ne_bytes([ev[16], ev[17]]);
        let value = i32::from_ne_bytes([ev[20], ev[21], ev[22], ev[23]]);
        if kind != 1 {
            continue; // EV_KEY only
        }
        match value {
            1 => pressed = Some(Instant::now()),
            0 => {
                let action = pressed.take().and_then(|p| button_action(p.elapsed()));
                let started = match action {
                    Some(ButtonAction::SetupAp) => {
                        eprintln!("setup: button held, starting the setup access point");
                        start_ap()
                    }
                    Some(ButtonAction::FactoryReset) => {
                        eprintln!("setup: button held 15 s, factory reset");
                        systemctl(&["start", "--no-block", RESET_UNIT])
                    }
                    None => Ok(()),
                };
                if let Err(e) = started {
                    eprintln!("setup: {e}");
                }
            }
            _ => {}
        }
    }
    eprintln!("setup: {dev} closed");
}

// --- DNS on the setup network -------------------------------------------------------------------------------------

/// The answer to a DNS query: every A question gets AP_ADDR, other types an empty answer. None for anything
/// that is not a standard query with one question.
pub fn dns_answer(q: &[u8]) -> Option<Vec<u8>> {
    if q.len() < 12 || q[2] & 0x80 != 0 || (q[2] >> 3) & 0x0f != 0 || q[4..6] != [0, 1] {
        return None;
    }
    // The question: labels up to a zero byte, then type and class.
    let mut i = 12;
    loop {
        let len = *q.get(i)? as usize;
        if len == 0 {
            i += 1;
            break;
        }
        if len & 0xc0 != 0 {
            return None;
        }
        i += 1 + len;
    }
    let qtype = u16::from_be_bytes([*q.get(i)?, *q.get(i + 1)?]);
    let qclass = u16::from_be_bytes([*q.get(i + 2)?, *q.get(i + 3)?]);
    let end = i + 4;
    let a = qtype == 1 && qclass == 1;
    let mut r = Vec::with_capacity(end + 16);
    r.extend_from_slice(&q[..2]);
    // QR, opcode 0, AA, RD copied; RA; NOERROR
    r.push(0x84 | (q[2] & 0x01));
    r.push(0x80);
    r.extend_from_slice(&[0, 1, 0, a as u8, 0, 0, 0, 0]);
    r.extend_from_slice(&q[12..end]);
    if a {
        r.extend_from_slice(&[0xc0, 0x0c, 0, 1, 0, 1, 0, 0, 0, 10, 0, 4]);
        r.extend_from_slice(&AP_ADDR.octets());
    }
    Some(r)
}

/// Answers DNS on the setup network while the access point is up.
fn dns() {
    loop {
        thread::sleep(Duration::from_secs(2));
        if !ap_active() {
            continue;
        }
        let Ok(sock) = UdpSocket::bind((AP_ADDR, 53)) else {
            continue; // address not up yet
        };
        let _ = sock.set_read_timeout(Some(Duration::from_secs(2)));
        let mut buf = [0u8; 512];
        while ap_active() {
            match sock.recv_from(&mut buf) {
                Ok((n, from)) => {
                    if let Some(r) = dns_answer(&buf[..n]) {
                        let _ = sock.send_to(&r, from);
                    }
                }
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                    ) => {}
                Err(_) => break,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wpa_roundtrip() {
        let c = render_wpa("Home \"net\"", "pass word\"x1");
        assert!(c.contains("ssid=486f6d6520226e657422\n"));
        assert!(c.contains("psk=\"pass word\"x1\"\n"));
        assert!(c.contains("ctrl_interface="));
        assert_eq!(parse_wpa_ssid(&c).unwrap(), "Home \"net\"");
        let open = render_wpa("cafe", "");
        assert!(open.contains("key_mgmt=NONE") && !open.contains("psk="));
        assert_eq!(
            parse_wpa_ssid("network={\n\tssid=\"My Wi-Fi\"\n\t#psk=\"x\"\n\tpsk=ab\n}\n").unwrap(),
            "My Wi-Fi"
        );
    }

    #[test]
    fn checks() {
        assert!(check_ssid("").is_err());
        assert!(check_ssid(&"x".repeat(33)).is_err());
        assert!(check_ssid("a\nb").is_err());
        assert!(check_ssid("Żabka 5G").is_ok());
        assert!(check_psk("").is_ok());
        assert!(check_psk("short").is_err());
        assert!(check_psk("12345678").is_ok());
        assert!(check_psk(&"x".repeat(64)).is_err());
        assert!(check_psk("new\nline1").is_err());
        assert!(check_psk("zażółć123").is_err());
    }

    #[test]
    fn form() {
        let f = "ssid=My+Net%26Co&psk=a%3Db%25c12";
        assert_eq!(form_value(f, "ssid").unwrap(), "My Net&Co");
        assert_eq!(form_value(f, "psk").unwrap(), "a=b%c12");
        assert_eq!(form_value(f, "x"), None);
    }

    #[test]
    fn scan() {
        let text = "BSS 11:22:33:44:55:66(on wlan0)\n\
            \tfreq: 2412\n\tsignal: -71.00 dBm\n\tSSID: Home\n\tRSN:\t * Version: 1\n\
            BSS 11:22:33:44:55:67(on wlan0)\n\tsignal: -48.00 dBm\n\tSSID: Home\n\tRSN:\t * Version: 1\n\
            BSS aa:22:33:44:55:66(on wlan0)\n\tcapability: ESS (0x0401)\n\tsignal: -60.00 dBm\n\tSSID: Caf\\xc3\\xa9\n\
            BSS bb:22:33:44:55:66(on wlan0)\n\tsignal: -30.00 dBm\n\tSSID: \n\tRSN:\n\
            BSS cc:22:33:44:55:66(on wlan0)\n\tcapability: ESS Privacy (0x0411)\n\tsignal: -80.00 dBm\n\tSSID: Old\n";
        assert_eq!(
            parse_scan(text),
            vec![
                Network {
                    ssid: "Home".into(),
                    signal: -48,
                    secure: true
                },
                Network {
                    ssid: "Café".into(),
                    signal: -60,
                    secure: false
                },
                Network {
                    ssid: "Old".into(),
                    signal: -80,
                    secure: true
                },
            ]
        );
    }

    #[test]
    fn peers_and_redirect() {
        let phone: IpAddr = "::ffff:192.168.4.23".parse().unwrap();
        assert!(via_ap(Some(phone)));
        assert!(!via_ap(Some("192.168.4.1".parse().unwrap())));
        assert!(!via_ap(Some("192.168.1.20".parse().unwrap())));
        assert!(!via_ap(None));
        // Not on the setup network (or no access point here): never redirected
        assert!(captive_redirect(Some("10.0.0.2".parse().unwrap()), Some("example.com")).is_none());
    }

    #[test]
    fn start_rules() {
        assert!(!should_start(300, 60, false, false), "too early after boot");
        assert!(!should_start(60, 600, false, false), "offline too short");
        assert!(should_start(130, 130, false, true), "first time this boot");
        assert!(
            !should_start(500, 900, true, true),
            "once per boot when configured"
        );
        assert!(
            should_start(500, 900, true, false),
            "again when never set up"
        );
    }

    #[test]
    fn button_lengths() {
        let s = Duration::from_secs;
        assert_eq!(button_action(s(1)), None);
        assert_eq!(button_action(s(5)), Some(ButtonAction::SetupAp));
        assert_eq!(button_action(s(14)), Some(ButtonAction::SetupAp));
        assert_eq!(button_action(s(15)), Some(ButtonAction::FactoryReset));
        assert_eq!(button_action(s(40)), Some(ButtonAction::FactoryReset));
    }

    #[test]
    fn button_dev() {
        let d = "I: Bus=0019 Vendor=0001 Product=0001 Version=0100\nN: Name=\"rk805 pwrkey\"\nH: Handlers=kbd event0 \n\n\
                 I: Bus=0019 Vendor=0001 Product=0001 Version=0100\nN: Name=\"adc-keys\"\nP: Phys=adc-keys/input0\nH: Handlers=kbd event1 \n";
        assert_eq!(button_device(d).unwrap(), "/dev/input/event1");
        assert_eq!(button_device("N: Name=\"x\"\nH: Handlers=event3\n"), None);
    }

    #[test]
    fn dns() {
        // id 0x1234, RD, one question: connectivitycheck.gstatic.com? no, "a.b" A IN
        let q = [
            0x12, 0x34, 0x01, 0x00, 0, 1, 0, 0, 0, 0, 0, 0, 1, b'a', 1, b'b', 0, 0, 1, 0, 1,
        ];
        let r = dns_answer(&q).unwrap();
        assert_eq!(&r[..4], &[0x12, 0x34, 0x85, 0x80]);
        assert_eq!(&r[4..12], &[0, 1, 0, 1, 0, 0, 0, 0]);
        assert_eq!(&r[12..21], &q[12..]);
        assert_eq!(&r[r.len() - 4..], &[192, 168, 4, 1]);
        // AAAA: no answer, still NOERROR
        let mut q6 = q;
        q6[18] = 28;
        let r = dns_answer(&q6).unwrap();
        assert_eq!(&r[6..8], &[0, 0]);
        assert_eq!(r.len(), q.len());
        // A response, or a truncated question, is ignored
        let mut resp = q;
        resp[2] |= 0x80;
        assert!(dns_answer(&resp).is_none());
        assert!(dns_answer(&q[..15]).is_none());
    }
}
