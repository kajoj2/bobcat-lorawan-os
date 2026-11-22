<!-- SPDX-License-Identifier: MIT -->
# System updates (A/B with RAUC)

A gateway installed on the eMMC (docs/flashing.md, "Install onto the eMMC") has two system partitions, `system_a`
and `system_b`, and a `data` partition for `/var/lib`. An update is written into the system partition that is not
running, while the gateway keeps working; a restart then starts the new one. If it does not come up, the gateway
goes back to the old one by itself. Settings, ChirpStack's database, the community network list and the Wi-Fi
network stay.

[RAUC](https://rauc.io/) does the work: it checks the update's signature, writes the slot and tells U-Boot which
slot to start next.

## Installing an update

On the panel (`http://<gateway>/`), card "System update" (shown when the gateway was started as an A/B slot):

1. "Install an update", choose the bundle (`lorawan-gateway-os-<version>-bobcat-g285.raucb` from a release,
   `lorawan-gateway-bundle-bobcat-g285.raucb` from your own build), "Upload and install". The
   browser asks for a ChirpStack login (the account of the web interface on port 8080). The default admin/admin
   works too; while ChirpStack has it, the card warns that anyone on your network can install a system (only
   bundles signed with your build's key are accepted).
2. The bundle is uploaded (a few hundred MB, onto the data partition), then RAUC checks it and writes the other
   slot. The card shows the progress.
3. "Restart now". About a minute later the panel shows the new version as "running".

From a shell, the same: `rauc install /path/to/bundle.raucb`, then `reboot`. `rauc status --detailed` shows both
slots, the installed versions and which one starts next.

## How a slot is chosen

U-Boot (`meta-bobcat/recipes-bsp/u-boot/files/bobcat-g285.env`) starts, in this order:

1. a bootable SD card, as before (install and rescue);
2. when the eMMC has `system_a` and `system_b`: the first slot in `BOOT_ORDER` (initially `A B`) that has tries
   left (`BOOT_A_LEFT`, `BOOT_B_LEFT`, 3 each). One try is used up and saved before the kernel starts, with
   `rauc.slot=A` or `B` on the kernel command line;
3. anything else the standard boot finds (`extlinux.conf` on a partition with the bootable attribute).

On the started system, `rauc-mark-good.service` gives the slot its 3 tries back once the network, SSH and the
panel are up. A slot that fails three times in a row (a kernel that does not start, a panel that does not come up)
is passed over, and the other one starts. After an install, RAUC puts the new slot first in `BOOT_ORDER`. A slot whose
kernel runs but which never becomes good (no network, SSH or panel) restarts by itself 10 minutes after boot
(`bobcat-boot-check.timer`), so the counter runs out without anyone pulling the plug.

U-Boot keeps these variables in its environment on the eMMC: two copies, at 0x3F0000 and 0x3F8000 (before the
`uboot` partition at 8 MiB). From Linux, `fw_printenv` and `fw_setenv` (`/etc/fw_env.config`).

## What carries over

Both slots share the `data` partition on `/var/lib` (`var-lib.mount`, active when U-Boot passed `rauc.slot`):
ChirpStack, Redis and Mosquitto data, the secrets made on the first boot, the Wi-Fi network
(`/var/lib/lorawan-wifi`), the community network list, the Home Assistant bridge, extra SSH keys
(`/var/lib/lorawan-ssh/authorized_keys`), extra firewall rules, the journal and RAUC's own status. Files on the
system partition are copied from the running slot to the new one after an install (`bobcat-rauc-post-install`):

| File | Why |
|---|---|
| `/etc/machine-id` | DHCP client ID (same IP addresses), journal |
| `/etc/ssh/ssh_host_*` | the gateway's SSH identity (no "host key changed" warning) |
| `/root/.ssh/authorized_keys` | merged: the new image's keys plus the ones the gateway has |

Images before the read-only system kept the Wi-Fi network in `/etc/wpa_supplicant/wpa_supplicant-wlan0.conf`; their
update handler still copies it, and the new system moves it to `/var/lib/lorawan-wifi` once on its first start
(`lorawan-wifi-migrate.service`).

## Read-only system partition

A slot runs with its root file system read-only (`bobcat-ro-root.service`, once the early boot steps that write
there are done): the slot stays exactly what the signed bundle contained, and nothing the gateway does can wear
it out or leave it half-changed. What changes lives on the data partition or in RAM (`/run`, `/var/volatile`,
`/tmp`). A change by hand needs `mount -o remount,rw /` first and is gone with the next update: put it on
`/var/lib` instead. A system started from the SD card stays writable.

## Journal

An A/B gateway keeps the journal on the data partition (`/var/lib/journal`, bound to `/var/log/journal` by
`bobcat-journal.service`), so the log of a crash or a watchdog restart is still there afterwards:
`journalctl -b -1`. Limits: 200 MB, at least 1 GB left free, three months (`journald.conf.d/10-lorawan-journald.conf`).
From the SD card the journal stays in RAM, which spares the card.

## Building a bundle

```sh
kas build kas/bobcat-g285.yml --target lorawan-gateway-bundle
# build/tmp/deploy/images/bobcat-g285/lorawan-gateway-bundle-bobcat-g285.raucb
```

The bundle holds the image's root file system (ext4, `verity` format) and is signed. Each builder has its own keys:

```sh
tools/rauc-keys.sh ../rauc-keys          # once; next to the kas build directory by default
```

| File | What |
|---|---|
| `ca.cert.pem` | the update CA: built into the image as RAUC's keyring |
| `ca.key.pem` | the CA's key, only for issuing a new signing certificate: keep it offline |
| `key.pem`, `cert.pem` | sign the bundles (the certificate is issued by the CA, for code signing) |

The build stops with a message when the CA certificate is missing. Set `BOBCAT_RAUC_KEY_DIR` to keep them
elsewhere. **Keep the directory**: a gateway only accepts bundles signed under the CA its image carries, so new
keys mean installing the next image by hand on every gateway. Never commit the keys.

## First install of an A/B system

The update path exists from the first image that has RAUC on: the U-Boot that chooses slots and the system that
marks them good. A gateway gets it once by hand:

- **New install**: SD card with the image, then the panel's eMMC install (docs/flashing.md). It writes the card's
  U-Boot too; the next start from the eMMC is already an A/B start (slot A).
- **A gateway already on the eMMC with an older image**: write the new U-Boot (`u-boot.itb`) into the `uboot`
  partition and the new root file system into one slot; see the project's notes for the steps. Until then, the
  older U-Boot starts whichever system partition has the bootable attribute.

## Tested on a board (2026-11-20)

G285 on the eMMC, both slots and U-Boot from this repository:

- U-Boot started slot A by itself (`rauc.slot=A`), saved its environment, `rauc-mark-good` set `BOOT_A_LEFT`
  back to 3; `/var/lib` on the data partition; same IP addresses, all services up.
- `rauc install` of a bundle (109 MB): signature checked, slot B written (about a minute), the post-install
  handler carried the settings over, `BOOT_ORDER=B A`. After a restart: slot B, marked good, same addresses.
- Fallback: with the panel switched off on B (`systemctl mask lorawan-agent`), three starts of B used up its
  tries (2, 1, 0) and the fourth start was slot A, marked good. `rauc status mark-active other` and a restart went
  back to B.
- The panel's upload needs a ChirpStack login; admin/admin is accepted (owner's choice), with a warning on the card.

## Limits

- The boot loader itself is not updated by a bundle (a failed write there has no way back without a card).
- `/var/lib` content that a new image ships in its root file system is hidden under the data partition; new
  services create their directories at start (systemd `StateDirectory=`, tmpfiles).
- The SD card's system (started with the card inserted) does not use the A/B layout and has no update card.
- Settings changed with `systemctl enable`/`disable`, `timedatectl` or `hostnamectl` need a writable root (see
  "Read-only system partition"); the panel never needs one.
