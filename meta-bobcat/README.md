<!-- SPDX-License-Identifier: MIT -->
# meta-bobcat

Board support for Bobcat Miner 300 Helium hotspots with a Rockchip RK3566.

| Machine | Board |
|---|---|
| `bobcat-g285` | Bobcat Miner 300, revision G285-V1.0 |

Depends on: `openembedded-core`, `meta-arm`, `meta-rockchip` (series `wrynose`).

Provides the machine configuration, the mainline device tree
(`recipes-kernel/linux/files/dts/`), a linux-yocto config fragment and `LORAWAN_CONCENTRATOR_MODEL` for
meta-lorawan. Hardware notes: `../docs/hardware-g285.md`.
