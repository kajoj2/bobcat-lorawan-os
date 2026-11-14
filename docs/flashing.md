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

## Boot

1. Unplug the Bobcat, open it, insert the card into the microSD slot on the board.
2. Connect Ethernet, then the 12 V supply.
3. The gateway gets an address over DHCP and announces itself as `bobcat-g285.local`.

The eMMC is never written. To go back to the factory firmware, power off and remove the card.

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
