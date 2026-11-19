// SPDX-License-Identifier: MIT
//! First boot: add this gateway to the local ChirpStack, so it shows up in the web interface without typing its
//! EUI. Runs as the chirpstack user with ChirpStack's environment (lorawan-agent-register.service):
//! 1. the gateway ID from the forwarder's retained connection state on MQTT;
//! 2. an API key from `chirpstack create-api-key`, kept in the state directory for later runs;
//! 3. gRPC-web calls to ChirpStack's API (the same port and protocol its web interface uses):
//!    TenantService/List for the default tenant, then GatewayService/Create.

use crate::http;
use crate::mqtt::Client;
use crate::pb::{Reader, Writer};
use crate::radio;
use crate::status::STATE_DIR;
use std::fs;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

pub struct Options {
    pub broker: String,
    pub api: String,
    pub chirpstack_config: String,
    pub name: Option<String>,
    /// Instead of the one the forwarder reports (tests, a second gateway)
    pub gateway_id: Option<String>,
}

#[derive(Debug)]
pub struct GrpcError {
    pub code: u32,
    pub message: String,
}

impl std::fmt::Display for GrpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "gRPC status {}: {}", self.code, self.message)
    }
}

pub const NOT_FOUND: u32 = 5;
pub const ALREADY_EXISTS: u32 = 6;

/// grpc-message is percent-encoded.
fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Some(v) = std::str::from_utf8(&b[i + 1..i + 3])
                .ok()
                .and_then(|h| u8::from_str_radix(h, 16).ok())
            {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn gateway_id_from_mqtt(broker: &str, wait: Duration) -> Result<String, String> {
    let deadline = Instant::now() + wait;
    loop {
        let attempt = Client::connect(
            broker,
            &format!("lorawan-agent-register-{}", std::process::id()),
            Duration::from_secs(10),
        )
        .and_then(|mut c| {
            c.subscribe(&radio::TOPICS)?;
            loop {
                let p = c.next_publish()?;
                let mut r = radio::Radio::default();
                radio::handle(&mut r, &p.topic, &p.payload);
                if let Some(id) = r.gateway_id {
                    return Ok(id);
                }
            }
        });
        match attempt {
            Ok(id) => return Ok(id),
            Err(e) if Instant::now() >= deadline => {
                return Err(format!("no gateway ID on MQTT ({broker}): {e}"))
            }
            Err(_) => thread::sleep(Duration::from_secs(5)),
        }
    }
}

fn api_token(chirpstack_config: &str) -> Result<String, String> {
    let path = format!("{STATE_DIR}/api-token");
    if let Ok(t) = fs::read_to_string(&path) {
        if !t.trim().is_empty() {
            return Ok(t.trim().to_string());
        }
    }
    let out = Command::new("chirpstack")
        .args([
            "-c",
            chirpstack_config,
            "create-api-key",
            "--name",
            "lorawan-agent",
        ])
        .output()
        .map_err(|e| format!("chirpstack create-api-key: {e}"))?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    let token = stdout
        .lines()
        .find_map(|l| l.strip_prefix("token: "))
        .map(|t| t.trim().to_string())
        .ok_or_else(|| {
            format!(
                "chirpstack create-api-key gave no token: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            )
        })?;
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&path)
        .map_err(|e| format!("{path}: {e}"))?;
    f.write_all(token.as_bytes())
        .map_err(|e| format!("{path}: {e}"))?;
    Ok(token)
}

/// One unary gRPC-web call: request and response messages are protobuf, framed as 1 flag byte + 4 length bytes;
/// the status comes in a trailer frame (flag 0x80) or, for errors without a message, in the headers.
pub fn grpc_web(api: &str, method: &str, token: &str, msg: &[u8]) -> Result<Vec<u8>, GrpcError> {
    let mut body = vec![0u8];
    body.extend_from_slice(&(msg.len() as u32).to_be_bytes());
    body.extend_from_slice(msg);
    let auth = format!("Bearer {token}");
    let r = http::request(
        api,
        "POST",
        &format!("/{method}"),
        &[
            ("Content-Type", "application/grpc-web+proto"),
            ("X-Grpc-Web", "1"),
            ("Authorization", &auth),
        ],
        &body,
        Duration::from_secs(15),
    )
    .map_err(|e| GrpcError {
        code: 14,
        message: format!("{api}: {e}"),
    })?;
    if r.status != 200 {
        return Err(GrpcError {
            code: 14,
            message: format!("HTTP {}", r.status),
        });
    }
    let mut status = r.header("grpc-status").and_then(|v| v.parse().ok());
    let mut message = r.header("grpc-message").unwrap_or_default().to_string();
    let mut reply = Vec::new();
    let mut rest = r.body.as_slice();
    while rest.len() >= 5 {
        let flag = rest[0];
        let len = u32::from_be_bytes([rest[1], rest[2], rest[3], rest[4]]) as usize;
        let Some(frame) = rest.get(5..5 + len) else {
            break;
        };
        if flag & 0x80 != 0 {
            for line in String::from_utf8_lossy(frame).lines() {
                if let Some((k, v)) = line.split_once(':') {
                    match k.trim().to_ascii_lowercase().as_str() {
                        "grpc-status" => status = v.trim().parse().ok(),
                        "grpc-message" => message = v.trim().to_string(),
                        _ => {}
                    }
                }
            }
        } else {
            reply = frame.to_vec();
        }
        rest = &rest[5 + len..];
    }
    match status {
        Some(0) => Ok(reply),
        Some(code) => Err(GrpcError {
            code,
            message: percent_decode(&message),
        }),
        None => Err(GrpcError {
            code: 2,
            message: "no grpc-status in the reply".into(),
        }),
    }
}

/// The first tenant that may have gateways (on a fresh ChirpStack: the "ChirpStack" tenant).
fn tenant_id(api: &str, token: &str) -> Result<String, String> {
    let mut req = Writer::default();
    req.uint(1, 10); // ListTenantsRequest.limit
    let reply = grpc_web(api, "api.TenantService/List", token, &req.0)
        .map_err(|e| format!("listing tenants: {e}"))?;
    let mut first = None;
    for (field, v) in Reader::new(&reply) {
        if field != 2 {
            continue; // ListTenantsResponse.result
        }
        let (mut id, mut can_have_gateways) = (String::new(), false);
        for (f, tv) in Reader::new(v.as_bytes().unwrap_or_default()) {
            match f {
                1 => id = tv.as_str().unwrap_or_default().to_string(),
                5 => can_have_gateways = tv.as_u64() == Some(1),
                _ => {}
            }
        }
        if can_have_gateways {
            return Ok(id);
        }
        first.get_or_insert(id);
    }
    first.ok_or_else(|| "ChirpStack has no tenant".into())
}

pub fn create_gateway_request(gateway_id: &str, name: &str, tenant_id: &str) -> Vec<u8> {
    let mut gw = Writer::default();
    gw.string(1, gateway_id)
        .string(2, name)
        .string(3, "This gateway, added by lorawan-agent on its first boot.")
        .string(5, tenant_id)
        .uint(8, 30); // stats_interval, as Concentratord sends them
    let mut req = Writer::default();
    req.message(1, &gw.0);
    req.0
}

fn hostname() -> String {
    fs::read_to_string("/proc/sys/kernel/hostname")
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "gateway".into())
}

fn attempt(o: &Options) -> Result<String, String> {
    let gateway_id = match &o.gateway_id {
        Some(id) => id.clone(),
        None => gateway_id_from_mqtt(&o.broker, Duration::from_secs(120))?,
    };
    let token = api_token(&o.chirpstack_config)?;
    let tenant = tenant_id(&o.api, &token)?;
    let name = o.name.clone().unwrap_or_else(hostname);
    // Added by hand already? (Create on an existing gateway is not ALREADY_EXISTS with SQLite, but INTERNAL.)
    let mut get = Writer::default();
    get.string(1, &gateway_id); // GetGatewayRequest.gateway_id
    match grpc_web(&o.api, "api.GatewayService/Get", &token, &get.0) {
        Ok(_) => println!("gateway {gateway_id} is already in ChirpStack"),
        Err(e) if e.code == NOT_FOUND => {
            match grpc_web(
                &o.api,
                "api.GatewayService/Create",
                &token,
                &create_gateway_request(&gateway_id, &name, &tenant),
            ) {
                Ok(_) => println!("gateway {gateway_id} added to ChirpStack as \"{name}\""),
                Err(e) if e.code == ALREADY_EXISTS => {
                    println!("gateway {gateway_id} is already in ChirpStack")
                }
                Err(e) => return Err(format!("adding the gateway: {e}")),
            }
        }
        Err(e) => return Err(format!("looking the gateway up: {e}")),
    }
    if o.gateway_id.is_none() {
        fs::write(format!("{STATE_DIR}/registered"), format!("{gateway_id}\n"))
            .map_err(|e| format!("{STATE_DIR}/registered: {e}"))?;
    }
    Ok(gateway_id)
}

/// Retries for up to 30 minutes (ChirpStack may still be starting), then gives up; systemd starts it again later.
pub fn run(o: &Options) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(30 * 60);
    loop {
        match attempt(o) {
            Ok(_) => return Ok(()),
            Err(e) if Instant::now() >= deadline => return Err(e),
            Err(e) => {
                eprintln!("{e}; trying again in 30 s");
                thread::sleep(Duration::from_secs(30));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grpc_message_decoding() {
        assert_eq!(
            percent_decode("UNIQUE%20constraint%20failed:%20gateway.gateway_id"),
            "UNIQUE constraint failed: gateway.gateway_id"
        );
        assert_eq!(percent_decode("50%"), "50%");
        assert_eq!(percent_decode("%zz"), "%zz");
    }

    #[test]
    fn create_request_layout() {
        let req = create_gateway_request(
            "0016c001f150193b",
            "bobcat-g285",
            "52f14cd4-c6f1-4fbd-8f87-4025e1d49242",
        );
        let (field, gw) = Reader::new(&req).next().unwrap();
        assert_eq!(field, 1);
        let fields: Vec<_> = Reader::new(gw.as_bytes().unwrap()).collect();
        assert_eq!(fields[0].1.as_str(), Some("0016c001f150193b"));
        assert_eq!(fields[1].1.as_str(), Some("bobcat-g285"));
        assert_eq!(
            fields[3],
            (
                5,
                crate::pb::Value::Bytes(b"52f14cd4-c6f1-4fbd-8f87-4025e1d49242")
            )
        );
        assert_eq!(fields[4].1.as_u64(), Some(30));
    }
}
