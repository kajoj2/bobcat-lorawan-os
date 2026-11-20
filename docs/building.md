<!-- SPDX-License-Identifier: MIT -->
# Building

## Host

- Linux x86-64 (any distribution kas-container supports), 8+ cores recommended, 16 GB RAM, ~150 GB free disk.
- [kas](https://kas.readthedocs.io/) 4.x: `pip install kas`. `kas-container` runs the build in the official
  kas image and needs only Docker or Podman on the host.

Once, before the first build: the keys that sign system updates (docs/updates.md). They stay on your machine,
next to the kas build directory; keep them, a gateway only takes updates signed with the keys its image was built
with.

```sh
tools/rauc-keys.sh ../rauc-keys           # from the repository; kas-container: inside its work directory
kas-container build kas/bobcat-g285.yml
kas-container build kas/bobcat-g285.yml --target lorawan-gateway-bundle   # the update bundle for the panel
```

A first build compiles the toolchain, Rust, Clang (for bindgen), Node.js (for the ChirpStack web interface) and the
kernel: plan for 1-3 hours on a desktop CPU. Later builds reuse the shared state cache (`SSTATE_DIR`).

Output in `build/tmp/deploy/images/bobcat-g285/`:

| File | What |
|---|---|
| `lorawan-gateway-image-bobcat-g285.rootfs.wic.xz` | SD card image |
| `lorawan-gateway-image-bobcat-g285.rootfs.wic.bmap` | block map for `bmaptool` |
| `lorawan-gateway-bundle-bobcat-g285.raucb` | update bundle for the panel's "System update" (with `--target lorawan-gateway-bundle`) |
| `idbloader.img`, `u-boot.itb` | boot loader parts (already inside the .wic) |
| `rk3566-bobcat-g285.dtb` | device tree (also in the image's `/boot`) |

## Pinned revisions

`kas/bobcat-g285.lock.yml` pins every layer to a commit; recipes pin their sources (`SRCREV`, crate checksums).
To move to newer layers: `tools/update-lock.py kas/bobcat-g285.yml --hold meta-rockchip` (needs PyYAML), then
build, test, and commit the lock file with the result. Not `kas lock`: it would move meta-rockchip to its master,
which no longer supports this release (see the kas file). The `updates` workflow does the same every Monday and
opens a pull request with each layer's new commits; it also keeps an issue listing recipes with newer upstream
releases (`tools/check-upstream.py`).

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
