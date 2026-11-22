<!-- SPDX-License-Identifier: MIT -->
# Roadmap: more from the hardware

Features planned on top of the gateway, each with what it needs, the steps and the open points. Order is a
suggestion: 7 makes the gateway easier to use, 6 makes it last, 4 and 9 are for later.

## Done: boot firmware

On main since 2026-11-19 (docs/boot-firmware.md, with the board tests): U-Boot with the G285's own device tree
(status LED from power-on; the button held at power-on shows the SD card and eMMC as USB disks), BL31 from upstream
TF-A sources instead of Rockchip's blob, and LPDDR4X at 1332 MHz (passed an 8-hour memory test). Each can be
switched back on its own.

## 10. Community networks

Share the gateway with public networks next to the local ChirpStack (docs/forwarding.md).

Steps:
1. Semtech UDP to The Things Network or any UDP server, chosen on the panel (chirpstack-udp-forwarder 4.3.1).
   Tested on a board with TTN eu1 2026-11-20.
2. LoRa Basics Station (TLS, API key) for TTN instead of UDP: needs a Basics Station client for Concentratord's
   ZeroMQ API (none exists; Semtech's `station` drives the radio itself, which Concentratord already owns): the
   protocol's timing (`xtime`, RX1/RX2 choice) is the hard part.
3. Helium: the Helium gateway (`gateway-rs`) takes Semtech UDP locally and signs with a key in the ATECC608 (slot 0
   holds the Helium key; using it is read-only, see item 4). Only if the Helium network is still worth it.

Open: a white LED flash per forwarded frame (with item 7); a per-network JoinEUI/DevAddr filter, if anyone needs one.

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
3. Done in U-Boot: the LED is red from power-on, so a board stuck before Linux shows it.

Open: which states are worth a colour; brightness at night (an option to turn the RGB LED off).

## Done: setup wizard (was 5, Wi-Fi setup without a cable or console)

docs/setup.md: a gateway without a network (or with the button held 5 s) opens an open setup access point; a
phone joins it, and a wizard on the panel sets Wi-Fi (falling back to the access point when the join fails), the
community networks and the install onto the eMMC. Open on purpose (owner's choice, 2026-11-22): a new gateway has
no screen for a password; it runs only while setting up, for 15 minutes at most.

Still open: access point and client at the same time (brcmfmac on some firmware), so Ethernet and Wi-Fi users stay
connected while it is open; a LED colour for "setup network open" (with item 7); the same page for the ChirpStack
admin password and MQTT credentials.

## 6. Run from the eMMC, and keep local history

The SD card is the weak part in a gateway that writes all day. The board has 64 GB eMMC (HS200).

Install (done 2026-11-20, docs/flashing.md, "Install onto the eMMC"):
- `bobcat-emmc` (meta-bobcat) with a page on the panel: eMMC backups (essentials with only used blocks, or a full
  raw copy) and an install that writes idbloader, U-Boot and an A/B layout (`system_a`, `system_b`, `data` on
  `/var/lib`) from the running card. U-Boot tries the card first (`boot_targets=mmc1 mmc0`), so a card still wins.
- A/B updates with RAUC from the panel (docs/updates.md), tested on a board 2026-11-20 (install into the other
  slot, restart, fall back after three failed starts). Read-only system partition and the journal on the data partition
  since 2026-11-22.

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

## Not possible: spectral scan on the panel

A noise and interference view of the 868 MHz band needs the SX1261 that some SX1302 cards carry for listen before
talk and spectral scan (libloragw's `spectral_scan`). The G285's card has only the SX1302 and two SX1250
(docs/lorawan-agent.md), so there is nothing to scan with. Dropped 2026-11-22.
