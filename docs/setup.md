<!-- SPDX-License-Identifier: MIT -->
# Setup wizard: from a fresh card to a working gateway, with a phone

Flash the card (docs/flashing.md), put it into the gateway, power on. No cable, console or SSH key is needed:

1. **About two minutes after power-on**, a gateway with no network opens a Wi-Fi network named
   `bobcat-setup-XXXX` (the first part of the host name, then the last four digits of its MAC address). It is open,
   without a password; see "Security" below.
2. **Join it with a phone.** Most phones then open the wizard by themselves ("Sign in to network"). If not, open
   <http://192.168.4.1/setup>.
3. **Wi-Fi**: pick your network from the list (or type its name for a hidden one), enter the password, Join. The
   setup network closes and the gateway joins yours.
   - It worked: connect the phone to your Wi-Fi and open `http://bobcat-g285.local/setup` (or the address your
     router shows for the gateway). The wizard carries on there.
   - It did not (wrong password, out of range, a 5 GHz-only network): after about a minute the gateway goes back to
     its old configuration and the setup network returns. Join it again; the wizard shows why it failed.
4. **Where the frames go**: the local ChirpStack always gets every frame. Tick The Things Network (a cluster) or
   enter another Semtech UDP server to share the gateway with a community network (docs/forwarding.md).
5. **Internal storage** (boards with an eMMC, running from the card): optionally install the system with these
   settings onto the eMMC (docs/flashing.md, "Install onto the eMMC"). It erases the eMMC. It uses ChirpStack's
   default login (admin / admin) by itself and asks only if that password was changed.
6. **Done**: links to the status panel and ChirpStack, the gateway EUI, how to register the gateway on TTN, and
   where to add an SSH key (the panel's "SSH keys" card).

A gateway on a network cable can use the same wizard at `http://<gateway>/setup` without the setup network: the
Wi-Fi step is then optional. The status panel shows a link to the wizard until it has been finished once.

## When the setup network opens

| Trigger | |
|---|---|
| No network 2 minutes after boot | a gateway that was never set up (no Wi-Fi configured, wizard never finished): every time it is offline for 2 minutes. Any other gateway: once per boot, so a router that comes up late does not leave the gateway unreachable, and a short outage later does not open it. |
| The board's button held for 5 seconds (less than 15), then released | any time. Bobcat 300: the SARADC button, the one that held at power-on gives the USB disks (docs/boot-firmware.md). |

It closes after 15 minutes, when the wizard joins a network, or when the wizard is finished. While it is open the
gateway is not on your Wi-Fi (one radio), so a gateway that opened it after a late router comes back on its own
within 15 minutes.

## Factory reset

Hold the button for **15 seconds**, then release: the gateway forgets everything set or learned since it was
flashed and restarts, as if freshly flashed. Useful above all for a gateway running from the eMMC, where there is
no card to flash again.

| Gone | Kept |
|---|---|
| Wi-Fi network | the system itself (and the other A/B slot) |
| ChirpStack: devices, applications, users, so the login is admin / admin again, and its Redis state | SSH host keys and the image's authorized keys |
| SSH keys added on the panel (`/var/lib/lorawan-ssh/authorized_keys`) | |
| community networks, Home Assistant bridge, device alerts, remote access (WireGuard), BLE switch, extra firewall rules | the machine ID (same DHCP address) |
| setup wizard state, ChirpStack registration (redone on the next boot), first-boot secrets (made again) | the Helium key, if any (the hotspot's identity) |

After the restart the gateway has no network, so the setup network opens 2 minutes later. By hand:
`systemctl start lorawan-setup-reset`.

## Security

The setup network is open: a new gateway has no screen or label to show a password on. What limits that:

- It only runs while the gateway is being set up (above), and for 15 minutes at most.
- The Wi-Fi password typed into the wizard crosses the air unencrypted, from the phone to the gateway, during
  those minutes. Someone within Wi-Fi range listening at that moment could read it. If that matters to you, set
  the gateway up over a cable instead (`http://<gateway>/setup`), or over SSH (docs/flashing.md, "Wi-Fi").
- On the setup network the wizard needs no login. From your LAN, changing the Wi-Fi needs the ChirpStack login once
  the wizard has been finished (before that, anyone on the LAN can, like on any new device).
- ChirpStack's `admin` / `admin` login works until you change it; the wizard's last step says where.

## How it works

- `lorawan-agent` (docs/lorawan-agent.md) watches the network and the button, serves the wizard (`/setup`,
  `/api/setup`, `/api/setup/scan`, `POST /api/setup/wifi`, `POST /api/setup/done`), and on the setup network
  answers every DNS question with 192.168.4.1 and redirects any other host name to the wizard, which is what
  makes phones show it as a captive portal.
- `lorawan-setup-ap.service` (recipe `lorawan-setup`): scans for networks, then runs `hostapd` (open, channel 6)
  and gives wlan0 192.168.4.1/24 with systemd-networkd's DHCP server (a network file in `/run`, removed when it
  stops). Stopping it starts `wpa_supplicant@wlan0` again if Wi-Fi is configured.
- `lorawan-setup-wifi.service`: installs the configuration the agent wrote to `/var/lib/lorawan-setup/` as
  `/var/lib/lorawan-wifi/wpa_supplicant-wlan0.conf` (SSID in hex, so any name works), joins, and waits up to
  45 seconds for an address. On failure it puts the previous file back (or none) and reopens the setup network.
  The result is in `/var/lib/lorawan-setup/wifi-result`.
- The Wi-Fi file is the same one an update keeps (docs/updates.md) and the eMMC install copies.
- By hand: `systemctl start lorawan-setup-ap` opens the setup network, `systemctl stop lorawan-setup-ap` closes it;
  `rm /var/lib/lorawan-setup/done` shows the wizard's link on the panel again. Logs:
  `journalctl -u lorawan-agent -u lorawan-setup-ap -u lorawan-setup-wifi`.

Not done (yet): keeping the gateway on your Wi-Fi while the setup network is open (the BCM43430 may allow an
access point next to a client on some firmware), and a colour on the front LED for "setup network open".
