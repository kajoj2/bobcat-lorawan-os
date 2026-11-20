// SPDX-License-Identifier: MIT
//! lorawan-agent: the gateway's own status service.
//!
//!   lorawan-agent serve      web panel on port 80 (with the forwarding, update and eMMC pages), LED status
//!                            (lorawan-agent.service)
//!   lorawan-agent register   add this gateway to the local ChirpStack (lorawan-agent-register.service)
//!   lorawan-agent diag       print the status report (also installed as `lorawan-diag`)
//!   lorawan-agent led-reset  give the LED back to the device tree's default (green heartbeat)

mod auth;
mod forward;
mod http;
mod led;
mod mqtt;
mod pb;
mod radio;
mod register;
mod status;
mod storage;
mod sys;
mod update;

use http::{Request, Response};
use radio::{now, Radio};
use status::{health, Level, Status};
use std::env;
use std::process::ExitCode;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

const VERSION: &str = env!("CARGO_PKG_VERSION");
const INDEX: &str = include_str!("index.html");
const DEFAULT_BROKER: &str = "127.0.0.1:1883";
const DEFAULT_LISTEN: &str = "[::]:80";

const USAGE: &str = "usage: lorawan-agent <serve|register|diag|led-reset> [options]
  serve      [--listen ADDR:PORT] [--broker ADDR:PORT] [--api ADDR:PORT]   web panel and LED status
  register   [--broker ADDR:PORT] [--api ADDR:PORT] [--chirpstack-config DIR] [--name NAME]
             [--gateway-id EUI]  (another gateway; does not mark this one as registered)
  diag       [--agent ADDR:PORT]                          status report (same as lorawan-diag)
  led-reset                                               LED back to green heartbeat
";

fn opt(args: &[String], name: &str, default: &str) -> String {
    args.windows(2)
        .find(|w| w[0] == name)
        .map(|w| w[1].clone())
        .unwrap_or_else(|| default.to_string())
}

/// System snapshots run a few processes; the panel polls every few seconds, so keep one for a while.
struct Cache {
    at: Option<Instant>,
    system: sys::System,
}

fn build_status(radio: &Radio, system: sys::System, started: u64, agent_running: bool) -> Status {
    let network = system.ifaces.iter().any(|i| i.up && i.default_route);
    let t = now();
    Status {
        now: t,
        agent_version: VERSION,
        health: health(radio, &system.services, network, started, t),
        radio: radio.clone(),
        system,
        registered: status::registered_gateway(),
        channels: sys::channel_plan(),
        agent_running,
    }
}

fn serve(args: &[String]) -> ExitCode {
    let listen = opt(args, "--listen", DEFAULT_LISTEN);
    let broker = opt(args, "--broker", DEFAULT_BROKER);
    let api = opt(args, "--api", "127.0.0.1:8080");
    let started = now();
    let shared: radio::Shared = Arc::new(Mutex::new(Radio::default()));
    radio::spawn(shared.clone(), broker);

    // LED: re-evaluated every 10 s from cheap checks (one systemctl call, /sys and /proc reads).
    {
        let shared = shared.clone();
        thread::spawn(move || {
            let mut led = led::Led::new();
            if !led.present() {
                eprintln!("no red:/green:/blue: LEDs in /sys/class/leds, LED status off");
                return;
            }
            loop {
                let services = sys::service_states(&sys::units());
                let h = health(
                    &shared.lock().unwrap(),
                    &services,
                    sys::network_ok(),
                    started,
                    now(),
                );
                led.show(h.level);
                thread::sleep(Duration::from_secs(10));
            }
        });
    }

    let listener = match http::bind(&listen) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("cannot listen on {listen}: {e}");
            return ExitCode::FAILURE;
        }
    };
    eprintln!("lorawan-agent {VERSION}: panel on {listen}");
    let cache = Arc::new(Mutex::new(Cache {
        at: None,
        system: sys::System::default(),
    }));
    let auth = auth::Auth::new(&api);
    let tracker = forward::Tracker::default();
    let updating: update::Shared = Default::default();
    let route = move |req: &Request| -> Response {
        let status = || {
            let system = {
                let mut c = cache.lock().unwrap();
                if c.at.is_none_or(|t| t.elapsed() > Duration::from_secs(4)) {
                    c.system = sys::snapshot();
                    c.at = Some(Instant::now());
                }
                c.system.clone()
            };
            let radio = shared.lock().unwrap().clone();
            build_status(&radio, system, started, true)
        };
        let get = req.method != "POST";
        match (get, req.path.as_str()) {
            (true, "/" | "/index.html") => Response::new(200, "text/html; charset=utf-8", INDEX),
            (true, "/api/status") => Response::new(200, "application/json", status().to_json()),
            (true, "/api/status.txt") => {
                Response::new(200, "text/plain; charset=utf-8", status().to_text())
            }
            (true, "/api/forward") => Response::new(
                200,
                "application/json",
                forward::info_json(&tracker, auth.default_password_valid().ok()),
            ),
            // No login for now (owner's choice): the card only warns while ChirpStack has admin/admin.
            (false, "/api/forward") => forward::save(&req.body),
            (true, "/api/update") => {
                Response::new(200, "application/json", update::info_json(&updating))
            }
            // Installing a system is full control over the gateway: always behind the ChirpStack login.
            (_, p) if p.starts_with("/api/update/") => {
                if let Some(denied) = login_required(&auth, req) {
                    return denied;
                }
                match (get, &p["/api/update/".len()..]) {
                    (true, "login") => Response::new(200, "text/plain", "ok\n"),
                    (false, "upload") => update::upload(req, &updating),
                    (false, "reboot") => update::reboot(&updating),
                    _ => Response::new(404, "text/plain", "not found\n"),
                }
            }
            (true, "/api/storage") => Response::new(200, "application/json", storage::info_json()),
            (_, p) if p.starts_with("/api/storage/") => {
                if let Some(denied) = login_required(&auth, req) {
                    return denied;
                }
                match (get, &p["/api/storage/".len()..]) {
                    (true, "login") => Response::new(200, "text/plain", "ok\n"),
                    (true, item) if item.starts_with("backup/") => {
                        storage::backup(&item["backup/".len()..])
                    }
                    (false, "install") => storage::start_install(&req.body),
                    _ => Response::new(404, "text/plain", "not found\n"),
                }
            }
            (false, _) => Response::new(405, "text/plain", "method not allowed\n"),
            _ => Response::new(404, "text/plain", "not found\n"),
        }
    };
    http::serve(listener, Arc::new(route))
}

/// None when the request may go on, else the refusal to send.
fn login_required(auth: &auth::Auth, req: &Request) -> Option<Response> {
    match auth.check(req) {
        auth::Check::Ok => None,
        auth::Check::Unauthorized => Some(
            Response::new(401, "text/plain", "log in with a ChirpStack user (email and password)\n")
                .with_header("WWW-Authenticate", "Basic realm=\"Gateway: ChirpStack login\", charset=\"UTF-8\""),
        ),
        auth::Check::DefaultPassword => Some(Response::new(
            403,
            "text/plain",
            "ChirpStack still has its default password (admin/admin). Change it in the ChirpStack web interface \
             (port 8080) first: these actions are protected by that login.\n",
        )),
        auth::Check::Unavailable(e) => Some(Response::new(
            503,
            "text/plain",
            format!("cannot check the login, ChirpStack does not answer: {e}\n"),
        )),
    }
}

fn diag(args: &[String]) -> ExitCode {
    let agent = opt(args, "--agent", "127.0.0.1:80");
    match http::request(
        &agent,
        "GET",
        "/api/status.txt",
        &[],
        b"",
        Duration::from_secs(10),
    ) {
        Ok(r) if r.status == 200 => print!("{}", String::from_utf8_lossy(&r.body)),
        _ => {
            // Agent not running: report what can be seen without it.
            let s = build_status(&Radio::default(), sys::snapshot(), 0, false);
            print!("{}", s.to_text());
        }
    }
    ExitCode::SUCCESS
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    let invoked_as = args
        .first()
        .and_then(|a| a.rsplit('/').next())
        .unwrap_or_default();
    let (cmd, rest) = if invoked_as == "lorawan-diag" {
        ("diag", &args[1..])
    } else {
        (
            args.get(1).map(String::as_str).unwrap_or(""),
            args.get(2..).unwrap_or_default(),
        )
    };
    match cmd {
        "serve" => serve(rest),
        "diag" => diag(rest),
        "register" => {
            let o = register::Options {
                broker: opt(rest, "--broker", DEFAULT_BROKER),
                api: opt(rest, "--api", "127.0.0.1:8080"),
                chirpstack_config: opt(rest, "--chirpstack-config", "/etc/chirpstack"),
                name: rest
                    .windows(2)
                    .find(|w| w[0] == "--name")
                    .map(|w| w[1].clone()),
                gateway_id: rest
                    .windows(2)
                    .find(|w| w[0] == "--gateway-id")
                    .map(|w| w[1].clone()),
            };
            match register::run(&o) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("{e}");
                    ExitCode::FAILURE
                }
            }
        }
        "led-reset" => {
            led::Led::new().show(Level::Ok);
            ExitCode::SUCCESS
        }
        "--version" | "-V" => {
            println!("lorawan-agent {VERSION}");
            ExitCode::SUCCESS
        }
        _ => {
            eprint!("{USAGE}");
            ExitCode::from(2)
        }
    }
}
