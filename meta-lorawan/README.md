<!-- SPDX-License-Identifier: MIT -->
# meta-lorawan

A self-contained LoRaWAN gateway as a Yocto distro (`lorawan-os`) and image (`lorawan-gateway-image`):
ChirpStack Concentratord (SX1302), ChirpStack MQTT Forwarder, Mosquitto, Redis and the ChirpStack network server
with its web interface, all on the gateway.

Depends on: `openembedded-core`, `meta-oe`, `meta-python`, `meta-networking` (series `wrynose`).

Board layers set `LORAWAN_CONCENTRATOR_MODEL` (a Concentratord model name) in their machine configuration; the
region defaults to `EU868` (`LORAWAN_REGION`). See `../docs/architecture.md`.
