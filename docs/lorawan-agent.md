<!-- SPDX-License-Identifier: MIT -->
# lorawan-agent: status panel, diagnostics, LED, registration

One small Rust program (`lorawan-agent/`, no crate dependencies, about 0.8 MB) that makes the gateway easy to
check without logging in:

| What | How |
|---|---|
| Web panel | `http://<gateway>/` (port 80): health, gateway ID, radio counters, frames per hour over the last 24 h, the channel plan with RX/TX per channel, RX/TX per data rate, downlink results (OK, too late, duty cycle overflow, ...), EU868 duty-cycle use per band, the last frames heard (channel, data rate, coding rate, RSSI, SNR, DevAddr/DevEUI), services, network, system. Read only, no login; ChirpStack itself stays on port 8080. JSON at `/api/status`, the text report at `/api/status.txt`. |
| `lorawan-diag` | The same report in a terminal (over SSH or the serial console). Works without the agent running, minus the radio part. |
| Front LED | Green heartbeat: all fine. Blue: no network (no interface with a default route). Red: a gateway service is down, Mosquitto is unreachable, or the concentrator has sent no stats for 2 minutes. While starting (first 3 minutes) the device tree's green heartbeat stays. |
| ChirpStack registration | On the first boot the gateway adds itself to the local ChirpStack (tenant "ChirpStack", name = host name, stats every 30 s), so it appears under Gateways without typing its EUI. |

## How it works

- **Radio**: subscribes to the local Mosquitto (`+/gateway/+/event/+`, `+/gateway/+/state/+`), where the MQTT
  Forwarder publishes Concentratord's stats every 30 s, every uplink and its connection state. The protobuf
  messages are decoded by hand (field numbers from chirpstack_api 4.19). Counters are since the agent started;
  the hourly history keeps 24 hours in memory (gone after a restart). The channel plan comes from
  `/etc/chirpstack-concentratord/channels.toml`.
- **Not available from this radio**: noise floor and spectral scan (they need an SX1261, the G285's card has only
  the SX1302 and two SX1250), the concentrator's temperature (no STTS751 on the card) and GPS time.
- **System**: /proc and /sys, plus `systemctl is-active`, `ip`, `df` and `wpa_cli`; the panel's snapshot is reused
  for 4 s, the LED is re-evaluated every 10 s from cheaper checks.
- **Registration** (`lorawan-agent-register.service`, as the `chirpstack` user with ChirpStack's environment): the
  gateway ID from the forwarder's retained connection state; an API key from `chirpstack create-api-key`, kept in
  `/var/lib/lorawan-agent/api-token` (0600); then ChirpStack's API over gRPC-web on port 8080 (the protocol its web
  interface uses): `TenantService/List`, `GatewayService/Get`, `GatewayService/Create`. When done it writes
  `/var/lib/lorawan-agent/registered` and does not run again; delete that file to repeat it. A gateway added by
  hand before is left as it is. It retries for 30 minutes, then systemd starts it again 5 minutes later.
- The ChirpStack login stays `admin` / `admin` (change it in the web interface, docs/flashing.md).

## Commands

```
lorawan-agent serve [--listen [::]:80] [--broker 127.0.0.1:1883]
lorawan-agent register [--api 127.0.0.1:8080] [--name NAME] [--gateway-id EUI]
lorawan-diag                      (= lorawan-agent diag [--agent 127.0.0.1:80])
lorawan-agent led-reset           LED back to green heartbeat (run when the service stops)
```

## Development

`cargo test` and `cargo clippy` run on any host (CI does both). A static binary for a quick try on the board,
without a Yocto build (`rustup target add aarch64-unknown-linux-musl`):

```
cargo build --release --target aarch64-unknown-linux-musl \
  --config 'target.aarch64-unknown-linux-musl.linker="rust-lld"'
```

Copy it to `/tmp` on the board and run `serve --listen [::]:8081` next to the installed one.
