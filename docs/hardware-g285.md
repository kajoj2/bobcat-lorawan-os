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
- **[FACT]** this project, on its own G285 (2026-11-17): the factory firmware's device tree, configuration files
  and boot log, read from the board (facts only, nothing copied), and our image running on it.
- **[ROCK3C]** Linux `arch/arm64/boot/dts/rockchip/rk3566-rock-3c.dts`, the Rockchip RK3566 reference power tree
  (RK809 + TCS4525) the board shares.

No device tree or text was copied from sources without a license (Armbian G285 image); facts were taken from
the notes above, and the device tree was written from scratch.

## Identification

| | | |
|---|---|---|
| Serial number prefix `285` = G285 (G280: no Wi-Fi; G29x: different SPI bus) | U | [TTN] hostnames/compose files |
| SoC RK3566, 2 GB RAM, 64 GB eMMC (`SLD64G`, HS200, 8 bit) | V | [DD] §4, [FACT] |
| RAM: LPDDR4X, one rank, 32 bit; the factory DDR blob (V1.07) and rkbin's `rk3566_ddr_1056MHz` (v1.23) both run it at 1056 MHz | V | [FACT] |
| No SPI NOR flash | V | [FACT] (SPL: `unrecognized JEDEC id bytes: 00, 00, 00`) |
| Factory eMMC GPT: `uboot` (sector 0x4000, FIT), `skyboot` (0x8000, 512 MB ext4: kernel, initrd, DTB), `update` (0x108000, 4 GB), `rootfs` (0x908000, rest) | V | [FACT] |

## Boot

| | | |
|---|---|---|
| The boot ROM starts the eMMC's loader when it has one; the factory loader (Rockchip SPL 2017.09) tries the SD card first, but fails on our card (`space exhausted` reading its GPT) and then on the eMMC too | V | [FACT] |
| With the eMMC's loader area (sectors 64..16383) cleared, the boot ROM boots our idbloader and U-Boot from the SD card (docs/flashing.md) | V | [FACT] |
| The factory U-Boot ignores console input (no way to stop autoboot) | V | [FACT] |
| RECOVERY held at power-on: Rockchip Loader mode on the rear micro-USB (`2207:350a`); `rkdeveloptool rl` returns real data for the first 32 MiB of the eMMC only, `0xCC` filler beyond | V | [FACT] |
| Debug console UART2 (`0xfe660000`), pins GPIO0_D1 TX / GPIO0_D0 RX, **1500000 8N1**, 3.3 V | V | [DD] §3 |
| Console header: internal "DEBUG" pads, pin order not documented | ? | [DD] §3 |
| Rear micro-USB = OTG / Maskrom / Loader port | V | [DD] §3, [FACT] |
| Gadget mode on that port fails under mainline (`dwc3: failed to enable ep0out`) | ? | [FACT] |

## Power

| | | |
|---|---|---|
| 12 V DC input | V | label |
| PMIC RK809 at i2c0 0x20 (IRQ GPIO0_A3), CPU regulator TCS4525 at i2c0 0x1c | V | [DD] §10, [NEBRA] |
| RK809 regulator names and voltages as the RK3566 reference design | U | [NEBRA], [ROCK3C] |
| IO domains: pmuio2 = vcca_1v8, vccio1 = vccio_acodec (3.3 V), vccio3 = vccio_sd, vccio4..7 = vcc_3v3 (pmuio1, vccio2 not set) | V | [FACT] (factory DTB; [DD]'s list, vccio4 = 1.8 V and pmuio2 = 3.3 V, is the Rockchip EVB's) |

## LoRa concentrator

| | | |
|---|---|---|
| Mini-PCIe style card "N504S", SX1302 (chip model 0x00) + 2x SX1250 | V | [DD] §7 |
| SPI1 (`0xfe620000`), pin group m1: CS0 GPIO3_A1, MOSI/MISO/CLK GPIO3_C1/C2/C3 → `/dev/spidev1.0` | V | [DD] §7 |
| GPIO3_D5 (sysfs 125, gpio3-29): socket rail, factory sets it high once at boot | V | [DD] §7 (stock udev script) |
| GPIO3_D2 (sysfs 122, gpio3-26): the line the factory reset script drives low 1 s, then high | V | [DD] §7 |
| GPIO4_C5 (sysfs 149, gpio4-21): SX1302 reset, pulsed high then low | V | [DD] §7 |
| No STTS751 temperature sensor | U | [DD] §10 |
| Factory EU868 RF parameters: RSSI offset -215.4, temperature compensation c=20.41 d=2162.56, clock source radio 0, TX on radio 0 only (863-870 MHz), single input mode off, TX gain table 10..26 dBm with the PA on (pwr_idx 2..22) | V | [FACT] (factory `global_conf_sx1250_eu868.json`) |
| Concentrator starts under Concentratord with these pins; gateway ID read from the SX1302 matches the factory one | V | [FACT] |
| Socket SPI is on card pins 3/5/6/7, not the RAK2287/RAK5146 positions: a RAK card needs an adapter | V | [DD] socket pinout |
| No GNSS fitted; an empty u-blox NEO footprint, its UART would be UART5 (GPIO2_A1/A2) | V/U | [DD] §11, `gnss-populate.md` |
| SARADC channel 2 "card present" rule of the factory firmware is unreliable | V | [DD] §9 |

## Ethernet

| | | |
|---|---|---|
| GMAC1, RMII, SoC drives the 50 MHz reference clock (`clock_in_out = "output"`) | V | [DD] §5 |
| PHY Motorcomm YT8512B (id 0x00000128) at MDIO address 0, reset GPIO4_C2 active low, 20 ms / 100 ms | V | [DD] §5, [FACT] |
| The reset must be released before the MDIO bus is scanned (reset on the bus, not in the PHY node), or no PHY is found | V | [FACT] |
| Mainline has no YT8512B driver: the generic PHY driver gets no link | V | [FACT] (patch in `linux-yocto`, not yet tested) |
| GMAC1 pin group m0 (GPIO3_B1..B7, C0, C4, C5) | V | [NEBRA], [FACT] (factory DTB) |
| MAC address: stable, U-Boot derives it from the SoC's serial number (`local-mac-address`) | V | [FACT] |
| Autonegotiation may wedge after a warm reboot; a PHY soft reset or power cycle clears it | V (symptom), ? (cause) | [DD] §5 |

## Wi-Fi / Bluetooth

| | | |
|---|---|---|
| BCM43430/1 (AzureWave NM372) on SDIO (sdmmc1), reset GPIO2_B1 active low, host wake GPIO2_B2 | V | [DD] §6, [FACT] |
| Module power enable GPIO0_A0, active high; SDIO at 3.3 V | V | [FACT] (factory DTB) |
| Bluetooth on UART1 (m0, with RTS/CTS), shutdown GPIO2_B7, device wake GPIO2_C1, **host wake GPIO0_B3** (GPIO2_C0 is UART1's CTS) | V | [DD] §6, [FACT] (factory DTB) |
| NVRAM: the factory one is Rockchip rkwifibt's `nvram_azw372.txt` (identical bytes); linux-firmware's AP6212 file has a different TX power calibration | V | [FACT] |
| WPA2/WPA3 transition access points: `brcmfmac` needs `feature_disable=0x82000`, or no EAPOL frame reaches wpa_supplicant | V | [FACT] |

## Other

| | | |
|---|---|---|
| LEDs D3..D6 on GPIO0_D3..D6, active high (status, power, LAN, Wi-Fi) | V | [DD] §12 |
| Front RGB LED on GPIO4_A0/A1/A2 (factory sets 0/0/1), not described yet | U | [DD] §12 |
| RTC HYM8563 at 0x51 on **i2c4**, CR1220 backed (the factory DTB enables it on both i2c2 and i2c4) | V | [FACT] |
| Buttons: a "menu" key on SARADC channel 0 (pressed ~0.98 V) in the factory DTB; RESET and RECOVERY on the board | V (DTB), U (which button) | [DD] §12, [FACT] |
| Hardware TRNG of the RK3568 family works under mainline (`rockchip-rng`) | U | [FACT] (enabled, to be checked) |
| Microchip ATECC608 secure element (the Helium key) at 0x60 on **i2c2** (answers `i2cdetect -y 2`, 2026-11-18); unused by this project | V | [FACT] |
