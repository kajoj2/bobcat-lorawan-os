<!-- SPDX-License-Identifier: MIT -->
# lorawan-agent: status panel, diagnostics, LED, registration

One small Rust program (`lorawan-agent/`, no crate dependencies, about 0.8 MB) that makes the gateway easy to
check without logging in:

| What | How |
|---|---|
| Web panel | `http://<gateway>/` (port 80): health, gateway ID, radio counters, frames per hour over the last 24 h, the channel plan with RX/TX per channel, RX/TX per data rate, downlink results (OK, too late, duty cycle overflow, ...), EU868 duty-cycle use per band, the last frames heard (channel, data rate, coding rate, RSSI, SNR, DevAddr/DevEUI), services, network, system. Read only, no login; ChirpStack itself stays on port 8080. JSON at `/api/status`, the text report at `/api/status.txt`. |
| `lorawan-diag` | The same report in a terminal (over SSH or the serial console). Works without the agent running, minus the radio part. |
| Front LED | Green heartbeat: all fine. Blue: no network (no interface with a default route). Red: a gateway service is down, Mosquitto is unreachable, or the concentrator has sent no stats for 2 minutes. While starting (first 3 minutes) the device tree's green heartbeat stays. |
| System update | Card "System update" where the gateway runs as an A/B slot (RAUC): version and boot state of both slots, upload of a signed bundle with progress, install into the other slot, restart. Upload and restart need a ChirpStack login (admin/admin accepted, with a warning on the card). Details: docs/updates.md. |
| Community networks | Card "Community networks" on the panel: forward what the gateway hears to The Things Network (a cluster to tick) or any Semtech UDP server, next to the local ChirpStack; per server whether it answers, uplinks sent, downlinks; a warning while ChirpStack still has admin/admin. No login yet. Off by default. Details: docs/forwarding.md. |
| Home Assistant | Card "Home Assistant (MQTT)": a Mosquitto bridge that sends ChirpStack's application events (`application/#`, with a topic prefix, `chirpstack/` by default) to another broker such as Home Assistant's Mosquitto add-on, optionally taking downlink commands back; shows whether the bridge is connected. Saving needs a ChirpStack login (admin/admin accepted, with a warning). Details: docs/home-assistant.md. |
| eMMC page (Bobcat) | "Internal storage (eMMC)" on the panel, where the board's `bobcat-emmc` helper is installed: what is on the eMMC, backups of it as downloads (the essentials of a few hundred MB, or a full raw copy), and installing the running system from the SD card onto it. Downloads and the install need a ChirpStack login (admin/admin accepted, with a warning on the card). Details: docs/flashing.md, "Install onto the eMMC". |
| Setup wizard | `http://<gateway>/setup`: Wi-Fi from a scan list, community networks, eMMC install, a summary. Starts the setup access point (no network 2 min after boot, or the button held 5 s; held 15 s: factory reset), answers DNS on it so phones show the wizard as a captive portal. Details: docs/setup.md. |
| Devices | Card "Devices": every device the local ChirpStack hears, its last uplink, its usual interval, RSSI/SNR, and an alert when it goes **silent** (nothing for longer than the threshold, 60 min by default, and three of its usual intervals). Alerts are published retained on `lorawan/alerts/<DevEUI>`, passed to Home Assistant with a "problem" sensor per device, and optionally posted to a webhook (ntfy or any URL taking a plain text POST). Settings need a ChirpStack login. |
| Updates from GitHub | In the "System update" card: the project's releases with a bundle for this machine, whether one is newer than the running system, and "Install" (downloads the bundle onto the gateway, then installs it like an upload; ChirpStack login). Docs: docs/updates.md. |
| Backup | Card "Backup": one file with ChirpStack's database and the gateway's settings (Wi-Fi, community networks, Home Assistant, alerts, remote access, SSH keys, firewall rules) to download, and restoring one (then the gateway restarts). ChirpStack login. |
| Metrics | `http://<gateway>/metrics` for Prometheus: radio counters per frequency, data rate and result, duty cycle, each device's last uplink and signal, service states, system numbers. ChirpStack's own metrics at `/metrics/chirpstack`. No login (read only, like the panel). |
| Remote access | Card "Remote access (WireGuard)": a tunnel to your own WireGuard server so the gateway is reachable behind NAT (docs/remote-access.md). ChirpStack login. |
| BLE sensors | Card "BLE sensors": BTHome, Xiaomi (pvvx/ATC firmware, unencrypted MiBeacon) and RuuviTag readings heard over Bluetooth LE, published on `lorawan/ble/<MAC>` with Home Assistant discovery (docs/ble.md). Off by default; switching needs a ChirpStack login. |
| SSH keys | Card "SSH keys": public keys allowed to log in as root (`/var/lib/lorawan-ssh/authorized_keys`, read by sshd next to the image's own keys): add one by pasting a `.pub` line, list, remove. Plain keys only (no `command=` or other options). This is how a gateway flashed from a release image gets a shell. The count is public, the list and changes need a ChirpStack login. |
| ChirpStack registration | On the first boot the gateway adds itself to the local ChirpStack (tenant "ChirpStack", name = host name, stats every 30 s), so it appears under Gateways without typing its EUI. |

## How it works

- **Radio**: subscribes to the local Mosquitto (`+/gateway/+/event/+`, `+/gateway/+/state/+`), where the MQTT
  Forwarder publishes Concentratord's stats every 30 s, every uplink and its connection state. The protobuf
  messages are decoded by hand (field numbers from chirpstack_api 4.19). Counters are since the agent started;
  the hourly history keeps 24 hours in memory (gone after a restart). The channel plan comes from
  `/etc/chirpstack-concentratord/channels.toml`.
- **Not available from this radio**: noise floor and spectral scan (they need an SX1261, the G285's card has only
  the SX1302 and two SX1250; checked again 2026-11-22 for a spectrum card on the panel: not possible on this board), the concentrator's temperature (no STTS751 on the card) and GPS time.
- **System**: /proc and /sys, plus `systemctl is-active`, `ip`, `df` and `iw`; the panel's snapshot is reused
  for 4 s, the LED is re-evaluated every 10 s from cheaper checks.
- **Registration** (`lorawan-agent-register.service`, as the `chirpstack` user with ChirpStack's environment): the
  gateway ID from the forwarder's retained connection state; an API key from `chirpstack create-api-key`, kept in
  `/var/lib/lorawan-agent/api-token` (0600); then ChirpStack's API over gRPC-web on port 8080 (the protocol its web
  interface uses): `TenantService/List`, `GatewayService/Get`, `GatewayService/Create`. When done it writes
  `/var/lib/lorawan-agent/registered` and does not run again; delete that file to repeat it. A gateway added by
  hand before is left as it is. It retries for 30 minutes, then systemd starts it again 5 minutes later.
- **System update**: `GET /api/update` (whether the system was started as an A/B slot, `rauc status
  --detailed` as RAUC's JSON, the install's state and percentage, whether ChirpStack still has admin/admin; no
  login), and behind the ChirpStack login (admin/admin accepted)
  `GET /api/update/login`, `POST /api/update/upload` (the bundle as the request body, streamed to
  `/var/lib/lorawan-update/update.raucb`, up to 1 GiB; then `rauc install` in the background, its progress lines
  feed the card; the file is deleted afterwards) and `POST /api/update/reboot`.
- **Community networks**: `GET /api/forward` (servers, their state from chirpstack-udp-forwarder's Prometheus
  counters on `127.0.0.1:9800`, the TTN presets, whether ChirpStack still has admin/admin) and `POST /api/forward`
  with `server=host:port` repeated (up to 4; none turns forwarding off). Neither needs a login for now. It writes `/var/lib/lorawan-forward/servers.toml` (the unit's `StateDirectory`) and restarts or
  stops `chirpstack-udp-forwarder.service`. While a server list exists, that service counts as a gateway service
  for the health and the LED.
- **Home Assistant**: `GET /api/bridge` (settings without the password, the bridge state from Mosquitto's
  notifications on the local topic `lorawan-agent/bridge/state`; no login), and behind a login `GET /api/bridge/login`
  and `POST /api/bridge/save` (form: `server`, `username`, `password` (empty keeps the stored one), `prefix`,
  `tls=on`, `downlinks=on`, or `action=off`). It writes `/var/lib/lorawan-mqtt-bridge/home-assistant.conf` (0640,
  Mosquitto's group), which Mosquitto includes, and restarts `mosquitto.service`. docs/home-assistant.md.
- **eMMC page**: `GET /api/storage` (state, from `bobcat-emmc info`, whether ChirpStack still has admin/admin; no
  login), and behind a login
  `GET /api/storage/login`, `GET /api/storage/backup/<item>` (streams `bobcat-emmc backup <item>` with a file
  name) and `POST /api/storage/install` with `confirm=ERASE` (starts `bobcat-emmc-install.service`, so the install
  does not depend on the panel or the request). The login is HTTP Basic with a ChirpStack user's email and
  password, checked with ChirpStack's own `InternalService/Login`; a good login is remembered for 10 minutes, a
  wrong one waits 2 s. Over plain HTTP, like ChirpStack's own interface on port 8080: use it on your LAN only.
- The ChirpStack login stays `admin` / `admin` until you change it in the web interface (docs/flashing.md). The
  panel accepts it too (owner's choice): the update, eMMC and community network cards warn while it is the default,
  since anyone on the LAN knows it.

- **Devices** (`devices.rs`): a second MQTT connection subscribes to ChirpStack's `application/+/device/+/event/up`
  (JSON). The list and the settings are in `/var/lib/lorawan-alerts` (`devices.tsv`, saved every 5 minutes;
  `settings`). The usual interval is smoothed over the uplinks (bursts closer than 10 s do not count). Checked every
  minute. `GET /api/devices`, behind a login `POST /api/devices/save` (`threshold_min`, `webhook`, `webhook_clear=on`,
  or `forget=<DevEUI>`) and `POST /api/devices/test`. The webhook is called with `curl`: a POST with a plain text
  body and `Title`/`Tags` headers (what ntfy reads).
- **Updates from GitHub** (`release.rs`): `GET /api/releases` (GitHub's release list through `curl`, cached for 10
  minutes; `POST /api/releases` checks again, at most once a minute), behind the update login
  `POST /api/update/download` with `tag=`. Only assets named `*-<machine>.raucb` under
  `https://github.com/<repo>/releases/download/` are offered. The repository is set at build time
  (`LORAWAN_RELEASE_REPO`, written to `/usr/share/lorawan-agent/release-repo`); empty turns it off.
- **Backup** (`backup.rs`, `lorawan-backup`): `lorawan-backup.service` writes the file (`sqlite3 .backup` of
  ChirpStack's database, the settings directories of `/var/lib`, a manifest) which the panel then sends;
  `POST /api/backup/restore` takes a file of up to 256 MiB, `lorawan-backup check` looks at every entry (only the
  known directories, no absolute paths, no `..`), and `lorawan-restore.service` stops the services, puts it in place
  and restarts the gateway.
- **Remote access** (`wireguard.rs`): docs/remote-access.md.
- **BLE sensors** (`ble.rs`): docs/ble.md.

## Commands

```
lorawan-agent serve [--listen [::]:80] [--broker 127.0.0.1:1883] [--api 127.0.0.1:8080]
lorawan-agent register [--api 127.0.0.1:8080] [--name NAME] [--gateway-id EUI]
lorawan-diag                      (= lorawan-agent diag [--agent 127.0.0.1:80])
lorawan-agent ble [--broker 127.0.0.1:1883]   BLE sensors to MQTT (lorawan-ble.service)
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
