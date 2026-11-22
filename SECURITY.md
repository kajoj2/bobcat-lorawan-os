<!-- SPDX-License-Identifier: MIT -->
# Security

Please report vulnerabilities privately through GitHub's "Report a vulnerability" (Security tab of the
repository), not in public issues.

## Defaults the image ships with

- SSH accepts public keys only; there are no keys unless you add yours at build time (`LORAWAN_SSH_AUTHORIZED_KEYS`).
- `root` has no password, which only the local consoles accept (serial pads inside the case, USB gadget).
- The ChirpStack API secret is generated on the first boot of each gateway.
- Mosquitto and Redis listen on localhost only.
- **ChirpStack's web interface (port 8080) starts with the user `admin` / `admin`.** Change the password right
  after the first boot and do not expose port 8080 to the internet.

## Firewall

`lorawan-firewall.service` loads an nftables rule set (`/usr/share/lorawan-firewall/firewall.nft`) before any
network comes up. Incoming connections are accepted only to what the gateway offers; answers to what the gateway
starts itself (community network forwarders, the Home Assistant bridge, NTP, updates) pass as usual:

| Port | What |
|---|---|
| TCP 22 | SSH (keys only) |
| TCP 80 | the panel and the setup wizard |
| TCP 8080 | ChirpStack's web interface and API |
| UDP 5353 | mDNS (`<host>.local`) |
| UDP 68, 546 | DHCP and DHCPv6 replies |
| UDP 53, 67, TCP 53 on `wlan*` | the setup access point's DHCP server and captive DNS (docs/setup.md) |
| ICMP, ICMPv6 | ping, IPv6 neighbour discovery |

Everything else is refused (TCP reset, ICMP unreachable), nothing is forwarded between interfaces.

- **More rules**: files ending in `.nft` in `/var/lib/lorawan-firewall/` are read inside the input chain, e.g.
  `tcp dport 9100 accept` in `/var/lib/lorawan-firewall/node-exporter.nft`; then
  `systemctl reload lorawan-firewall`. They live on the data partition, so they stay across updates.
- **Off**: `touch /var/lib/lorawan-firewall/disabled && systemctl stop lorawan-firewall`; on again: remove the
  file, `systemctl start lorawan-firewall`.
- `nft list ruleset` shows what is loaded. A factory reset removes the extra rules and the "off" switch.

## Read-only system

A gateway installed on the eMMC runs its system partition read-only (docs/updates.md): nothing changes the
installed system behind the update's back, and a restart always starts what the signed bundle contained. Settings
and data are on the data partition (`/var/lib`). Extra SSH keys go into `/var/lib/lorawan-ssh/authorized_keys`.

## Known vulnerabilities

Each full build checks the image's packages against the CVE List and NVD with
[sbom-cve-check](https://github.com/bootlin/sbom-cve-check) on its SPDX SBOM: the run's summary page and artifact
`cve-report` show the result, and a release carries it as `lorawan-gateway-os-<version>-bobcat-g285.cve.txt`.
