<!-- SPDX-License-Identifier: MIT -->
# Testing on QEMU

The gateway image also builds for `qemuarm64`, and Yocto's runtime tests (`testimage`) boot it in QEMU and check it
right after the build. This covers everything but the radio, so it works without a board:

```sh
kas-container build kas/qemuarm64.yml
```

A failing test fails the build. Results: `build/tmp/log/oeqa/`, the boot log in
`build/tmp/work/qemuarm64-*/lorawan-gateway-image/*/testimage/`.

## What is checked

`meta-lorawan/lib/oeqa/runtime/cases/lorawan.py`, after Yocto's own `ping` and `ssh` tests:

- boot finishes (`running`, or `degraded` from the missing concentrator only), no other unit failed;
- Redis, Mosquitto, ChirpStack, the MQTT Forwarder, systemd-networkd and Avahi are active;
- the first-boot secret exists, is long enough and readable by root only;
- ChirpStack created its SQLite database and serves the web interface on port 8080;
- a client is connected to Mosquitto, and a publish/subscribe round trip works;
- Mosquitto and Redis listen on localhost only;
- the key-only SSH policy is installed, and Concentratord's configuration and region files are in place.

## Differences from the board image

- `chirpstack-concentratord` fails and restarts every few seconds: there is no SX1302 (model set to `rak_2287`
  only to fill the configuration).
- `lorawan-test-access` lets the test harness log in as root over SSH with the empty password. The recipe only
  builds for QEMU machines.
- No KVM for arm64 on an x86 host: QEMU emulates the CPU, so a boot takes a few minutes.

## Running the image by hand

```sh
kas-container shell kas/qemuarm64.yml -c 'runqemu nographic slirp'
```

Log in as `root` on the console; with `slirp`, port 2222 on the host goes to the guest's SSH and the web interface
is not forwarded (add `hostfwd=tcp::8080-:8080` to `QB_SLIRP_OPT` for it).
