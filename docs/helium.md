<!-- SPDX-License-Identifier: MIT -->
# Helium (data-only hotspot)

The gateway can also serve the [Helium](https://www.helium.com/) IoT network, next to the local ChirpStack and The
Things Network. It joins Helium as a new **data-only hotspot** with a key of its own, which you onboard with your
own wallet. Tried on a board on 2026-11-21: it starts from the panel, makes its key, holds a session with Helium's
packet router and gets EU868 from the network; onboarding itself is not tried yet.

```
                                 ┌── ChirpStack MQTT Forwarder ── ChirpStack (local)
SX1302 ── Concentratord ─ZeroMQ──┤
                                 └── ChirpStack UDP Forwarder ─┬─ Semtech UDP ── TTN, other networks
                                                               └─ Semtech UDP 127.0.0.1:1680 ── Helium gateway ── gRPC ── Helium
```

## Why a new hotspot and not the factory one

Every Bobcat left the factory onboarded as a Helium hotspot: its key is in slot 0 of the ATECC608 secure element
(docs/hardware-g285.md). That hotspot is a token (NFT) on Solana in the wallet of whoever onboarded it, and only that
wallet can transfer it. With a second-hand Bobcat that is usually someone you cannot reach. Using the factory key
would work technically, but the rewards would go to that wallet and its asserted location would be someone else's.

So this image never uses the chip: the Helium gateway makes its own Ed25519 key in a file. The factory slots stay
untouched (the board can still go back to its factory use).

What a data-only hotspot gets: rewards only for the traffic of Helium devices it carries, no Proof-of-Coverage
rewards (beacons and witnesses, so the gateway does not send them). Near few Helium devices that is little or
nothing; it mainly adds coverage. Onboarding costs a one-time fee in Data Credits plus small Solana transaction
fees; check the current amounts in Helium's documentation before you start.

## Turning it on

1. **Panel** (`http://<gateway>/`), card "Community networks", "Choose networks": tick
   "Helium (data-only hotspot, onboard it first)", Save. The Helium gateway starts and makes its key on the first
   start; the card then shows the hotspot's three-word name and its address (a long base58 string).
2. **Back up the key** now: `scp root@<gateway>:/var/lib/helium-gateway/gateway_key.bin .` and keep it with your
   other secrets. It is the hotspot: lose it and you onboard (and pay for) a new one. It is on the data
   partition, so system updates keep it; a fresh SD card or eMMC install does not (see "Restoring the key").
3. **Wallet** (on your computer): install [`helium-wallet`](https://github.com/helium/helium-wallet-rs/releases)
   and create or import a wallet (`helium-wallet create basic`, see its README) holding a little SOL for fees and
   the HNT or Data Credits for the onboarding fee. Its Solana address is the owner and payer below.
4. **Onboarding transaction** (on the gateway, over SSH):

   ```
   helium_gateway add --owner <your Solana address> --payer <your Solana address>
   ```

   It prints JSON with `"mode": "dataonly"`, the hotspot's `address` and a base64 `txn`. Nothing is sent yet.
5. **Onboard** (on your computer), with the location of the gateway (`--gain` in dBi, `--elevation` in metres
   above ground, both optional):

   ```
   helium-wallet hotspots add iot --lat 52.2297 --lon 21.0122 --gain 3.0 --elevation 5 <txn> --commit
   ```

   Without `--commit` it only shows what it would do. Later moves: `helium-wallet hotspots update iot <address>
   --lat … --lon … --commit`.
6. **Check**: the card shows "connected to the Helium packet router", and the hotspot shows up in a Helium explorer
   under its name. `helium_gateway info key name region router` on the gateway prints the same.

To turn it off, untick Helium and Save: the panel stops the Helium gateway. The hotspot stays onboarded and comes
back when you tick it again (the key file stays).

## Restoring the key

On a fresh install, before ticking Helium on the panel (otherwise it makes a new key):

```
mkdir -p /var/lib/private/helium-gateway
cp gateway_key.bin /var/lib/private/helium-gateway/
```

systemd gives the file to the service's user when it starts. If a new key was made already, stop the service
(untick Helium), put the backup in place of `/var/lib/private/helium-gateway/gateway_key.bin`, and tick it again.

## What it does

- [Helium gateway](https://github.com/helium/gateway-rs) (gateway-rs, Apache-2.0; main's head after 1.3.0) listens
  for Semtech UDP on `127.0.0.1:1680`. The panel adds that address to chirpstack-udp-forwarder's server list, so
  every frame with a good CRC also goes there; downlinks from Helium come back the same way.
- It sends the frames to Helium's packet router, signed with its key. The router keeps only the frames of devices
  routed on Helium (by DevAddr and JoinEUI); your local devices stay private, as with TTN (docs/forwarding.md).
- Region: EU868 (the image's `LORAWAN_REGION`) until the Helium network answers with the region of the asserted
  location; both must match the concentrator's.
- `helium-gateway.service` runs as a dynamic user without privileges; it is enabled, but skipped (`ExecCondition`)
  while the server list has no Helium entry. While Helium is chosen it counts as a gateway service for the panel's
  health and the LED.
- The forwarder's row "Helium" on the card says whether the local Helium gateway answers; the line under the table
  says whether that gateway reaches Helium.

## Files

| Path | What |
|---|---|
| `/etc/helium_gateway/settings.toml` | Settings (upstream's, with the key path, region, PoC off). Also read by `helium_gateway info` and `add`. |
| `/var/lib/helium-gateway/gateway_key.bin` | The hotspot's key (a link into `/var/lib/private/helium-gateway`, the service's state directory). |
| `helium-gateway.service` | Started and stopped by the panel. Logs: `journalctl -u helium-gateway`. |

## Limits

- The onboarding steps follow the tools' current help texts; not tried yet.
- PoC is off, yet the Helium gateway still tries to reach the PoC ingest service and logs a warning
  (`beaconer: failed to reconnect`) when it cannot; harmless.
- Upstream's last release is from 2023 and main has had few changes since; Helium may change its services.
- One radio for all networks: Helium's downlinks share the transmitter and the duty cycle with the others.
- The card's onboarding hints need no login, like the rest of the community networks card; the key itself never
  leaves the gateway through the panel.
