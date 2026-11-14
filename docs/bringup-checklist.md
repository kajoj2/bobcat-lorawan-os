<!-- SPDX-License-Identifier: MIT -->
# Bring-up checklist (first board)

Everything the image assumes but nobody has seen on hardware yet, in the order it will show up. Tick items off in
the pull request that confirms them, and move the fact to "V" in [hardware-g285.md](hardware-g285.md).

## Before power-on

- [ ] Photos of the board (both sides) and of the concentrator card; note the SoC marking (RK3566 or RK3566T).
      An RK3566T would need `rk3566t.dtsi` (lower CPU OPPs).
- [ ] Find the debug console pads, measure 3.3 V, attach a USB-UART that does 1.5 Mbaud (CP2102N, CH343, FT232R).
- [ ] Boot the factory firmware once with the console attached and save the log (bootloader versions, boot order).
- [ ] Optional but recommended: back up the eMMC from a booted Linux on SD (`dd` of `mmcblk1`, `mmcblk1boot0/1`),
      off the board, checksummed. Keep it private: it holds the unit's keys.

## Boot chain

- [ ] SD card: does the factory eMMC loader hand over to our U-Boot (mainline 2026.01, sector 16384)? Expected
      console: the vendor SPL banner, then mainline U-Boot. If the vendor SPL rejects our FIT image, options are a
      vendor-compatible U-Boot image or making the boot ROM skip the eMMC (Maskrom); decide then.
- [ ] U-Boot finds `extlinux/extlinux.conf` on the root partition and starts the kernel.
- [ ] RAM size reported as 2 GB (DDR blob `rk3566_ddr_1056MHz_v1.23`).

## Kernel / device tree

- [ ] PMIC and regulators probe without errors (`dmesg | grep -i -e rk808 -e fan53555 -e regulator`).
- [ ] Compare the regulator voltages with the factory firmware (`/sys/kernel/debug/regulator/regulator_summary` on
      both).
- [ ] Ethernet: `end0` up, 100 Mb/s full duplex. If not, try GMAC1 pin group m1 in the DTS. Warm reboot five times:
      does the link come back each time? If not, add the YT8512B fix (PHY reset after the clock is up).
- [ ] `/dev/spidev1.0` exists; `/dev/gpiochip3` and `/dev/gpiochip4` are the gpio3/gpio4 banks
      (`gpiodetect`: labels gpio3 / gpio4).
- [ ] SX1302: `sx1302-chip-id -d /dev/spidev1.0` (after `gpioset` power on GPIO3_D2 and a reset pulse on GPIO4_C5)
      prints chip version 0x10 and the EUI.
- [ ] RTC: `i2cdetect -y 4` and `-y 2`: where is 0x51? Move the node if needed. `hwclock -r` works.
- [ ] LEDs: status heartbeats, power on.
- [ ] Wi-Fi: `wlan0` appears; which NVRAM file does `brcmfmac` ask for?
- [ ] Bluetooth: `hciconfig` / `bluetoothctl list`.
- [ ] Thermal: `/sys/class/thermal/thermal_zone*/temp` sensible under load.
- [ ] Rear micro-USB in gadget mode enumerates on a host (later: serial console + network gadget).

## Gateway

- [ ] `chirpstack-concentratord` starts, logs the gateway EUI, no SPI errors.
- [ ] `chirpstack-mqtt-forwarder` connects to both sides; `mosquitto_sub -t 'eu868/gateway/#' -v` shows stats.
- [ ] ChirpStack web interface on `http://<gateway>:8080`, login `admin` / `admin`, **change the password**.
- [ ] Register the gateway (its EUI), a device profile and one real sensor; uplinks arrive, a downlink is received.
- [ ] TX power: read the factory TX gain table (`global_conf.json` of the factory packet forwarder on the eMMC) and
      replace the Semtech reference values in the Concentratord model. Check RSSI offset against a known device.
- [ ] Survives a power cut during operation (SD card: journald in volatile mode? see open questions).
