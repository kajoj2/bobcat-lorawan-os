<!-- SPDX-License-Identifier: MIT -->
# Flashing and first boot

> Pre-alpha. Booting from the card with the factory boot loader is tested on a board with Rockchip's own SPL
> (rkbin `rk356x_spl_v1.14`, the code base the factory one is built from) standing in for the factory one; a report
> from an untouched Bobcat is still welcome (see "If it does not boot").

No soldering, no USB tools and no changes to the board's eMMC: write the card, insert it, power on.

## Write the SD card

Use a good microSD card (8 GB or more; an "endurance" or industrial card lasts much longer in a gateway). The
image is `lorawan-gateway-os-<version>-bobcat-g285.wic.xz` from a release, or
`lorawan-gateway-image-bobcat-g285.rootfs.wic.xz` from your own build.

```sh
# Linux (bmaptool is faster and verifies the write)
bmaptool copy lorawan-gateway-image-bobcat-g285.rootfs.wic.xz /dev/sdX

# macOS / Linux without bmaptool
xz -dc lorawan-gateway-image-bobcat-g285.rootfs.wic.xz | sudo dd of=/dev/rdiskN bs=4m
```

## Boot

1. Power off the gateway and insert the card into the microSD slot on the board.
2. Connect Ethernet, then the 12 V supply.
3. The front LED turns red when our U-Boot starts, then blinks green once Linux and the gateway are up (blue: no
   network; docs/lorawan-agent.md).
4. The gateway gets an address over DHCP and announces itself as `bobcat-g285.local`; its status page is on
   `http://bobcat-g285.local/`.

The factory system on the eMMC stays as it was: take the card out and the Bobcat boots its factory firmware again.

### How the card boots

The RK3566's boot ROM starts the boot loader on the eMMC first. Bobcat's factory loader (Rockchip U-Boot 2017.09)
looks at the SD card before its own eMMC and loads U-Boot from the card's GPT entry named `uboot`. Our card has
that entry first, holding our U-Boot and TF-A, which then boot our Linux from the card (`meta-bobcat/files/wic/bobcat-g285.wks`).
The memory is set up by the factory loader, at its 1056 MHz.

Earlier images (before 2026-11-20) used meta-rockchip's nine-entry layout without a `uboot` entry; the factory
loader ran out of memory looking for it and the board did not boot at all, so those images needed the eMMC's
loader cleared first (below).

### If it does not boot

- No red LED at all: the factory loader did not take our U-Boot. Check the card in a computer (two partitions,
  `uboot` and `rootfsA`), write it again, try another card. The serial console (debug pads on the board,
  1500000 8N1, 3.3 V) shows where it stops; please open an issue with that log.
- Red LED that stays red: U-Boot runs but Linux does not come up; the console shows why.
- As a fallback, clear the eMMC's loader area (below, with `rkdeveloptool`): the board then boots everything from
  the card.

## Write a new image later

Take the card out and write it again, or without taking it out: hold the button on the board while connecting the
12 V supply, and connect the rear micro-USB port to a computer. Our U-Boot then shows the SD card as a USB disk
(`ums 0 mmc 1`, tested on a board: ~6 MB/s); write the new image to it as above, then power-cycle
(docs/boot-firmware.md). Writing an image over the card the running system booted from does not work: the
mounted root file system keeps the card open, and the data never reaches it before the reboot.

## Install onto the eMMC

The SD card wears out under a gateway's constant writes; the board's 64 GB eMMC lasts much longer. Running from
the card, the panel (`http://bobcat-g285.local/`, "Internal storage (eMMC)") backs up the eMMC and installs the
system onto it. From a shell the same is `bobcat-emmc` (`bobcat-emmc info`, `bobcat-emmc backup ITEM`,
`bobcat-emmc install`).

1. Change the ChirpStack admin password (ChirpStack's web interface on port 8080): the panel's backups and install
   ask for that login. The default admin/admin works too, but then anyone on your network can use them; the page
   warns while it is the default.
2. **Back up the factory eMMC** if you may ever want the Bobcat firmware back. The panel offers:
   - **Essentials**: the partition table, the boot loader and every partition with only its used blocks
     (`e2image`). That is everything needed to put the eMMC back as it was, ~640 MB compressed and a few minutes
     for a factory G285. Download every item and `RESTORE.txt`, which lists the commands to write them back; the
     panel also shows a `curl` loop that fetches them all.
   - **Full raw copy**: every sector, old deleted data included. Exact, but slow (30 minutes to hours) and big
     (36 GB from a used factory eMMC).
3. **Install**: type ERASE and start. It takes about 2 minutes (tested on a board, 2026-11-20); the gateway
   services pause for about 25 seconds while the system is copied. The SD card's system is copied as it is, with your settings, SSH keys, Wi-Fi and
   ChirpStack data.
4. Power off, take the card out, power on: the gateway starts from the eMMC, with our whole boot chain (DDR at
   1332 MHz).

The eMMC layout:

| Sectors | GPT entry | Contents |
|---|---|---|
| 64 | (none) | idbloader (DDR init + U-Boot SPL), copied from the card |
| 16384 | 1: `uboot` | u-boot.itb, copied from the card |
| 32768 | 2: `system_a` | the running system (4 GiB) |
| | 3: `system_b` | empty, for A/B updates later (4 GiB) |
| | 4: `data` | the rest (~50 GB), mounted on `/var/lib`: ChirpStack, Redis, Mosquitto, secrets |

The eMMC system finds its partitions by PARTUUID, so a card inserted later does not mix with it: with a card in
the slot the board starts the card (U-Boot tries the SD card first), which is also the way back if the eMMC
system does not start. To reinstall, boot a card with a new image and install again from its panel.

If the install stops half-way, the eMMC has no boot loader (that is written last) and the board keeps starting
from the card; run it again.

## Optional: our whole boot chain

With the factory loader in front, the DDR set-up and the SPL are Bobcat's; everything after them is ours. To run
our own from power-on (DDR at 1332 MHz, docs/boot-firmware.md), the loader area of the eMMC (sectors 64 to 16383,
before the first partition) is cleared, so the boot ROM moves on to the card. The factory partitions stay as they
are. **Without a card the board then does not boot at all** (it waits in Maskrom mode) until the area is written
back.

From the running gateway (booted from the card), keep a copy first, then clear it:

```sh
ssh root@bobcat-g285.local 'dd if=/dev/mmcblk0 bs=512 skip=64 count=16320' > emmc-loader-factory.img
head -c 4 emmc-loader-factory.img    # RKNS or RKSS: the factory loader; keep this file safe
ssh root@bobcat-g285.local 'dd if=/dev/zero of=/dev/mmcblk0 bs=512 seek=64 count=16320 && sync'
```

Back to the factory boot, again from the gateway booted from the card:

```sh
ssh root@bobcat-g285.local 'dd of=/dev/mmcblk0 bs=512 seek=64 && sync' < emmc-loader-factory.img
```

A full backup of the eMMC (the factory system, ~60 GB) is a good idea before any of this:
`ssh root@bobcat-g285.local 'dd if=/dev/mmcblk0 bs=4M | zstd -T0' > emmc.img.zst`.

Without a working card, the same can be done over USB with `rkdeveloptool` and a micro-USB data cable to the rear
port:

1. Hold the RECOVERY button on the board while connecting the 12 V supply, release after ~5 s. The board shows up
   on USB as `2207:350a` and `rkdeveloptool ld` reports `Loader` (or `Maskrom` when the area is already empty).
2. Keep the area you are about to clear: `rkdeveloptool rl 0 32768 emmc-first-16MiB.img` (in this mode the factory
   loader only returns real data for the first 32 MiB of the eMMC).
3. Clear it: `head -c 8355840 /dev/zero > zero.bin && rkdeveloptool wl 64 zero.bin`.

Back to the factory firmware in Maskrom: load a USB loader built from Rockchip's rkbin (`boot_merger
RKBOOT/RK3566MINIALL.ini`, then `rkdeveloptool db rk356x_loader_*.bin`) and write the saved area back:
`rkdeveloptool wl 0 emmc-first-16MiB.img`.

## Wi-Fi

Wi-Fi is not configured in the image. The easy way is the setup wizard from a phone (docs/setup.md): a gateway
without a network opens the setup network `bobcat-setup-XXXX` about two minutes after power-on. By hand, on the
gateway (over Ethernet or the serial console):

```sh
install -d -m 700 /var/lib/lorawan-wifi
read -r SSID; read -rs PSK    # type the network name, then the passphrase (not echoed)
wpa_passphrase "$SSID" "$PSK" | sed '/#psk/d' > /var/lib/lorawan-wifi/wpa_supplicant-wlan0.conf
chmod 600 /var/lib/lorawan-wifi/wpa_supplicant-wlan0.conf; unset SSID PSK
systemctl restart wpa_supplicant@wlan0
```

`wlan0` then gets an address over DHCP. The module's calibration file and Bluetooth firmware are fetched at build
time from a public copy of Rockchip's rkwifibt package (`bobcat-wifibt-firmware`); the module driver runs with
`feature_disable=0x82000`, without which it never completes the WPA handshake with WPA2/WPA3 access points.

## Log in

- Network: `ssh root@bobcat-g285.local` with the key you put into `LORAWAN_SSH_AUTHORIZED_KEYS` at build time.
  Passwords are refused over SSH.
- Serial console (debug pads on the board, 1500000 8N1, 3.3 V): `root`, no password.

## Set up ChirpStack

1. Open `http://bobcat-g285.local:8080`, log in as `admin` / `admin` and **change the password at once** (ChirpStack
   creates this default user; the network does not reach the UI from outside your LAN unless you forward it).
2. Gateways → Add gateway: the gateway EUI is in `journalctl -u chirpstack-concentratord` (`Gateway ID ...`).
3. Add a device profile (region EU868) and your devices.
4. Applications publish to the local broker: `mosquitto_sub -h localhost -t 'application/#' -v`.

Opening the MQTT broker to the LAN (for Home Assistant on another host) is a manual step for now: add a listener
with a password file in `/etc/mosquitto/`. A guided option is on the roadmap.
