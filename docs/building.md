<!-- SPDX-License-Identifier: MIT -->
# Building

## Host

- Linux x86-64 (any distribution kas-container supports), 8+ cores recommended, 16 GB RAM, ~150 GB free disk.
- [kas](https://kas.readthedocs.io/) 4.x: `pip install kas`. `kas-container` runs the build in the official
  kas image and needs only Docker or Podman on the host.

```sh
kas-container build kas/bobcat-g285.yml
```

A first build compiles the toolchain, Rust, Clang (for bindgen), Node.js (for the ChirpStack web interface) and the
kernel: plan for 1-3 hours on a desktop CPU. Later builds reuse the shared state cache (`SSTATE_DIR`).

Output in `build/tmp/deploy/images/bobcat-g285/`:

| File | What |
|---|---|
| `lorawan-gateway-image-bobcat-g285.rootfs.wic.xz` | SD card image |
| `lorawan-gateway-image-bobcat-g285.rootfs.wic.bmap` | block map for `bmaptool` |
| `idbloader.img`, `u-boot.itb` | boot loader parts (already inside the .wic) |
| `rk3566-bobcat-g285.dtb` | device tree (also in the image's `/boot`) |

## Pinned revisions

`kas/bobcat-g285.lock.yml` pins every layer to a commit; recipes pin their sources (`SRCREV`, crate checksums).
To move to newer layers: `kas lock kas/bobcat-g285.yml`, build, test, commit the lock file with the result.

## Network access during the build

One task needs the network beyond the normal fetch: `chirpstack:do_compile_ui` installs the web interface's npm
packages with pnpm (the same way ChirpStack's own OpenWrt package does). Everything else is fetched in
`do_fetch` and can be mirrored (`DL_DIR`, `BB_GENERATE_MIRROR_TARBALLS`). Making the UI build offline is an open
item.

## Updating a Rust component

1. Bump the version and `SRCREV` in the recipe.
2. Regenerate the crate list from the new `Cargo.lock`:
   `tools/cargo-crates-inc.py https://raw.githubusercontent.com/chirpstack/<repo>/v<version>/Cargo.lock <recipe> > <recipe>-crates.inc`
   (or `bitbake -c update_crates <recipe>` on a build host).
3. Update `LIC_FILES_CHKSUM` if the license file changed.

## Checking the device tree without a build

```sh
tools/check-dts.sh             # sparse-clones the matching Linux tag (~30 MB) into a temp directory
tools/check-dts.sh ~/src/linux # or use an existing kernel tree
```
