<!-- SPDX-License-Identifier: MIT -->
# Changelog

## Unreleased

- Home Assistant: the panel's "Home Assistant (MQTT)" card bridges ChirpStack's application events from the gateway's
  local Mosquitto to another broker (Home Assistant's Mosquitto add-on), with a topic prefix, optional TLS and
  optional downlinks back; the gateway's own broker stays on localhost (docs/home-assistant.md). Tested on a board
  with Home Assistant's Mosquitto add-on: a node's uplinks arrive there, a downlink published there reaches ChirpStack.

- Fix: the first-boot registration of the gateway in the local ChirpStack (`lorawan-agent-register.service`) could not
  write ChirpStack's SQLite database under `ProtectSystem=strict` and retried forever; it now has
  `ReadWritePaths=/var/lib/chirpstack`. Checked on a board with a real node: join, uplinks, a downlink.

- System updates (A/B with RAUC): U-Boot chooses between `system_a` and `system_b` with a boot counter and falls back
  to the old slot; `lorawan-gateway-bundle` builds a signed update; the panel's "System update" card uploads and
  installs it (ChirpStack login). Keys per builder (`tools/rauc-keys.sh`). docs/updates.md.

- Community networks: `chirpstack-udp-forwarder` 4.3.1 forwards frames to The Things Network or any Semtech UDP
  server next to the local ChirpStack; servers chosen on the panel (card "Community networks", no login yet),
  off by default (docs/forwarding.md).

First version, written before the first board arrived. Builds; not booted yet.

- Machine `bobcat-g285` (Bobcat Miner 300, G285, RK3566) on meta-rockchip, Linux 6.18, U-Boot 2026.01.
- Mainline device tree `rk3566-bobcat-g285.dts`: power tree, eMMC, SD, Ethernet (RMII), SPI1 for the SX1302,
  Wi-Fi/Bluetooth, LEDs, RTC, USB gadget.
- `lorawan-os` distro and `lorawan-gateway-image`: ChirpStack 4.20 (SQLite), Concentratord 4.7.1 with a
  `bobcat_miner300_g285` model, MQTT Forwarder 4.6.1, Mosquitto, Redis.
- First-boot secret generation, key-only SSH, DHCP on Ethernet, hardware watchdog through systemd.
- `kas/qemuarm64.yml`: the image on QEMU with runtime tests (`testimage`) of the gateway stack, run after each
  build; `lorawan-test-access` (QEMU only) lets the test harness log in.
- Build fixes from the first full build: wrynose variable rename, linux-yocto `COMPATIBLE_MACHINE`, debug prefix
  map for C code vendored by crates (`buildpaths` QA), Yocto-side stripping of the MQTT Forwarder.
