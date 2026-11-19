<!-- SPDX-License-Identifier: MIT -->
# Roadmap: more from the hardware

Features planned on top of the gateway, each with what it needs, the steps and the open points. Order is a
suggestion: 7 and 5 make the gateway easier to use, 6 makes it last, 4 and 9 are for later.

## Experimental: boot firmware (branch `experimental/boot-firmware`)

Not on main and not tested on a board yet. The branch has three switchable changes to the boot chain, described
with their tests in `docs/experimental-boot-firmware.md` on that branch:

- U-Boot with the G285's own device tree: status LED from power-on, the button held at power-on shows the SD card
  and eMMC as USB disks (write an image without taking the card out); groundwork for item 6.
- BL31 built from upstream TF-A sources instead of Rockchip's blob.
- LPDDR4X at 1332 MHz instead of the factory 1056 MHz: above what the board was set up for, only worth it for
  memory-bound work, never the default without an overnight memory test.

Each part moves to main on its own once it has passed its tests on a board; the DDR clock probably stays opt-in.

## 7. Front RGB LED as status

The front RGB LED is the board's only LED: GPIO4_A0 green, GPIO4_A1 blue, GPIO4_A2 red, active high (checked on a
board, docs/hardware-g285.md). In the device tree since 2026-11-18 as three `gpio-leds`, green with the `heartbeat`
trigger; the single LEDs D3..D6 some notes describe are not on the G285.

Steps:
1. Device tree: group the three into one multicolour LED (`leds-group-multicolor`), so user space sets a colour
   through `/sys/class/leds/rgb:status/multi_intensity`; the heartbeat then moves to the service below.
2. Done in `lorawan-agent` (docs/lorawan-agent.md): green heartbeat when the gateway stack, the radio path and
   the network are fine, blue without network, red on a failure. Still open: a short white flash per received
   uplink.
3. U-Boot (experimental branch): light the LED from power-on, so a board stuck before Linux shows it.

Open: which states are worth a colour; brightness at night (an option to turn the RGB LED off).

## 5. Wi-Fi setup without a cable or console

Today Wi-Fi is set over SSH or the serial console (docs/flashing.md). Goal: set it from a phone.

Design:
- The SARADC button (adc-keys, already in the device tree) held for 5 s, or no network at all 2 minutes after
  boot, starts a setup access point `bobcat-setup-XXXX` (last 4 hex digits of the MAC), WPA2 with a per-device
  password generated on the first boot and printed on the serial console and in `/etc/issue`; never an open
  network. It stops after 15 minutes or once Wi-Fi is configured.
- The BCM43430 supports AP mode under brcmfmac; `hostapd` for the AP, systemd-networkd with `DHCPServer=yes`
  for addresses.
- A one-page form (network list from a scan, passphrase) served by busybox `httpd` with a CGI script that writes
  `wpa_supplicant-wlan0.conf` the same way docs/flashing.md does. Same page later for the ChirpStack admin
  password and MQTT credentials (an open item of its own).

Steps: button input daemon (`triggerhappy` or a tiny evdev reader), AP unit + network file, the CGI page, a
QEMU test for the CGI script, a board test (AP and client mode switching with brcmfmac's `feature_disable`).

Open: AP and station at the same time (brcmfmac allows it on some firmware) to keep Ethernet users unaffected;
how to show the AP password on a board without a screen (console, a label printed at install).

## 6. Run from the eMMC, and keep local history

The SD card is the weak part in a gateway that writes all day. The board has 64 GB eMMC (HS200).

Install:
- A `bobcat-install` command on the SD image: writes idbloader (sector 64), U-Boot (sector 16384) and the
  partitions to `/dev/mmcblk0`, after a check that a backup of the factory eMMC exists (or `--no-backup`). The
  boot ROM then starts our loader from the eMMC; a card with an image should still win when inserted (to
  check: U-Boot's boot order).
- Layout decided together with updates: two root file systems (A/B) + a data partition (`/var/lib`: ChirpStack's
  SQLite, Redis, Mosquitto, local history). A/B with RAUC (meta-rockchip has a demo) is the candidate; the
  update decision in docs/architecture.md comes first.
- The data partition survives updates and reinstalls; the root file system could become read-only.

Local history:
- ChirpStack keeps only recent frames. For days/months of sensor values on the gateway itself: decoded uplinks
  from MQTT into a small time-series store (VictoriaMetrics single binary, or SQLite with one table per device
  profile) with retention, readable from Home Assistant or Grafana elsewhere.
- Only worth it where the gateway is used without Home Assistant; with HA, its recorder already keeps history.

Open: A/B or not; which store, if any.

## 4. ATECC608 secure element as the gateway's identity

Every Bobcat has a Microchip ATECC608 (the Helium key in one slot). Its private keys never leave the chip.

Rules: read-only survey first; never write or lock the configuration zone, never touch the Helium slots (0: gateway
key, 15: onboarding key, from the factory start script), so the board can still go back to its factory use.

Steps:
1. Found on 2026-11-18: i2c2, address 0x60 (`i2cdetect -y 2`), the bus the factory firmware used.
2. Read its configuration zone and lock state (cryptoauthlib's tools, read only): which slots exist, which allow
   key generation (`GenKey`) and signing.
3. If a free slot allows it: generate a P-256 key in it, use it through cryptoauthlib's PKCS#11 module with
   OpenSSL, and make a client certificate for TLS to MQTT / Home Assistant from it.
4. Recipe for cryptoauthlib (check its license first), docs.

Open: whether Bobcat left any slot usable; if none, the chip still gives a hardware serial number and a random
number source, and the plan stops there.

## 9. Wireless M-Bus meters through the concentrator's FSK channel (experiment)

Many water and heat meters send Wireless M-Bus at 868.95 MHz: mode T1 (100 kchip/s, 3-out-of-6 coding) or C1
(100 kbit/s NRZ). The SX1302 has one FSK receive channel next to the LoRa channels. Whether it can receive these
frames is not known; this is an experiment, not a promise.

Steps:
1. Read the SX1302 FSK limits (data rate, sync word length, fixed or variable length, CRC off) against the
   wM-Bus C1 and T1 frame formats; C1 is the easier target (plain NRZ, sync word 0x543D54CD).
2. Test with libloragw's own test programs, outside Concentratord: FSK channel on 868.95 MHz, 100 kbit/s, the
   C1 sync word, CRC off, and see whether frames arrive next to a known meter.
3. If they do: pass them to wmbusmeters (decoding; encrypted meters need the key from the utility) and to MQTT.
   Concentratord's FSK channel would then need a raw mode, upstream or as our patch.

Open: a wM-Bus meter in range to test with; T1 decoding (3-out-of-6) in software if the radio can only deliver
raw bits.
