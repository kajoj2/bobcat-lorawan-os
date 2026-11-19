// SPDX-License-Identifier: MIT
//! What the concentrator does, as the MQTT Forwarder reports it: gateway ID, connection state, the stats it sends
//! every interval (counters per channel, data rate and TX result, duty cycle), the downlink results and the last
//! frames heard. Field numbers: chirpstack_api 4.19, `proto/chirpstack/gw/gw.proto`.

use crate::mqtt::Client;
use crate::pb::Reader;
use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const MAX_FRAMES: usize = 25;
pub const HISTORY_HOURS: usize = 24;

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[derive(Clone, Debug, Default)]
pub struct Frame {
    pub at: u64,
    pub frequency: u32,
    /// "SF7 BW125", "FSK 50k"
    pub modulation: String,
    pub code_rate: &'static str,
    pub channel: u32,
    pub rf_chain: u32,
    pub rssi: i32,
    pub snr: f32,
    /// 0 no CRC, 1 bad CRC, 2 CRC OK (CRCStatus)
    pub crc_status: u32,
    pub size: usize,
    pub kind: &'static str,
    /// DevAddr for data frames, DevEUI for join requests
    pub device: String,
    pub f_cnt: Option<u16>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Counts {
    pub rx: u64,
    pub rx_ok: u64,
    pub tx: u64,
    pub tx_emitted: u64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Band {
    pub name: String,
    pub frequency_min: u32,
    pub frequency_max: u32,
    pub load_max_ms: u64,
    pub load_tracked_ms: u64,
}

#[derive(Clone, Debug, Default)]
pub struct DutyCycle {
    pub regulation: &'static str,
    pub window_s: u64,
    pub bands: Vec<Band>,
}

/// One hour of counters, from the stats; `start` is the hour's first second (Unix time).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Hour {
    pub start: u64,
    pub rx: u64,
    pub rx_ok: u64,
    pub tx: u64,
}

#[derive(Clone, Debug, Default)]
pub struct Radio {
    pub mqtt_connected: bool,
    pub mqtt_error: Option<String>,
    pub gateway_id: Option<String>,
    pub online: Option<bool>,
    pub last_stats_at: Option<u64>,
    /// The last stats interval
    pub last: Counts,
    /// Everything below: summed since the agent started
    pub total: Counts,
    pub rx_per_frequency: BTreeMap<u32, u64>,
    pub tx_per_frequency: BTreeMap<u32, u64>,
    pub rx_per_modulation: BTreeMap<String, u64>,
    pub tx_per_modulation: BTreeMap<String, u64>,
    /// From the stats (Concentratord's own TX results)
    pub tx_per_status: BTreeMap<String, u64>,
    /// From the downlink acknowledgements
    pub acks: BTreeMap<String, u64>,
    pub duty_cycle: Option<DutyCycle>,
    pub history: VecDeque<Hour>,
    pub frames: VecDeque<Frame>,
    pub versions: Vec<(String, String)>,
}

pub type Shared = Arc<Mutex<Radio>>;

/// Splits `<prefix>/gateway/<id>/<event|state>/<name>`.
fn parse_topic(topic: &str) -> Option<(&str, &str, &str)> {
    let mut it = topic.split('/');
    let (_prefix, gw, id, class, name) =
        (it.next()?, it.next()?, it.next()?, it.next()?, it.next()?);
    (gw == "gateway" && it.next().is_none()).then_some((id, class, name))
}

fn lorawan_kind(phy: &[u8]) -> (&'static str, String, Option<u16>) {
    let Some(&mhdr) = phy.first() else {
        return ("empty", String::new(), None);
    };
    let kind = match mhdr >> 5 {
        0 => "JoinRequest",
        1 => "JoinAccept",
        2 => "UnconfirmedUp",
        3 => "UnconfirmedDown",
        4 => "ConfirmedUp",
        5 => "ConfirmedDown",
        6 => "RejoinRequest",
        _ => "Proprietary",
    };
    match mhdr >> 5 {
        2..=5 if phy.len() >= 8 => {
            let addr = u32::from_le_bytes([phy[1], phy[2], phy[3], phy[4]]);
            (
                kind,
                format!("{addr:08x}"),
                Some(u16::from_le_bytes([phy[6], phy[7]])),
            )
        }
        0 if phy.len() >= 17 => {
            let dev_eui: String = phy[9..17]
                .iter()
                .rev()
                .map(|b| format!("{b:02x}"))
                .collect();
            (kind, dev_eui, None)
        }
        _ => (kind, String::new(), None),
    }
}

fn code_rate(v: u64) -> &'static str {
    match v {
        1 => "4/5",
        2 => "4/6",
        3 => "4/7",
        4 => "4/8",
        5 => "3/8",
        6 => "2/6",
        7 => "1/4",
        8 => "1/6",
        9 => "5/6",
        10 => "LI 4/5",
        11 => "LI 4/6",
        12 => "LI 4/8",
        _ => "",
    }
}

pub fn tx_ack_status(v: u64) -> &'static str {
    match v {
        0 => "IGNORED",
        1 => "OK",
        2 => "TOO_LATE",
        3 => "TOO_EARLY",
        4 => "COLLISION_PACKET",
        5 => "COLLISION_BEACON",
        6 => "TX_FREQ",
        7 => "TX_POWER",
        8 => "GPS_UNLOCKED",
        9 => "QUEUE_FULL",
        10 => "INTERNAL_ERROR",
        11 => "DUTY_CYCLE_OVERFLOW",
        _ => "UNKNOWN",
    }
}

fn khz(hz: u64) -> String {
    if hz.is_multiple_of(1000) {
        format!("{}", hz / 1000)
    } else {
        format!("{:.1}", hz as f64 / 1000.0)
    }
}

/// gw.Modulation { lora = 3 { bandwidth = 1; spreading_factor = 2; code_rate = 5 }, fsk = 4 { datarate = 2 },
/// lr_fhss = 5 } -> ("SF7 BW125", code rate)
fn decode_modulation(buf: &[u8]) -> (String, &'static str) {
    for (field, v) in Reader::new(buf) {
        let inner = v.as_bytes().unwrap_or_default();
        match field {
            3 => {
                let (mut bw, mut sf, mut cr) = (0, 0, "");
                for (f, x) in Reader::new(inner) {
                    match f {
                        1 => bw = x.as_u64().unwrap_or(0),
                        2 => sf = x.as_u64().unwrap_or(0),
                        5 => cr = code_rate(x.as_u64().unwrap_or(0)),
                        _ => {}
                    }
                }
                return (format!("SF{sf} BW{}", khz(bw)), cr);
            }
            4 => {
                let rate = Reader::new(inner)
                    .find(|(f, _)| *f == 2)
                    .and_then(|(_, x)| x.as_u64())
                    .unwrap_or(0);
                return (format!("FSK {} kbit/s", khz(rate)), "");
            }
            5 => return ("LR-FHSS".into(), ""),
            _ => {}
        }
    }
    (String::new(), "")
}

/// gw.UplinkFrame -> Frame
pub fn decode_uplink(buf: &[u8], at: u64) -> Frame {
    let mut f = Frame {
        at,
        ..Default::default()
    };
    for (field, v) in Reader::new(buf) {
        let inner = v.as_bytes().unwrap_or_default();
        match field {
            1 => {
                f.size = inner.len();
                (f.kind, f.device, f.f_cnt) = lorawan_kind(inner);
            }
            4 => {
                // UplinkTxInfo { frequency = 1; modulation = 2 }
                for (tf, tv) in Reader::new(inner) {
                    match tf {
                        1 => f.frequency = tv.as_u64().unwrap_or(0) as u32,
                        2 => {
                            (f.modulation, f.code_rate) =
                                decode_modulation(tv.as_bytes().unwrap_or_default())
                        }
                        _ => {}
                    }
                }
            }
            5 => {
                // UplinkRxInfo { rssi = 6; snr = 7; channel = 8; rf_chain = 9; crc_status = 16 }
                for (rf, rv) in Reader::new(inner) {
                    match rf {
                        6 => f.rssi = rv.as_i32().unwrap_or(0),
                        7 => f.snr = rv.as_f32().unwrap_or(0.0),
                        8 => f.channel = rv.as_u64().unwrap_or(0) as u32,
                        9 => f.rf_chain = rv.as_u64().unwrap_or(0) as u32,
                        16 => f.crc_status = rv.as_u64().unwrap_or(0) as u32,
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    f
}

/// map<K, V> entry { key = 1; value = 2 }
fn map_entry<'a>(buf: &'a [u8]) -> (Option<crate::pb::Value<'a>>, Option<crate::pb::Value<'a>>) {
    let (mut k, mut v) = (None, None);
    for (f, x) in Reader::new(buf) {
        match f {
            1 => k = Some(x),
            2 => v = Some(x),
            _ => {}
        }
    }
    (k, v)
}

/// google.protobuf.Duration { seconds = 1; nanos = 2 } -> milliseconds
fn duration_ms(buf: &[u8]) -> u64 {
    let (mut s, mut n) = (0u64, 0u64);
    for (f, x) in Reader::new(buf) {
        match f {
            1 => s = x.as_u64().unwrap_or(0),
            2 => n = x.as_u64().unwrap_or(0),
            _ => {}
        }
    }
    s * 1000 + n / 1_000_000
}

fn decode_duty_cycle(buf: &[u8]) -> DutyCycle {
    let mut d = DutyCycle::default();
    for (field, v) in Reader::new(buf) {
        let inner = v.as_bytes().unwrap_or_default();
        match field {
            1 => {
                d.regulation = match v.as_u64() {
                    Some(1) => "ETSI EN 300 220",
                    _ => "",
                }
            }
            2 => d.window_s = duration_ms(inner) / 1000,
            3 => {
                let mut b = Band::default();
                for (f, x) in Reader::new(inner) {
                    match f {
                        1 => b.name = x.as_str().unwrap_or_default().to_string(),
                        2 => b.frequency_min = x.as_u64().unwrap_or(0) as u32,
                        3 => b.frequency_max = x.as_u64().unwrap_or(0) as u32,
                        4 => b.load_max_ms = duration_ms(x.as_bytes().unwrap_or_default()),
                        5 => b.load_tracked_ms = duration_ms(x.as_bytes().unwrap_or_default()),
                        _ => {}
                    }
                }
                d.bands.push(b);
            }
            _ => {}
        }
    }
    d
}

/// Everything one gw.GatewayStats message carries that the panel shows.
#[derive(Debug, Default)]
pub struct Stats {
    pub counts: Counts,
    pub rx_per_frequency: Vec<(u32, u64)>,
    pub tx_per_frequency: Vec<(u32, u64)>,
    pub rx_per_modulation: Vec<(String, u64)>,
    pub tx_per_modulation: Vec<(String, u64)>,
    pub tx_per_status: Vec<(String, u64)>,
    pub duty_cycle: Option<DutyCycle>,
    pub metadata: Vec<(String, String)>,
}

pub fn decode_stats(buf: &[u8]) -> Stats {
    let mut s = Stats::default();
    for (field, v) in Reader::new(buf) {
        let n = v.as_u64().unwrap_or(0);
        let inner = v.as_bytes().unwrap_or_default();
        match field {
            5 => s.counts.rx = n,
            6 => s.counts.rx_ok = n,
            7 => s.counts.tx = n,
            8 => s.counts.tx_emitted = n,
            10 => {
                let (k, val) = map_entry(inner);
                s.metadata.push((
                    k.and_then(|x| x.as_str()).unwrap_or_default().to_string(),
                    val.and_then(|x| x.as_str()).unwrap_or_default().to_string(),
                ));
            }
            12 | 13 => {
                let (k, val) = map_entry(inner);
                let e = (
                    k.and_then(|x| x.as_u64()).unwrap_or(0) as u32,
                    val.and_then(|x| x.as_u64()).unwrap_or(0),
                );
                if field == 12 {
                    &mut s.tx_per_frequency
                } else {
                    &mut s.rx_per_frequency
                }
                .push(e);
            }
            14 | 15 => {
                // PerModulationCount { modulation = 1; count = 2 }
                let (m, c) = map_entry(inner);
                let e = (
                    decode_modulation(m.and_then(|x| x.as_bytes()).unwrap_or_default()).0,
                    c.and_then(|x| x.as_u64()).unwrap_or(0),
                );
                if field == 14 {
                    &mut s.tx_per_modulation
                } else {
                    &mut s.rx_per_modulation
                }
                .push(e);
            }
            16 => {
                let (k, val) = map_entry(inner);
                s.tx_per_status.push((
                    k.and_then(|x| x.as_str()).unwrap_or_default().to_string(),
                    val.and_then(|x| x.as_u64()).unwrap_or(0),
                ));
            }
            18 => s.duty_cycle = Some(decode_duty_cycle(inner)),
            _ => {}
        }
    }
    s.metadata.sort();
    s
}

/// gw.ConnState { state = 2 } -> online
pub fn decode_conn(buf: &[u8]) -> bool {
    Reader::new(buf).any(|(f, v)| f == 2 && v.as_u64() == Some(1))
}

/// gw.DownlinkTxAck { items = 5 { status = 1 } } -> one status per item (the first that was not IGNORED
/// is the one used; the others are the alternatives not tried)
pub fn decode_ack(buf: &[u8]) -> Vec<&'static str> {
    Reader::new(buf)
        .filter(|(f, _)| *f == 5)
        .map(|(_, v)| {
            let status = Reader::new(v.as_bytes().unwrap_or_default())
                .find(|(f, _)| *f == 1)
                .and_then(|(_, x)| x.as_u64())
                .unwrap_or(0);
            tx_ack_status(status)
        })
        .collect()
}

fn add<K: Ord + Clone>(map: &mut BTreeMap<K, u64>, items: &[(K, u64)]) {
    for (k, n) in items {
        *map.entry(k.clone()).or_default() += n;
    }
}

fn add_to_history(history: &mut VecDeque<Hour>, at: u64, c: Counts) {
    let start = at - at % 3600;
    if history.back().is_none_or(|h| h.start != start) {
        history.push_back(Hour {
            start,
            ..Default::default()
        });
        while history.len() > HISTORY_HOURS {
            history.pop_front();
        }
    }
    let h = history.back_mut().expect("pushed above");
    h.rx += c.rx;
    h.rx_ok += c.rx_ok;
    h.tx += c.tx_emitted;
}

pub fn handle_at(radio: &mut Radio, topic: &str, payload: &[u8], at: u64) {
    let Some((id, class, name)) = parse_topic(topic) else {
        return;
    };
    radio.gateway_id = Some(id.to_string());
    match (class, name) {
        ("state", "conn") => radio.online = Some(decode_conn(payload)),
        ("event", "stats") => {
            let s = decode_stats(payload);
            let c = s.counts;
            radio.last = c;
            radio.total.rx += c.rx;
            radio.total.rx_ok += c.rx_ok;
            radio.total.tx += c.tx;
            radio.total.tx_emitted += c.tx_emitted;
            add(&mut radio.rx_per_frequency, &s.rx_per_frequency);
            add(&mut radio.tx_per_frequency, &s.tx_per_frequency);
            add(&mut radio.rx_per_modulation, &s.rx_per_modulation);
            add(&mut radio.tx_per_modulation, &s.tx_per_modulation);
            add(&mut radio.tx_per_status, &s.tx_per_status);
            if s.duty_cycle.is_some() {
                radio.duty_cycle = s.duty_cycle;
            }
            add_to_history(&mut radio.history, at, c);
            radio.last_stats_at = Some(at);
            if !s.metadata.is_empty() {
                radio.versions = s.metadata;
            }
        }
        ("event", "up") => {
            radio.frames.push_front(decode_uplink(payload, at));
            radio.frames.truncate(MAX_FRAMES);
        }
        ("event", "ack") => {
            if let Some(status) = decode_ack(payload).into_iter().find(|s| *s != "IGNORED") {
                *radio.acks.entry(status.to_string()).or_default() += 1;
            }
        }
        _ => {}
    }
}

pub fn handle(radio: &mut Radio, topic: &str, payload: &[u8]) {
    handle_at(radio, topic, payload, now())
}

pub const TOPICS: [&str; 2] = ["+/gateway/+/event/+", "+/gateway/+/state/+"];

/// Follows the broker forever, reconnecting with a growing pause.
pub fn spawn(shared: Shared, broker: String) {
    thread::spawn(move || {
        let mut pause = Duration::from_secs(2);
        loop {
            let result: std::io::Result<()> = Client::connect(
                &broker,
                &format!("lorawan-agent-{}", std::process::id()),
                Duration::from_secs(60),
            )
            .and_then(|mut c| {
                c.subscribe(&TOPICS)?;
                {
                    let mut r = shared.lock().unwrap();
                    r.mqtt_connected = true;
                    r.mqtt_error = None;
                }
                pause = Duration::from_secs(2);
                loop {
                    let p = c.next_publish()?;
                    handle(&mut shared.lock().unwrap(), &p.topic, &p.payload);
                }
            });
            if let Err(e) = result {
                let mut r = shared.lock().unwrap();
                r.mqtt_connected = false;
                r.mqtt_error = Some(e.to_string());
            }
            thread::sleep(pause);
            pause = (pause * 2).min(Duration::from_secs(30));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pb::Writer;

    fn lora(bw: u64, sf: u64, cr: u64) -> Vec<u8> {
        let mut l = Writer::default();
        l.uint(1, bw).uint(2, sf).uint(5, cr);
        let mut m = Writer::default();
        m.message(3, &l.0);
        m.0
    }

    fn entry_u(k: u64, v: u64) -> Vec<u8> {
        let mut e = Writer::default();
        e.uint(1, k).uint(2, v);
        e.0
    }

    fn duration(s: u64, nanos: u64) -> Vec<u8> {
        let mut d = Writer::default();
        d.uint(1, s).uint(2, nanos);
        d.0
    }

    #[test]
    fn topics() {
        assert_eq!(
            parse_topic("eu868/gateway/0016c001f150193b/event/up"),
            Some(("0016c001f150193b", "event", "up"))
        );
        assert_eq!(parse_topic("eu868/gateway/x/event/up/extra"), None);
        assert_eq!(parse_topic("eu868/application/x/event/up"), None);
    }

    #[test]
    fn uplink() {
        let mut tx = Writer::default();
        tx.uint(1, 868_100_000).message(2, &lora(125_000, 7, 1));
        let mut rx = Writer::default();
        rx.string(1, "0016c001f150193b")
            .uint(6, (-97i64) as u64)
            .uint(8, 3)
            .uint(9, 1)
            .uint(16, 2);
        rx.0.push(0x3d);
        rx.0.extend_from_slice(&7.25f32.to_le_bytes());
        // unconfirmed data up, DevAddr 260b1234, FCnt 42
        let phy = [0x40, 0x34, 0x12, 0x0b, 0x26, 0x80, 42, 0, 1, 2, 3, 4, 5, 6];
        let mut up = Writer::default();
        up.bytes(1, &phy).message(4, &tx.0).message(5, &rx.0);

        let f = decode_uplink(&up.0, 7);
        assert_eq!(
            (f.frequency, f.modulation.as_str(), f.code_rate),
            (868_100_000, "SF7 BW125", "4/5")
        );
        assert_eq!(
            (f.rssi, f.snr, f.crc_status, f.channel, f.rf_chain),
            (-97, 7.25, 2, 3, 1)
        );
        assert_eq!(
            (f.kind, f.device.as_str(), f.f_cnt, f.size),
            ("UnconfirmedUp", "260b1234", Some(42), 14)
        );
    }

    #[test]
    fn stats_counters_and_history() {
        let mut meta = Writer::default();
        meta.string(1, "concentratord_version").string(2, "4.7.1");
        let mut per_mod = Writer::default();
        per_mod.message(1, &lora(125_000, 12, 0)).uint(2, 2);
        let mut status = Writer::default();
        status.string(1, "OK").uint(2, 1);
        let mut band = Writer::default();
        band.string(1, "L")
            .uint(2, 869_400_000)
            .uint(3, 869_650_000)
            .message(4, &duration(360, 0))
            .message(5, &duration(1, 500_000_000));
        let mut dc = Writer::default();
        dc.uint(1, 1)
            .message(2, &duration(3600, 0))
            .message(3, &band.0);
        let mut s = Writer::default();
        s.string(17, "0016c001f150193b")
            .uint(5, 3)
            .uint(6, 2)
            .uint(8, 1)
            .message(10, &meta.0)
            .message(13, &entry_u(868_100_000, 3))
            .message(12, &entry_u(869_525_000, 1))
            .message(15, &per_mod.0)
            .message(16, &status.0)
            .message(18, &dc.0);

        let mut radio = Radio::default();
        let topic = "eu868/gateway/0016c001f150193b/event/stats";
        handle_at(&mut radio, topic, &s.0, 7200 + 10);
        handle_at(&mut radio, topic, &s.0, 7200 + 40);
        handle_at(&mut radio, topic, &s.0, 10800 + 5);
        assert_eq!(
            (radio.last.rx, radio.last.rx_ok, radio.last.tx_emitted),
            (3, 2, 1)
        );
        assert_eq!(radio.total.rx, 9);
        assert_eq!(radio.rx_per_frequency.get(&868_100_000), Some(&9));
        assert_eq!(radio.tx_per_frequency.get(&869_525_000), Some(&3));
        assert_eq!(radio.rx_per_modulation.get("SF12 BW125"), Some(&6));
        assert_eq!(radio.tx_per_status.get("OK"), Some(&3));
        let dc = radio.duty_cycle.as_ref().unwrap();
        assert_eq!((dc.regulation, dc.window_s), ("ETSI EN 300 220", 3600));
        assert_eq!(
            dc.bands[0],
            Band {
                name: "L".into(),
                frequency_min: 869_400_000,
                frequency_max: 869_650_000,
                load_max_ms: 360_000,
                load_tracked_ms: 1500
            }
        );
        assert_eq!(
            radio.history.iter().copied().collect::<Vec<_>>(),
            vec![
                Hour {
                    start: 7200,
                    rx: 6,
                    rx_ok: 4,
                    tx: 2
                },
                Hour {
                    start: 10800,
                    rx: 3,
                    rx_ok: 2,
                    tx: 1
                }
            ]
        );
        assert_eq!(
            radio.versions,
            vec![("concentratord_version".into(), "4.7.1".into())]
        );
    }

    #[test]
    fn history_keeps_a_day() {
        let mut h = VecDeque::new();
        for i in 0..30 {
            add_to_history(
                &mut h,
                i * 3600,
                Counts {
                    rx: 1,
                    ..Default::default()
                },
            );
        }
        assert_eq!(h.len(), HISTORY_HOURS);
        assert_eq!(h.front().unwrap().start, 6 * 3600);
    }

    #[test]
    fn conn_and_ack() {
        let mut radio = Radio::default();
        let mut conn = Writer::default();
        conn.string(3, "0016c001f150193b").uint(2, 1);
        handle(
            &mut radio,
            "eu868/gateway/0016c001f150193b/state/conn",
            &conn.0,
        );
        assert_eq!(radio.online, Some(true));
        handle(&mut radio, "eu868/gateway/0016c001f150193b/state/conn", &[]);
        assert_eq!(radio.online, Some(false));

        // first item TOO_LATE, second (RX2, not tried) IGNORED
        let (mut a, mut b) = (Writer::default(), Writer::default());
        a.uint(1, 2);
        b.uint(1, 0);
        let mut ack = Writer::default();
        ack.uint(2, 77).message(5, &a.0).message(5, &b.0);
        assert_eq!(decode_ack(&ack.0), vec!["TOO_LATE", "IGNORED"]);
        handle(
            &mut radio,
            "eu868/gateway/0016c001f150193b/event/ack",
            &ack.0,
        );
        assert_eq!(radio.acks.get("TOO_LATE"), Some(&1));
    }

    #[test]
    fn fsk_modulation() {
        let mut fsk = Writer::default();
        fsk.uint(1, 25_000).uint(2, 50_000);
        let mut m = Writer::default();
        m.message(4, &fsk.0);
        assert_eq!(decode_modulation(&m.0).0, "FSK 50 kbit/s");
    }

    #[test]
    fn join_request_dev_eui() {
        let mut phy = vec![0x00];
        phy.extend_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
        phy.extend_from_slice(&[0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88]);
        phy.extend_from_slice(&[0, 0, 0, 0, 0, 0]);
        let (kind, dev, _) = lorawan_kind(&phy);
        assert_eq!((kind, dev.as_str()), ("JoinRequest", "8877665544332211"));
    }
}
