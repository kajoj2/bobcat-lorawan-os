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
   (or `bitbake -c update_crates <recipe>` on a build host). Git dependencies end up as `# git:` comments: the
   recipe fetches each repository with `name=` and `destsuffix=` and its own `SRCREV_<name>` (see
   `helium-gateway_1.3.0.bb`).
3. Update `LIC_FILES_CHKSUM` if the license file changed.
4. `tools/check-crates-inc.sh` checks every crate list against its recipe's `SRCREV` (CI runs it too).

## Checking the device tree without a build

```sh
tools/check-dts.sh             # sparse-clones the matching Linux tag (~30 MB) into a temp directory
tools/check-dts.sh ~/src/linux # or use an existing kernel tree
```

## What CI builds

GitHub's hosted runners (4 vCPU, a few tens of GB of disk, 6 h per job) cannot build the whole image: ChirpStack
needs the Rust toolchain, and a build from scratch takes an hour on 24 threads. So CI splits the work:

| Workflow | When | What |
|---|---|---|
| `ci` | every push and pull request | REUSE, device trees (`dtc`), shellcheck, oelint-adv, lorawan-agent tests and clippy, crate lists against `Cargo.lock`, `kas dump`, `bitbake --dry-run` of the image and the bundle |
| `build` | changes to `meta-bobcat/`, `kas/`, the distro config or libloragw; weekly | `kas/ci-board.yml`: cross toolchain, kernel with our device tree, U-Boot with TF-A, and our small recipes up to package QA |
| `full-build` | nightly when main changed; on demand; on the lock update pull request; on a pull request labelled `full-build` | the whole image and update bundle, in stages (toolchain; then the Rust toolchain with clang, Node.js and the rest of the packages side by side; ChirpStack; image) that hand their shared state on as artifacts; the image, its bmap and manifest and the bundle as artifact `images` |

`build` keeps its shared state in the GitHub Actions cache, keyed on the lock file: a run with an unchanged lock
reuses the toolchain and builds only what changed; a lock change starts over (90 min on the first run: 2580 tasks,
sstate 1.3 GB). Pull requests read the cache but never write it. `full-build` keeps all of its shared state as
artifact `sstate-full-<lock hash>` for 7 days (artifacts are not tied to a branch, and not to the 10 GB cache limit);
a later run with the same lock file starts from it, or from the stage artifacts of the newest earlier run, or from
`build`'s cache. A run from scratch takes about 10 runner hours, a run with an unchanged lock only rebuilds what
changed. The weekly `updates` workflow starts it on the lock pull request, so a new lock is merged once the image
builds, and main's next run starts from that shared state. Shared state is only taken from and kept by runs in this
repository, never by a pull request from a fork. A stage that runs out of time keeps what it built, and "Re-run failed
jobs" continues from there. The `images` artifact is for a board test: its bundle is signed with the run's throwaway
keys. Releases come from a tag (docs/releasing.md); the QEMU tests (docs/testing.md) run on a maintainer's build host.

