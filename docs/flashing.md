<!-- SPDX-License-Identifier: MIT -->
# Flashing and first boot

> Pre-alpha: these steps are what the image is designed for, not yet tested on a board.

## Write the SD card

Use a good microSD card (8 GB or more; an "endurance" or industrial card lasts much longer in a gateway).

```sh
# Linux (bmaptool is faster and verifies the write)
bmaptool copy lorawan-gateway-image-bobcat-g285.rootfs.wic.xz /dev/sdX

# macOS / Linux without bmaptool
xz -dc lorawan-gateway-image-bobcat-g285.rootfs.wic.xz | sudo dd of=/dev/rdiskN bs=4m
```

## Make the board boot from the card (once)

The factory loader on the eMMC tries the SD card first but cannot start ours (its SPL runs out of memory reading
the card's partition table, seen on a G285). The boot ROM only moves on to the card when the eMMC has no valid
loader, so the loader area of the eMMC (sectors 64 to 16383, before the first partition) has to be cleared once.
The factory partitions stay as they are.

You need `rkdeveloptool`, a micro-USB data cable to the rear port, and the serial console is useful.

1. Hold the RECOVERY button on the board while connecting the 12 V supply, release after ~5 s. The board shows up
   on USB as `2207:350a` and `rkdeveloptool ld` reports `Loader`.
2. Keep the area you are about to clear (and everything before the first partition):
   `rkdeveloptool rl 0 32768 emmc-first-16MiB.img`
   (In this mode the factory loader only returns real data for the first 32 MiB of the eMMC; a full backup of the
   eMMC is easier from the running gateway: `ssh root@<gateway> 'dd if=/dev/mmcblk0 bs=4M | zstd -T0' > emmc.img.zst`.)
3. Clear the loader area: `head -c 8355840 /dev/zero > zero.bin && rkdeveloptool wl 64 zero.bin`.
4. Power off, insert the card, power on: the serial console shows `U-Boot SPL 2026.01 ... Trying to boot from MMC2`.

Without a card the board now starts in Maskrom mode. Back to the factory firmware: in Maskrom, load a USB loader
built from Rockchip's rkbin (`boot_merger RKBOOT/RK3566MINIALL.ini`, then `rkdeveloptool db rk356x_loader_*.bin`)
and write the saved area back: `rkdeveloptool wl 0 emmc-first-16MiB.img`.

## Boot

1. Insert the card into the microSD slot on the board.
2. Connect Ethernet, then the 12 V supply.
3. The gateway gets an address over DHCP and announces itself as `bobcat-g285.local`.

## Wi-Fi

Wi-Fi is not configured in the image. On the gateway (over Ethernet or the serial console):

```sh
mkdir -p /etc/wpa_supplicant
read -r SSID; read -rs PSK    # type the network name, then the passphrase (not echoed)
wpa_passphrase "$SSID" "$PSK" | sed '/#psk/d' > /etc/wpa_supplicant/wpa_supplicant-wlan0.conf
chmod 600 /etc/wpa_supplicant/wpa_supplicant-wlan0.conf; unset SSID PSK
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
