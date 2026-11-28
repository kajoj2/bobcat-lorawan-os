<!-- SPDX-License-Identifier: MIT -->
# AGENTS.md

Instructions for AI coding agents (Claude Code, Codex, Copilot, Cursor and the like) working with this project:
either **installing and setting up a gateway** for someone, or **changing this repository**. People are welcome to
read it too; the user-facing guides are README.md ("Quick start") and docs/.

LoRaWAN Gateway OS turns a retired Bobcat Miner 300 (board revision G285, RK3566, EU868) into a LoRaWAN gateway
with its own ChirpStack network server, a web panel and A/B updates. It is a Yocto image; releases on GitHub carry
an SD card image and a signed update bundle.

## Part 1: install a gateway for a user

### Ground rules

- **Writing a card erases a disk.** Never pick the target device yourself: list the disks, show them to the user,
  and write only after the user has named the device (e.g. `/dev/sdb`, `/dev/disk4`) and confirmed it. Never write
  to a disk that holds a mounted system partition.
- **The eMMC install erases the Bobcat's factory system.** Do it only when the user asks for it, after the eMMC
  backup (docs/flashing.md, "Install onto the eMMC"). Running from the card changes nothing on the board.
- **Factory reset** (button held 15 s, or `systemctl start lorawan-setup-reset`) wipes ChirpStack's devices and all
  settings. Ask first.
- Ask the user for physical steps and wait for them: inserting the card, plugging Ethernet and power, pressing the
  button, reading the label.
- Never handle private keys: for SSH only the user's **public** key (`~/.ssh/id_ed25519.pub`) goes to the gateway.
  Do not print Wi-Fi passwords or the ChirpStack password into logs, commits or issue text.
- ChirpStack's default login is `admin` / `admin`. Tell the user to change it (ChirpStack UI, top right); do not
  change it yourself unless asked, and if asked, make sure the user has the new password.
- Do not open the gateway's ports to the internet (router port forwards). Remote access is the WireGuard card
  (docs/remote-access.md).

### Steps

1. **Check the hardware.** Ask for the serial number on the Bobcat's label: it must start with `285` (G285).
   Other revisions (G280, G290, G295) are not supported yet; stop and say so. Region: EU868 only.
2. **Download the latest release** into an empty directory:

   ```sh
   gh release download -R kajoj2/bobcat-lorawan-os --pattern '*.wic.xz' --pattern '*.wic.bmap' --pattern SHA256SUMS
   # without gh: the asset URLs from https://api.github.com/repos/kajoj2/bobcat-lorawan-os/releases/latest
   sha256sum -c --ignore-missing SHA256SUMS        # macOS: shasum -a 256 -c --ignore-missing SHA256SUMS
   ```

   Stop if the check fails.
3. **Find the card** (user inserts it into the computer): `lsblk -d -o NAME,SIZE,MODEL,TRAN` on Linux,
   `diskutil list external` on macOS. Show the list; the user names the device.
4. **Write it**:
   - Linux: `sudo bmaptool copy lorawan-gateway-os-<version>-bobcat-g285.wic.xz /dev/sdX` (or
     `xz -dc … | sudo dd of=/dev/sdX bs=4M conv=fsync`).
   - macOS: `diskutil unmountDisk /dev/diskN`, then
     `xz -dc lorawan-gateway-os-<version>-bobcat-g285.wic.xz | sudo dd of=/dev/rdiskN bs=4m`, then
     `diskutil eject /dev/diskN`.
   - Windows, or if the user prefers a GUI: balenaEtcher or Raspberry Pi Imager ("Use custom") with the `.wic.xz`.
   `sudo` asks for the user's password: let the user type it (`! <command>` in Claude Code), never ask for it.
5. **Boot.** The user puts the card into the slot on the board, plugs Ethernet (recommended) and then the 12 V
   supply. LED: red (boot loader), then blinking green (up) or blinking blue (up, no network). About 1–2 minutes.
6. **Find the gateway**: `bobcat-g285.local` (mDNS). If it does not resolve, ask the user for its IP address from
   the router, or look for a new host on the LAN. Check: `curl -s http://bobcat-g285.local/api/status`.
7. **Set it up.** With a phone, the user can follow the wizard on the setup Wi-Fi (`bobcat-setup-XXXX`, opens
   2 minutes after boot on a gateway without network): docs/setup.md. Over the LAN you can do the same with HTTP
   (form-encoded POSTs; `<gw>` is the host name or IP):

   ```sh
   # Wi-Fi (optional when on Ethernet). Before the wizard is finished no login is needed; afterwards the
   # ChirpStack login is (add -u admin:admin or the user's own).
   curl -s --data-urlencode "ssid=$SSID" --data-urlencode "psk=$PSK" http://<gw>/api/setup/wifi
   # Share with The Things Network (optional; the user must register the gateway EUI in the TTN Console):
   curl -s --data-urlencode server=eu1.cloud.thethings.network:1700 http://<gw>/api/forward
   # The user's SSH public key (ChirpStack login):
   curl -s -u admin:admin --data-urlencode action=add --data-urlencode "key=$(cat ~/.ssh/id_ed25519.pub)" \
     http://<gw>/api/ssh/save
   # Mark the wizard as done (the panel stops offering it):
   curl -s -X POST http://<gw>/api/setup/done
   ```

   Read `$SSID`/`$PSK` from the user without echoing them into the transcript where possible.
8. **Verify.**
   - `curl -s http://<gw>/api/status`: `health.level` is `ok` (`starting` in the first 3 minutes),
     `gateway.registered` equals `gateway.id`, `problems` is empty. Text form: `/api/status.txt`.
   - `ssh root@<gw> lorawan-diag` once the key is added.
   - ChirpStack at `http://<gw>:8080` lists the gateway under Gateways (it registers itself).
9. **Hand over** to the user: panel URL, ChirpStack URL and the advice to change admin/admin, the gateway EUI
   (`gateway.id`), how to update (panel → System update → From GitHub), docs/troubleshooting.md.

### Later tasks

- **Update**: panel → System update → From GitHub → Install, then restart from the card. By API:
  `curl -s -X POST http://<gw>/api/releases` (refresh the list), then
  `curl -s -u <login> --data-urlencode tag=<tag> http://<gw>/api/update/download`, follow `GET /api/update` until
  installed, then `curl -s -u <login> -X POST http://<gw>/api/update/reboot`. A failed new system falls back to the
  old one by itself.
- **Add sensors**: in ChirpStack (device profile, application, device with its keys). Data: MQTT on the gateway
  (`application/#`), or Home Assistant through the panel's card (docs/home-assistant.md).
- **Install onto the eMMC**: only on request, after the eMMC backup; panel card "Internal storage (eMMC)" or
  `bobcat-emmc` over SSH (docs/flashing.md).
- **Something wrong**: docs/troubleshooting.md, then the report from `/api/status.txt` and `journalctl -b -u <unit>`.

## Part 2: change this repository

### Layout

| Path | What |
|---|---|
| `kas/` | Build configs; `bobcat-g285.lock.yml` pins every layer |
| `meta-bobcat/` | Board support: machine, device tree, U-Boot, kernel config, eMMC and RAUC integration |
| `meta-lorawan/` | Board-independent gateway: distro, ChirpStack stack, image, config, setup access point, runtime tests (`lib/oeqa`) |
| `lorawan-agent/` | Web panel, setup wizard, LED status, diagnostics, BLE: Rust, **no crate dependencies** on purpose |
| `docs/` | One file per topic; user guides and design notes |
| `tools/` | Device tree check, crates.inc tools, update keys, lock updates |
| `.github/workflows/` | PR checks, full builds, nightly lock updates, releases (docs/releasing.md) |

### Checks before a pull request

Run what applies; CI runs all of them:

```sh
(cd lorawan-agent && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test)
reuse lint
shellcheck <changed shell scripts>              # POSIX sh
oelint-adv $(git ls-files '*.bb' '*.bbappend' '*.bbclass' '*.inc')
tools/check-dts.sh                              # after device tree changes
```

A full image build needs a Linux x86-64 host with ~150 GB free and kas (`kas build kas/bobcat-g285.yml`; from
scratch several hours). Without a board, `kas build kas/qemuarm64.yml` runs the runtime tests in QEMU
(docs/testing.md). Do not start a multi-hour build without the user's go-ahead. The panel can be tried on a board
without an image build: cross-compile `lorawan-agent` for `aarch64-unknown-linux-musl`, copy it to `/tmp`, stop
`lorawan-agent.service` and run it from there; restart the service afterwards.

### Rules

- English everywhere. Commit subject `<area>: <what>`, imperative. Every commit has `Signed-off-by:`
  (`git commit -s`, DCO). New files start with an SPDX header (MIT; device trees `GPL-2.0-or-later OR MIT`).
- Pin everything: layer commits in the lock file, `SRCREV`s, checksums; no `AUTOREV`, no floating tags, no `latest`.
- No proprietary binaries in the repository: recipes fetch them from upstream with pinned revisions.
- Hardware facts in docs/hardware-g285.md need a source (log, measurement, datasheet); say how you know.
- No personal data in commits, logs or examples: serial numbers, MAC addresses, gateway EUIs of real units, keys,
  Wi-Fi passwords.
- Never commit signing keys. Release signing happens only in the `release` environment after a maintainer's
  approval (docs/releasing.md).
- A change users see updates its doc in docs/ and an entry in CHANGELOG.md ("Unreleased").
- Security defaults stay: SSH keys only, firewall (SECURITY.md), secrets generated on first boot, read-only system
  partition on the eMMC. Panel endpoints that change the system sit behind the ChirpStack login (`login_required`
  in `lorawan-agent/src/main.rs`).
- One topic per pull request; say what you tested and on which board revision.
