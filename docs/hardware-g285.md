<!-- SPDX-License-Identifier: MIT -->
# Bobcat Miner 300, G285: hardware notes

What the image relies on, with the source of each fact. **V** = verified by the source on real hardware (live
logs, device trees, logic analyzer); **U** = unverified (inference or a different board revision); **?** = still
open. Facts verified by this project on its own board get a "this project" source when it happens.

Sources:
- **[DD]** docdailey/bobcat-300, `docs/os-boot-gpio.md` (fact sheet, 2026-10-22) and
  `docs/socket-pinout-rak5146.md`: https://github.com/docdailey/bobcat-300 (MIT)
- **[NEBRA]** NebraLtd/balena-bobcat-rk3566, decompiled factory DTB of another RK3566 Bobcat revision (EVB2 based,
  kernel 4.19): https://github.com/NebraLtd/balena-bobcat-rk3566 (Apache-2.0)
- **[TTN]** AdvdLaar/Bobcat300-TTN, TTN packet forwarder setup for G280/G285/G29x:
  https://github.com/AdvdLaar/Bobcat300-TTN (MIT)
- **[ROCK3C]** Linux `arch/arm64/boot/dts/rockchip/rk3566-rock-3c.dts`, the Rockchip RK3566 reference power tree
  (RK809 + TCS4525) the board shares.

No device tree or text was copied from sources without a license (Armbian G285 image); facts were taken from
the notes above, and the device tree was written from scratch.

## Identification

| | | |
|---|---|---|
| Serial number prefix `285` = G285 (G280: no Wi-Fi; G29x: different SPI bus) | U | [TTN] hostnames/compose files |
| SoC RK3566, 2 GB RAM, 64 GB eMMC (`SLD64G`, HS200, 8 bit) | V | [DD] §4 |

## Boot

| | | |
|---|---|---|
| RK356x boot ROM tries eMMC before SD; the factory eMMC loader (Rockchip SPL 2017.09) prefers SD for the next stage | U | [DD] §2 (which loader runs first is "not established") |
| A card with an idbloader at sector 64 and U-Boot at sector 16384 boots while the eMMC stays untouched; removing the card restores the factory firmware | V | [DD] §2 (Armbian on SD) |
| Debug console UART2 (`0xfe660000`), pins GPIO0_D1 TX / GPIO0_D0 RX, **1500000 8N1**, 3.3 V | V | [DD] §3 |
| Console header: internal "DEBUG" pads, pin order not documented | ? | [DD] §3 |
| Rear micro-USB = OTG / Maskrom port | U | [DD] §3 |

## Power

| | | |
|---|---|---|
| 12 V DC input | V | label |
| PMIC RK809 at i2c0 0x20 (IRQ GPIO0_A3), CPU regulator TCS4525 at i2c0 0x1c | V | [DD] §10, [NEBRA] |
| RK809 regulator names and voltages as the RK3566 reference design | U | [NEBRA], [ROCK3C] |
| IO domains: vccio1/5/6/7 = vcc_3v3, vccio2/4 = vcc_1v8, vccio3 = vccio_sd, pmuio1/2 = vcc3v3_pmu | V | [DD] `pps-uart-qerr.md` (G285 DTB) |

## LoRa concentrator

| | | |
|---|---|---|
| Mini-PCIe style card "N504S", SX1302 (chip model 0x00) + 2x SX1250 | V | [DD] §7 |
| SPI1 (`0xfe620000`), pin group m1: CS0 GPIO3_A1, MOSI/MISO/CLK GPIO3_C1/C2/C3 → `/dev/spidev1.0` | V | [DD] §7 |
| GPIO3_D5 (sysfs 125, gpio3-29): socket rail, factory sets it high once at boot | V | [DD] §7 (stock udev script) |
| GPIO3_D2 (sysfs 122, gpio3-26): the line the factory reset script drives low 1 s, then high | V | [DD] §7 |
| GPIO4_C5 (sysfs 149, gpio4-21): SX1302 reset, pulsed high then low | V | [DD] §7 |
| No STTS751 temperature sensor | U | [DD] §10 |
| Socket SPI is on card pins 3/5/6/7, not the RAK2287/RAK5146 positions: a RAK card needs an adapter | V | [DD] socket pinout |
| No GNSS fitted; an empty u-blox NEO footprint, its UART would be UART5 (GPIO2_A1/A2) | V/U | [DD] §11, `gnss-populate.md` |
| SARADC channel 2 "card present" rule of the factory firmware is unreliable | V | [DD] §9 |

## Ethernet

| | | |
|---|---|---|
| GMAC1, RMII, SoC drives the 50 MHz reference clock (`clock_in_out = "output"`) | V | [DD] §5 |
| PHY Motorcomm YT8512B (id 0x00000128) at MDIO address 0, reset GPIO4_C2 active low, 20 ms / 100 ms | V | [DD] §5 |
| GMAC1 pin group m0 (GPIO3_B1..B7, C0, C4, C5) | U | [NEBRA] (other revision, same PHY reset pin) |
| Autonegotiation may wedge after a warm reboot; a PHY soft reset or power cycle clears it | V (symptom), ? (cause) | [DD] §5 |

## Wi-Fi / Bluetooth

| | | |
|---|---|---|
| BCM43430/1 (AzureWave NM372) on SDIO (sdmmc1), reset GPIO2_B1 active low, host wake GPIO2_B2 | V | [DD] §6 |
| Bluetooth on UART1 (m0, with RTS/CTS), shutdown GPIO2_B7, host wake GPIO2_C0, device wake GPIO2_C1 | V | [DD] §6 |
| `brcmfmac` works with the generic NVRAM file | V | [DD] §6 |

## Other

| | | |
|---|---|---|
| LEDs D3..D6 on GPIO0_D3..D6, active high (status, power, LAN, Wi-Fi) | V | [DD] §12 |
| Front RGB LED on GPIO4_A0/A1/A2 (factory sets 0/0/1), not described yet | U | [DD] §12 |
| RTC HYM8563 at 0x51, CR1220 backed; on i2c4 (m0) or i2c2 (m1) | ? | [DD] §10 |
| Buttons: a "menu" key on SARADC channel 0 in the factory DTB; RESET and RECOVERY on the board | U | [DD] §12 |
