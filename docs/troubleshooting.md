<!-- SPDX-License-Identifier: MIT -->
# Troubleshooting

Symptom first, then what to check. If none of it helps, open an issue with the gateway's report (below) and what
you did.

## The gateway's report

Everything the panel shows, as text, for an issue or a forum post:

- Panel: `http://bobcat-g285.local/api/status.txt` (no login).
- Over SSH or the serial console: `lorawan-diag`.
- Logs of one service: `journalctl -b -u <service>`, e.g. `chirpstack-concentratord`, `chirpstack`,
  `lorawan-agent`, `lorawan-setup-ap`, `rauc`.

Before posting, remove the gateway EUI, MAC addresses and anything you would not want public.

## Before it boots

| Symptom | Check |
|---|---|
| No LED at all, or the factory firmware starts | The factory loader did not take the card. Is it a **G285** (serial number starting with `285`)? Other revisions are not supported yet. Write the card again (with a checksum-verified download, docs/flashing.md), try another card, push it fully into the slot. |
| Red LED that stays red | U-Boot runs, Linux does not come up. Write the card again; if it repeats, the serial console (1500000 8N1, 3.3 V, docs/flashing.md) shows where it stops: please open an issue with that log. |
| Red, then blinking green | Up and fine. |
| Blinking blue | Up, but no network. Plug in Ethernet, or wait for the setup Wi-Fi (next section). |
| Steady red later on | A gateway service is down, Mosquitto is unreachable, or the radio sent no statistics for 2 minutes. The panel's top card names the problem. |

## Setting it up

| Symptom | Check |
|---|---|
| No `bobcat-setup-XXXX` network | It opens only on a gateway **without a network**, about 2 minutes after power-on. With Ethernet plugged in, use the wizard from your LAN instead: `http://bobcat-g285.local/setup`. To open it any time: hold the board's button 5 s and release (15 s is a factory reset). It closes after 15 minutes. |
| Joined the setup network, nothing opens | Open `http://192.168.4.1/setup` by hand. Turn off mobile data if the phone drops networks without internet. |
| Wi-Fi join failed | The wizard comes back on the setup network after about a minute and shows why. The gateway's Wi-Fi is 2.4 GHz only (a 5 GHz-only network is not seen); open, WPA2 and WPA3 personal networks work; the wizard has no enterprise (802.1X) login. |
| `bobcat-g285.local` does not open | Your computer or phone does not resolve `.local` names (some Android and Windows setups). Use the gateway's IP address from your router's list of clients. |
| Two gateways | Both call themselves `bobcat-g285`; the second gets `bobcat-g285-2.local` from Avahi. Use IP addresses, or rename one (`hostnamectl set-hostname`). |

## Logins

| Symptom | Check |
|---|---|
| Forgot the ChirpStack password | There is no recovery: a factory reset (button held 15 s) brings back admin / admin, but also empties ChirpStack (devices, applications) and the gateway's settings. With a backup from the panel's Backup card, restore it afterwards. |
| `ssh root@…`: Permission denied (publickey) | A release image has no SSH key. Add your **public** key on the panel, card "SSH keys" (ChirpStack login). Passwords are never accepted over SSH. After a factory reset the keys added there are gone. |
| `ssh`: host key changed | Expected after writing a new card: a fresh card makes new host keys on its first boot (updates keep them). Remove the old line (`ssh-keygen -R bobcat-g285.local`) and connect again. |

## Radio and networks

| Symptom | Check |
|---|---|
| Panel shows no frames | Is a device sending nearby, on EU868? The radio card counts every frame heard, also from other people's devices; nothing at all for hours points at the antenna (connected to the radio card?) or the concentrator (`journalctl -u chirpstack-concentratord`). |
| Device does not join ChirpStack | Its DevEUI, JoinEUI and AppKey in ChirpStack must match the device exactly; the device profile must be EU868 and the right LoRaWAN version. ChirpStack's device page → Events shows each join request and why it was rejected. |
| TTN shows the gateway as disconnected, or no traffic | The gateway must be **registered** in the TTN Console under its EUI (the panel shows it), on the same cluster that is ticked on the panel. The network must let UDP to port 1700 out and the answers back (any home router does). docs/forwarding.md. |
| Home Assistant gets nothing | The panel's Home Assistant card shows whether the bridge is connected and why not (login, port, TLS). docs/home-assistant.md. |

## Updates and the eMMC

| Symptom | Check |
|---|---|
| Update refused: signature or "unable to get local issuer certificate" | Only bundles signed with the project's key are accepted: take the `.raucb` from the release page, not from a CI run's artifacts. A gateway running your own build trusts your keys instead (docs/updates.md). |
| After an update the old version is back | The new system did not start three times in a row, so the gateway went back to the old one: the System update card shows the slots. Please open an issue with `journalctl -b -1` if you can get it. |
| eMMC system does not start | Insert a card with a release image: with a card in the slot the board always starts from the card, and its panel can install onto the eMMC again. |
| Want the Bobcat factory firmware back | Running from the card only: take the card out. After an eMMC install: restore the eMMC backup you took before (docs/flashing.md, "Install onto the eMMC"). |
