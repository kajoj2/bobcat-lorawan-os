<!-- SPDX-License-Identifier: MIT -->
# Bring-up checklist (first board)

Everything the image assumes but nobody has seen on hardware yet, in the order it will show up. Tick items off in
the pull request that confirms them, and move the fact to "V" in [hardware-g285.md](hardware-g285.md).

## Before power-on

- [ ] Photos of the board (both sides) and of the concentrator card; note the SoC marking (RK3566 or RK3566T).
      An RK3566T would need `rk3566t.dtsi` (lower CPU OPPs).
- [x] Find the debug console pads, measure 3.3 V, attach a USB-UART that does 1.5 Mbaud (CP2102N, CH343, FT232R).
      Done 2026-11-17: FTDI FT2232H at 1500000 baud; the TX lead needed soldering.
- [x] Boot the factory firmware once with the console attached and save the log (bootloader versions, boot order).
      Done 2026-11-17.
- [x] Optional but recommended: back up the eMMC from a booted Linux on SD (`dd` of `mmcblk1`, `mmcblk1boot0/1`),
      off the board, checksummed. Keep it private: it holds the unit's keys.
      Done 2026-11-18: on this board the eMMC is `mmcblk0`; image checked by sha256 against the device.

## Boot chain

- [x] SD card: does the factory eMMC loader hand over to our U-Boot (mainline 2026.01, sector 16384)? Expected
      console: the vendor SPL banner, then mainline U-Boot. If the vendor SPL rejects our FIT image, options are a
      vendor-compatible U-Boot image or making the boot ROM skip the eMMC (Maskrom); decide then.
      2026-11-17: no, the factory SPL fails on our card; the board boots the card once the eMMC loader area is cleared (docs/flashing.md).
      2026-11-20: cause found (no GPT entry named `uboot`, the SPL runs out of heap looking for it); with
      `meta-bobcat/files/wic/bobcat-g285.wks` Rockchip's SPL (rkbin v1.14, standing in for the factory one) starts our U-Boot, TF-A and Linux.
- [x] U-Boot finds `extlinux/extlinux.conf` on the root partition and starts the kernel.
- [x] RAM size reported as 2 GB (DDR blob `rk3566_ddr_1056MHz_v1.23`).

## Kernel / device tree

- [x] PMIC and regulators probe without errors (`dmesg | grep -i -e rk808 -e fan53555 -e regulator`).
      2026-11-21: no errors; TCS4525 detected as FAN53555 option 12, all RK809 rails in `regulator_summary`.
- [x] Compare the regulator voltages with the factory firmware (`/sys/kernel/debug/regulator/regulator_summary` on
      both).
      2026-11-21: against the factory DTB from the eMMC backup (the factory system is no longer on the board): same
      rail names and voltages; the factory kernel keeps every rail always on, mainline turns off the unused ones.
- [x] Ethernet: `end0` up, 100 Mb/s full duplex. If not, try GMAC1 pin group m1 in the DTS. Warm reboot five times:
      does the link come back each time? If not, add the YT8512B fix (PHY reset after the clock is up).
      2026-11-18: `eth0`, 100 Mb/s full duplex with the YT8512B driver patch, which also stops the PHY advertising Extended Next Page (autonegotiation stalled with it). 2026-11-21: the link comes back after warm reboots.
- [x] `/dev/spidev1.0` exists; `/dev/gpiochip3` and `/dev/gpiochip4` are the gpio3/gpio4 banks
      (`gpiodetect`: labels gpio3 / gpio4).
- [x] SX1302: `sx1302-chip-id -d /dev/spidev1.0` (after `gpioset` power on GPIO3_D2 and a reset pulse on GPIO4_C5)
      prints chip version 0x10 and the EUI.
      2026-11-18: checked through Concentratord instead (starts, reads the gateway EUI, receives frames).
- [x] RTC: `i2cdetect -y 4` and `-y 2`: where is 0x51? Move the node if needed. `hwclock -r` works.
      2026-11-18: 0x51 on i2c4, bound by the driver; `hwclock -r` reads the time (in step with NTP).
- [x] LEDs: status heartbeats, power on.
      2026-11-18: the only LED is the front RGB one (GPIO4_A0 green, A1 blue, A2 red), lit line by line; in the device tree as green heartbeat (first in the image after dad3a90).
- [x] Wi-Fi: `wlan0` appears; which NVRAM file does `brcmfmac` ask for?
      2026-11-18: `brcm/brcmfmac43430-sdio.bobcat,miner300-g285.txt`; joins a WPA2/WPA3 network with the image as built.
- [x] Bluetooth: `hciconfig` / `bluetoothctl list`.
      2026-11-18: controller up, `BCM43430A1.hcd` patch loaded.
- [x] Thermal: `/sys/class/thermal/thermal_zone*/temp` sensible under load.
      2026-11-17: 43 °C at full load on 4 cores at 1.8 GHz, no throttling.
- [x] Rear micro-USB in gadget mode enumerates on a host (later: serial console + network gadget).
      2026-11-17: not yet; dwc3 reports "failed to enable ep0out" when a gadget is bound. 2026-11-21: works.

## Gateway

- [x] `chirpstack-concentratord` starts, logs the gateway EUI, no SPI errors.
- [x] `chirpstack-mqtt-forwarder` connects to both sides; `mosquitto_sub -t 'eu868/gateway/#' -v` shows stats.
- [ ] ChirpStack web interface on `http://<gateway>:8080`, login `admin` / `admin`, **change the password**.
- [x] Register the gateway (its EUI), a device profile and one real sensor; uplinks arrive, a downlink is received.
      2026-11-18: receive path checked with a LoRa radio set to 868.1 MHz, SF8, private sync word: frames at -25 dBm, SNR 15 dB, CRC OK. No real LoRaWAN sensor yet.
      2026-11-21: the gateway registered itself (after the `lorawan-agent-register` fix below); an nRF52840 + SX1262
      node (Zephyr, LoRaWAN 1.0.4, OTAA) joined, uplinks decoded by a JavaScript codec (DR5, -59 dBm, SNR 13.8 dB),
      a queued downlink sent with ADR's LinkADRReq and acknowledged by the node in its next uplink. Found on the way:
      `lorawan-agent-register` could not write ChirpStack's database (`ProtectSystem=strict`), so it never
      registered the gateway; fixed with `ReadWritePaths=/var/lib/chirpstack`.
- [x] TX power: read the factory TX gain table (`global_conf.json` of the factory packet forwarder on the eMMC) and
      replace the Semtech reference values in the Concentratord model. Check RSSI offset against a known device.
      2026-11-17: factory table in the Concentratord model patch.
- [ ] Survives a power cut during operation (SD card: journald in volatile mode? see open questions).
