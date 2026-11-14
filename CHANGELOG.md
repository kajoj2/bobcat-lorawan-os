<!-- SPDX-License-Identifier: MIT -->
# Changelog

## Unreleased

First version, written before the first board arrived; not booted yet.

- Machine `bobcat-g285` (Bobcat Miner 300, G285, RK3566) on meta-rockchip, Linux 6.18, U-Boot 2026.01.
- Mainline device tree `rk3566-bobcat-g285.dts`: power tree, eMMC, SD, Ethernet (RMII), SPI1 for the SX1302,
  Wi-Fi/Bluetooth, LEDs, RTC, USB gadget.
- `lorawan-os` distro and `lorawan-gateway-image`: ChirpStack 4.20 (SQLite), Concentratord 4.7.1 with a
  `bobcat_miner300_g285` model, MQTT Forwarder 4.6.1, Mosquitto, Redis.
- First-boot secret generation, key-only SSH, DHCP on Ethernet.
