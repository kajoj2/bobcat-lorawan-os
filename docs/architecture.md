<!-- SPDX-License-Identifier: MIT -->
# Architecture and decisions

## Layers

- **meta-bobcat**: everything specific to the hardware. Machine `bobcat-g285` (on top of `meta-rockchip`'s RK3566
  support), the board's device tree (mainline style, kept as a plain `.dts` file), a kernel config fragment, and the
  name of the board's concentrator model (`LORAWAN_CONCENTRATOR_MODEL`). New board revisions are new machines here.
- **meta-lorawan**: everything a LoRaWAN gateway needs, independent of the board. The `lorawan-os` distro, the
  ChirpStack stack, `lorawan-gateway-config` (glue configuration, first-boot secrets) and `lorawan-gateway-image`.
  It could serve any SX1302 board that Concentratord supports.

## Data flow

```
SX1302 card ─SPI─ chirpstack-concentratord ─ZeroMQ (/run/chirpstack-concentratord)─ chirpstack-mqtt-forwarder
                                                                                        │ MQTT eu868/gateway/...
                                                                     mosquitto (127.0.0.1:1883)
                                                                                        │
                       web UI / API :8080 ─ chirpstack (SQLite /var/lib/chirpstack, Redis 127.0.0.1)
                                                                                        │ MQTT application/...
                                                                             your integrations
```

## Decisions

Short records; newest last. Each says what was chosen and why, so a later change can see what it overturns.

1. **Yocto 6.0 "wrynose" (LTS)** with kas, pinned by a lock file. Long support window, and the same release the
   maintainer's other projects build on. `meta-rockchip` has no wrynose branch; the lock pins the last master commit
   compatible with wrynose (`b6bc140`, before the 2026-09-25 switch to blacksail).
2. **Mainline everything**: linux-yocto 6.18, U-Boot 2026.01, a device tree written for mainline. No vendor 4.19
   kernel. The device tree can go upstream later; the boot loader uses U-Boot's `generic-rk3568` configuration, so
   no U-Boot board file is needed to start. The DDR init and BL31 binaries come from Rockchip's `rkbin` (fetched by
   `meta-rockchip`, never committed here).
3. **Boot from microSD, eMMC untouched.** Bring-up is safe and reversible: removing the card restores the factory
   firmware. Installing to eMMC is a later, optional step.
4. **ChirpStack on the gateway** (not a cloud LNS, not a separate server). Works without internet; sensors reach
   Home Assistant over MQTT. SQLite backend instead of PostgreSQL (as ChirpStack Gateway OS does): less RAM, nothing
   to administer.
5. **Concentratord instead of Semtech's packet forwarder.** It handles the SX1302 reset/power GPIOs itself through
   the GPIO character device (no sysfs scripts), and the board becomes a small model definition
   (`bobcat_miner300_g285`, a patch meant for upstream). Its HAL fork already copes with the missing temperature
   sensor, so the HAL patch other Bobcat projects carry is not needed.
6. **Socket rail as a device tree regulator.** GPIO3_D5 (factory: set once at boot) is an always-on fixed
   regulator; GPIO3_D2 (cycled by the factory reset script) and the SX1302 reset GPIO4_C5 are left to Concentratord.
7. **Secure defaults for an image strangers will flash**: secrets generated on the first boot, SSH keys only,
   Mosquitto and Redis on localhost, root without a password only on the local consoles.
8. **Licensing**: MIT for the layers and scripts, `GPL-2.0-or-later OR MIT` for the device tree (kernel
   convention, upstreamable). REUSE-compliant headers. Only facts are taken from sources without a license.
9. **Hardware watchdog through systemd** (`RuntimeWatchdogSec=30s`): an unattended gateway reboots itself when
   the kernel or PID 1 hangs. RK3566's `dw_wdt` is in the kernel config and enabled in the SoC device tree.
10. **GNU tools instead of busybox** (`packagegroup-lorawan-base-utils`): bash, coreutils, util-linux, procps,
    findutils, gawk, iproute2, curl. A few MB more, and scripts and people on a shell get the commands and
    options they expect; busybox's missing `base64` once left ChirpStack's API secret empty.
11. **Read-only system on the eMMC**: an A/B slot remounts its root read-only early in the boot; settings, data,
    the Wi-Fi network and the journal live on the data partition. A slot is always what its signed bundle held.
    The SD card's system stays writable (install and rescue), and keeps its journal in RAM.
12. **A firewall by default** (nftables, `lorawan-firewall`): only SSH, the panel, ChirpStack's port and mDNS are
    open; a gateway often ends up on networks its owner does not control. Extra rules on the data partition.
13. **Compressed swap in RAM** (zram, half the memory, zstd) instead of no swap: ChirpStack, Redis and Mosquitto
    grow with the number of devices, and 2 GB with no swap meant the OOM killer as the first answer.

## Open questions

- Answered (2026-11-17, 2026-11-20): the factory eMMC loader starts our U-Boot from the card when the card's GPT has
  an entry named `uboot` first (`meta-bobcat/files/wic/bobcat-g285.wks`); meta-rockchip's layout made it run out of memory. So the
  card works on an unmodified board (docs/flashing.md).
- Boot chain: SPL and U-Boot are built from mainline, the DDR init (TPL) and BL31 are rkbin blobs. Replace BL31
  with upstream TF-A if its RK3566/RK3568 support is complete enough, which leaves the DDR blob as the only closed
  part (there is no open DDR init for RK356x).
- A U-Boot configuration for the board itself instead of `generic-rk3568_defconfig`: Ethernet (network boot),
  LEDs during boot, the SARADC button.
- Gadget mode on the micro-USB port (`dwc3: failed to enable ep0out`).
- YT8512B warm-reboot link problem: still there with the PHY reset in the device tree? If yes, kernel fix
  (`PHY_RST_AFTER_CLK_EN`) or a oneshot service.
- SD card wear: answered (2026-11-22). From the card: logs in RAM, `noatime`, weekly `fstrim`. On the eMMC: a data
  partition and a read-only system (decision 11).
- Updates: decided 2026-11-20, whole-image A/B with RAUC, installed from the panel behind the ChirpStack login
  (docs/updates.md).
- ChirpStack web interface build needs the network (pnpm); make it offline (vendored store or Yocto's npm fetcher).
- A first-boot way to set the ChirpStack admin password and open MQTT to the LAN with credentials.
- The kernel version ends in `-dirty` because the DTS is copied into the kernel's git tree; carry it as a patch or
  set `LOCALVERSION`.
