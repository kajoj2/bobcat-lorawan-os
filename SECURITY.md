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
