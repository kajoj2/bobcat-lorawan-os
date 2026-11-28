<!-- SPDX-License-Identifier: MIT -->
# bobcat-lorawan-os

An open-source Yocto Linux image that turns a retired **Bobcat Miner 300** Helium hotspot into a self-contained
**LoRaWAN gateway with its own network server**: [ChirpStack](https://www.chirpstack.io/) runs on the box, so
sensors work on your LAN without any cloud service. Its MQTT integration feeds Home Assistant, Node-RED or anything
else that speaks MQTT.

> **Status: 1.0, running on hardware.** A G285 board runs it as a gateway: the radio receives and sends, a real
> LoRaWAN node joins the local ChirpStack, frames are forwarded to The Things Network, and the system installs onto
> the eMMC and updates itself A/B from the web panel, and a new gateway is set up from a phone. Releases (card
> image and signed update bundle) are on the [Releases](https://github.com/kajoj2/bobcat-lorawan-os/releases) page;
> or build it yourself. Still open: a card install
> reported from an untouched Bobcat (tested with Rockchip's own boot loader standing in for the factory one).
> What was checked on the board: [docs/bringup-checklist.md](docs/bringup-checklist.md);
> progress: [CHANGELOG.md](CHANGELOG.md).

This project is not affiliated with, endorsed by or supported by Bobcat or its manufacturer. "Bobcat" is used only
to say which hardware the image runs on.

## Hardware

| | |
|---|---|
| Supported | Bobcat Miner 300, board revision **G285** (serial number starts with `285`), EU868 |
| SoC | Rockchip RK3566, 4x Cortex-A55, 2 GB LPDDR4X, 64 GB eMMC |
| Radio | SX1302 + 2x SX1250 on a mini-PCIe style card (SPI) |
| Network | 100 Mbit Ethernet, BCM43430 Wi-Fi (WPA2/WPA3 client) and Bluetooth LE (BLE sensors to MQTT) |
| Planned | G280, G290/G295 (other RK3566 revisions), US915 |

Hardware notes, each fact with its source: [docs/hardware-g285.md](docs/hardware-g285.md).

## Quick start

You need a Bobcat Miner 300 **G285** (the serial number on the label starts with `285`), its 12 V supply, a microSD
card of 8 GB or more and a phone. Nothing is soldered or opened besides the card slot, and the factory system on the
eMMC stays as it is until you choose to replace it.

1. **Download** `lorawan-gateway-os-<version>-bobcat-g285.wic.xz` from the
   [latest release](https://github.com/kajoj2/bobcat-lorawan-os/releases/latest).
2. **Write it to the card** with [balenaEtcher](https://etcher.balena.io/) or Raspberry Pi Imager ("Use custom"), or
   `bmaptool` / `dd` ([docs/flashing.md](docs/flashing.md)).
3. **Boot**: card into the slot on the board, Ethernet cable if you have one, then power. The front LED turns red,
   then blinks green when the gateway is up (blue: no network yet).
4. **Set it up from a phone**: about two minutes after power-on a gateway without a network opens the Wi-Fi
   `bobcat-setup-XXXX`; join it and the setup wizard opens (or go to `http://192.168.4.1/setup`). On a cable, open
   `http://bobcat-g285.local/setup` from your LAN instead. The wizard sets Wi-Fi, The Things Network sharing and,
   optionally, the install onto the eMMC ([docs/setup.md](docs/setup.md)).
5. **Use it**: status panel `http://bobcat-g285.local/`, ChirpStack `http://bobcat-g285.local:8080` (admin / admin;
   change it there). Add your sensors in ChirpStack; their data is on MQTT and, with the panel's card, in Home
   Assistant. For a shell, add your SSH public key on the panel (card "SSH keys"), then `ssh root@bobcat-g285.local`.

Updates come from the panel (System update → From GitHub). Something not working:
[docs/troubleshooting.md](docs/troubleshooting.md). Using an AI coding agent to install or change it: point it at
[AGENTS.md](AGENTS.md).

## What is in the image

```
SX1302 ──SPI── ChirpStack Concentratord ──ZeroMQ── ChirpStack MQTT Forwarder ──┐
                                                                              Mosquitto (localhost)
                         web UI :8080, gRPC/REST API ── ChirpStack ───────────┘   │
                                                           │ SQLite, Redis        └─ MQTT integration → your apps
```

- **Linux 6.18** (linux-yocto) with a device tree for the board written for mainline, **U-Boot 2026.01**, TF-A (BL31)
  built from source.
- **ChirpStack 4.20** (SQLite backend, no PostgreSQL), **Concentratord 4.7.1**, **MQTT Forwarder 4.6.1**.
- **Web panel** on port 80 (`lorawan-agent`): health, radio statistics per channel and data rate, duty cycle, last
  frames, the front LED as a health light, and the cards below: [docs/lorawan-agent.md](docs/lorawan-agent.md).
- **Setup wizard from a phone**: a new gateway opens a setup Wi-Fi network; the wizard sets Wi-Fi, community
  networks and the eMMC install. The button held 15 s resets the gateway to its fresh state:
  [docs/setup.md](docs/setup.md).
- **A/B system updates** with RAUC from the web panel, falling back to the old system if the new one does not
  start: [docs/updates.md](docs/updates.md). Each builder signs with its own keys (`tools/rauc-keys.sh`).
- Optional, off by default: **UDP Forwarder 4.3.1** shares the gateway with [The Things Network](https://www.thethingsnetwork.org/)
  or another community network next to the local ChirpStack, set on the web panel: [docs/forwarding.md](docs/forwarding.md).
- Optional, off by default: a bridge from the gateway's MQTT broker to **Home Assistant**'s (or any other) broker,
  set on the web panel: [docs/home-assistant.md](docs/home-assistant.md).
- On the panel as well: updates straight from GitHub releases, backup and restore, alerts for devices that went
  silent (MQTT, Home Assistant, ntfy), Prometheus metrics at `/metrics`, remote access over WireGuard
  ([docs/remote-access.md](docs/remote-access.md)), BLE sensors to MQTT ([docs/ble.md](docs/ble.md)), and
  ChirpStack's device profile repository for adding devices by vendor and model.
- systemd, systemd-networkd (DHCP on Ethernet and Wi-Fi), OpenSSH (keys only), Avahi (`*.local`), hardware
  watchdog, GNU tools and bash (no busybox), zram swap.
- A **firewall** (nftables) that only lets in SSH, the panel, ChirpStack and mDNS ([SECURITY.md](SECURITY.md)); on
  the eMMC a **read-only system partition**, with settings, data and the journal on the data partition.
- Secrets (ChirpStack API secret) are generated on each gateway's first boot, never baked into the image.

The image boots from a **microSD card** and does not touch the eMMC: remove the card and the factory firmware boots
again. Write the card, insert it, power on, and set the gateway up from a phone ([docs/setup.md](docs/setup.md)).
Once it works for you, the web panel can back up the eMMC and install the system onto it (A/B layout, the card is no
longer needed): [docs/flashing.md](docs/flashing.md).

## Build

Requirements: a Linux x86-64 host with ~150 GB free disk space and [kas](https://kas.readthedocs.io/)
(`pip install kas`), or kas-container with Docker/Podman.

```sh
git clone https://github.com/kajoj2/bobcat-lorawan-os
cd bobcat-lorawan-os
kas build kas/bobcat-g285.yml          # or: kas-container build kas/bobcat-g285.yml
```

SSH keys can be added on the panel later (card "SSH keys"), or built into your image, e.g. with a small kas overlay:

```yaml
# my-keys.yml
header:
  version: 14
local_conf_header:
  ssh-keys: |
    LORAWAN_SSH_AUTHORIZED_KEYS = "ssh-ed25519 AAAA... me@laptop"
```

```sh
kas build kas/bobcat-g285.yml:my-keys.yml
```

Details: [docs/building.md](docs/building.md). Flashing and first boot: [docs/flashing.md](docs/flashing.md).

Without a board, `kas build kas/qemuarm64.yml` builds the same image for QEMU and runs the gateway's runtime
tests in it (everything but the radio): [docs/testing.md](docs/testing.md).

## Repository layout

| Path | What |
|---|---|
| `kas/` | Build configuration (`bobcat-g285.yml`, `qemuarm64.yml` for tests in QEMU, `debug-tools.yml` adds bring-up tools) and the pinned revisions of every layer (`*.lock.yml`) |
| `meta-bobcat/` | Board support: machine, device tree, kernel config |
| `meta-lorawan/` | Board-independent gateway: distro, ChirpStack stack, image, configuration, setup wizard's access point |
| `lorawan-agent/` | The web panel, setup wizard, LED status and diagnostics (Rust, no crate dependencies) |
| `docs/` | Hardware notes, architecture, decisions, bring-up checklist, troubleshooting |
| `AGENTS.md` | Instructions for AI coding agents: installing a gateway, checking it, and working on this repository |
| `tools/` | Device tree check, crates.inc generator and check, update signing keys (`rauc-keys.sh`), lock updates |
| `.github/workflows/` | Checks on every pull request, full builds, nightly lock updates, signed releases ([docs/releasing.md](docs/releasing.md)) |

## Contributing

Issues and pull requests are welcome, in particular from anyone with a G280, G290 or G295 board. Please read
[CONTRIBUTING.md](CONTRIBUTING.md) (commits need a `Signed-off-by` line).

## License

MIT for this repository ([LICENSE](LICENSE)); the device tree is `GPL-2.0-or-later OR MIT`, as in the Linux kernel.
Each file carries an SPDX header. The components the image builds (Linux, U-Boot, ChirpStack, the Semtech HAL,
Rockchip's DDR init binary from `rkbin`, ...) keep their own licenses; Yocto writes the full list into
the build's `tmp/deploy/licenses/`.

Hardware facts come from public community work, credited in [docs/hardware-g285.md](docs/hardware-g285.md).
