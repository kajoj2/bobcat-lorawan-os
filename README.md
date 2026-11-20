<!-- SPDX-License-Identifier: MIT -->
# bobcat-lorawan-os

An open-source Yocto Linux image that turns a retired **Bobcat Miner 300** Helium hotspot into a self-contained
**LoRaWAN gateway with its own network server**: [ChirpStack](https://www.chirpstack.io/) runs on the box, so
sensors work on your LAN without any cloud service. Its MQTT integration feeds Home Assistant, Node-RED or anything
else that speaks MQTT.

> **Status: pre-alpha, not yet booted on hardware.** The image builds (Yocto 6.0, 68 MB compressed); the first
> board is on its way. Do not flash it expecting a working gateway yet. See [docs/bringup-checklist.md](docs/bringup-checklist.md)
> for what has to be verified, and [CHANGELOG.md](CHANGELOG.md) for progress.

This project is not affiliated with, endorsed by or supported by Bobcat or its manufacturer. "Bobcat" is used only
to say which hardware the image runs on.

## Hardware

| | |
|---|---|
| Supported | Bobcat Miner 300, board revision **G285** (serial number starts with `285`), EU868 |
| SoC | Rockchip RK3566, 4x Cortex-A55, 2 GB LPDDR4X, 64 GB eMMC |
| Radio | SX1302 + 2x SX1250 on a mini-PCIe style card (SPI) |
| Network | 100 Mbit Ethernet; BCM43430 Wi-Fi/Bluetooth (not used yet) |
| Planned | G280, G290/G295 (other RK3566 revisions), US915 |

Hardware notes, each fact with its source: [docs/hardware-g285.md](docs/hardware-g285.md).

## What is in the image

```
SX1302 ──SPI── ChirpStack Concentratord ──ZeroMQ── ChirpStack MQTT Forwarder ──┐
                                                                              Mosquitto (localhost)
                         web UI :8080, gRPC/REST API ── ChirpStack ───────────┘   │
                                                           │ SQLite, Redis        └─ MQTT integration → your apps
```

- **Linux 6.18** (linux-yocto) with a device tree for the board written for mainline, **U-Boot 2026.01**.
- **ChirpStack 4.20** (SQLite backend, no PostgreSQL), **Concentratord 4.7.1**, **MQTT Forwarder 4.6.1**.
- Optional, off by default: **UDP Forwarder 4.3.1** shares the gateway with [The Things Network](https://www.thethingsnetwork.org/)
  or another community network next to the local ChirpStack, set on the web panel: [docs/forwarding.md](docs/forwarding.md).
- systemd, systemd-networkd (DHCP on Ethernet), OpenSSH (keys only), Avahi (`*.local`).
- Secrets (ChirpStack API secret) are generated on each gateway's first boot, never baked into the image.

The image boots from a **microSD card** and does not touch the eMMC: remove the card and the factory firmware
boots again.

## Build

Requirements: a Linux x86-64 host with ~150 GB free disk space and [kas](https://kas.readthedocs.io/)
(`pip install kas`), or kas-container with Docker/Podman.

```sh
git clone https://github.com/kajoj2/bobcat-lorawan-os
cd bobcat-lorawan-os
kas build kas/bobcat-g285.yml          # or: kas-container build kas/bobcat-g285.yml
```

To log in over SSH, add your public key before building, e.g. in a small kas overlay:

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
| `meta-lorawan/` | Board-independent gateway: distro, ChirpStack stack, image, configuration |
| `docs/` | Hardware notes, architecture, decisions, bring-up checklist |
| `tools/` | Device tree check, crates.inc generator |

## Contributing

Issues and pull requests are welcome, in particular from anyone with a G280, G290 or G295 board. Please read
[CONTRIBUTING.md](CONTRIBUTING.md) (commits need a `Signed-off-by` line).

## License

MIT for this repository ([LICENSE](LICENSE)); the device tree is `GPL-2.0-or-later OR MIT`, as in the Linux kernel.
Each file carries an SPDX header. The components the image builds (Linux, U-Boot, ChirpStack, the Semtech HAL,
Rockchip's DDR init and TF-A binaries from `rkbin`, ...) keep their own licenses; Yocto writes the full list into
the build's `tmp/deploy/licenses/`.

Hardware facts come from public community work, credited in [docs/hardware-g285.md](docs/hardware-g285.md).
