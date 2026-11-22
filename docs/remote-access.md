<!-- SPDX-License-Identifier: MIT -->
# Remote access (WireGuard)

A gateway at a friend's house, on a farm or behind a mobile router is usually behind NAT: nothing reaches its panel,
ChirpStack or SSH from outside. A WireGuard tunnel from the gateway to a server or VPN you run fixes that without
opening anything on the gateway's network: the gateway connects out and keeps the tunnel open.

## Set up

You need a WireGuard server with a public address (a VPS, your router, a home server with a forwarded UDP port).

1. Panel, card "Remote access (WireGuard)", "Show the gateway's public key" (asks for a ChirpStack login). The
   gateway makes its key pair on first use; the private key stays on the gateway (`/var/lib/lorawan-wireguard`).
2. On the server, add the gateway as a peer:

   ```ini
   [Peer]
   # bobcat gateway
   PublicKey = <the key from step 1>
   AllowedIPs = 10.8.0.5/32
   ```

3. On the card, the gateway's side, as a wg-quick file without `PrivateKey`:

   ```ini
   [Interface]
   Address = 10.8.0.5/32

   [Peer]
   PublicKey = <the server's public key>
   Endpoint = vpn.example.org:51820
   AllowedIPs = 10.8.0.0/24
   PersistentKeepalive = 25
   ```

   "Save and connect". The card shows the tunnel's state and, per peer, the last handshake and the traffic.

Then the gateway answers on `10.8.0.5`: `http://10.8.0.5/` (panel), `http://10.8.0.5:8080/` (ChirpStack),
`ssh root@10.8.0.5`. The firewall's rules apply as on any network (SECURITY.md).

## What is accepted

`[Interface]`: `Address`, `ListenPort`, `MTU` (`PrivateKey` is always the gateway's own). `[Peer]`: `PublicKey`,
`PresharedKey`, `AllowedIPs`, `Endpoint`, `PersistentKeepalive`. Not accepted: `PreUp`/`PostUp`/`PreDown`/`PostDown`
(wg-quick runs them as shell commands), `DNS` (there is no resolvconf), `Table`, `FwMark`. A `ListenPort` is opened
in the firewall (`/var/lib/lorawan-firewall/wireguard.nft`).

With `AllowedIPs = 0.0.0.0/0` all of the gateway's traffic, LoRaWAN forwarding and updates included, goes through
the tunnel; usually you want only the tunnel's own network there.

## How it works

The configuration is stored as `/var/lib/lorawan-wireguard/wg0.conf` (0600, on the data partition, so it stays
across updates and is part of a backup). `lorawan-wireguard.service` runs `wg-quick up` on it at boot once it
exists, and retries every 30 s while the endpoint cannot be reached. "Turn off" stops the tunnel and removes the
configuration; the key stays, so the server's peer entry keeps working when it is set up again. The kernel's
WireGuard module comes with the image.

Tailscale (also in meta-openembedded) would avoid running your own server, at about 30 MB more per image and
update; it is not in the image for now.
