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

## Open questions

- Does the factory eMMC loader start our mainline U-Boot from the SD card? (first boot)
- YT8512B warm-reboot link problem: still there with the PHY reset in the device tree? If yes, kernel fix
  (`PHY_RST_AFTER_CLK_EN`) or a oneshot service.
- SD card wear: volatile journald and `noatime` by default? Read-only root with an overlay?
- Updates: whole-image A/B (RAUC, `meta-rockchip` has a demo) or package feed?
- ChirpStack web interface build needs the network (pnpm); make it offline (vendored store or Yocto's npm fetcher).
- A first-boot way to set the ChirpStack admin password and open MQTT to the LAN with credentials.
