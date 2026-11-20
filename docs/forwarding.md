<!-- SPDX-License-Identifier: MIT -->
# Sharing the gateway with community networks

The gateway runs its own network server (ChirpStack) for your sensors. It can also pass what it hears on to a
public network such as [The Things Network](https://www.thethingsnetwork.org/) (TTN), so people around you can
use it for their devices, and your gateway helps the community coverage map. Both work at the same time: the
local ChirpStack keeps receiving every frame.

Off by default. Nothing leaves the gateway until you choose a network on the panel.

```
                                 ┌── ChirpStack MQTT Forwarder ── Mosquitto ── ChirpStack (local)
SX1302 ── Concentratord ─ZeroMQ──┤
                                 └── ChirpStack UDP Forwarder ── Semtech UDP :1700 ── TTN, or another network
```

## Turning it on

1. **Register the gateway** on the network. For TTN: [TTN Console](https://console.cloud.thethings.network/),
   pick the cluster closest to you (Europe: `eu1`), Gateways → Register gateway:
   - Gateway EUI: the Gateway ID shown on the panel (also `lorawan-diag`), e.g. `0016c001f150193b`;
   - Frequency plan: your region's, for EU868 "Europe 863-870 MHz (SF9 for RX2 - recommended)";
   - leave "Require authenticated connection" off (Semtech UDP has no authentication).
   TTN drops frames from gateways it does not know.
2. **Panel** (`http://<gateway>/`), card "Community networks", "Choose networks": tick the TTN cluster you
   registered on (or enter another server as `host:port`), Save. The forwarder starts; within about 30 s the server shows as "connected", and the gateway as connected in the TTN
   Console.

To turn it off, untick everything and Save.

## What it does

- [ChirpStack UDP Forwarder](https://github.com/chirpstack/chirpstack-udp-forwarder) 4.3.1 subscribes to
  Concentratord's events next to the MQTT Forwarder and sends every frame with a good CRC to each server over the
  Semtech UDP protocol (`PUSH_DATA`), with a keepalive every 10 s (`PULL_DATA`). Downlinks from a server
  (`PULL_RESP`) go to Concentratord's command socket, like the local ChirpStack's.
- Each network keeps only its own devices' frames, by DevAddr and JoinEUI; TTN passes frames of other networks on
  through Packet Broker. Your own devices on the local ChirpStack stay private to it: their payloads are encrypted
  with keys only ChirpStack has, so a public network sees metadata (DevAddr, signal strength) and cannot read them.
- The panel shows per server: state (connected when it answered a keepalive in the last 45 s), uplinks sent and
  acknowledged, downlinks received, time of the last answer. Counters are the forwarder's own (Prometheus on
  `127.0.0.1:9800`), since it last started.

## Files

| Path | What |
|---|---|
| `/etc/chirpstack-udp-forwarder/chirpstack-udp-forwarder.toml` | Fixed part: log level, metrics, Concentratord's sockets. |
| `/var/lib/lorawan-forward/servers.toml` | Server list, written by the panel (`[[udp_forwarder.servers]]`). On the data partition, so it survives updates. |
| `chirpstack-udp-forwarder.service` | Enabled, but skipped (`ConditionPathExists`) while there is no server list. Started, restarted and stopped by the panel. |

The same from a shell, without the panel: write `servers.toml` in the format the panel uses (see
`lorawan-agent/src/forward.rs`, `render`), then `systemctl restart chirpstack-udp-forwarder`. Logs:
`journalctl -u chirpstack-udp-forwarder` (warnings only; it logs every keepalive at the `INFO` level).

## Limits

- **No login on the card yet.** Anyone who can reach the panel can change the server list, and a forwarding
  server can also make the gateway transmit (downlinks). Keep the panel on your LAN. While ChirpStack still has
  its default password the card says so: change it in the ChirpStack web interface (port 8080, docs/flashing.md).

- **Semtech UDP is unauthenticated and unencrypted.** Anyone who knows the gateway's EUI could send traffic as
  it, and the path is not protected against tampering. TTN still supports it and most community gateways use it.
  LoRa Basics Station (TLS with an API key) would fix that; there is no Basics Station client for Concentratord
  yet, so it is a possible later step.
- **One radio for all networks.** Downlinks from the local ChirpStack and from each server share the transmitter
  and the EU868 duty-cycle budget. Two downlinks for the same moment: Concentratord sends one and refuses the
  other (the panel's Downlinks card counts it). Rare at home, but more networks mean more contention.
- The firewall must let the gateway send UDP to port 1700 and receive the answers (any NAT router does).
