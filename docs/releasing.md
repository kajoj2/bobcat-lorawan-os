# Releasing

A tag `v*` starts `.github/workflows/release.yml`:

1. The full build (`full-build.yml`), with the project's update CA built into the image instead of a throwaway one.
   It starts from the newest full build's shared state, so with an unchanged lock file it takes well under an hour.
2. The job `publish` waits for a maintainer to approve the `release` environment (the run's page, "Review
   deployments"). Look at the build first: the `images` artifact can go onto a board before you approve.
3. Once approved: the update bundle is signed again with the project's key (`rauc resign`) and checked against the
   CA, `SHA256SUMS` is written, every file gets a build provenance attestation, and the GitHub release is published
   with notes from the merged pull requests. A tag with a suffix (`v0.1.0-rc1`) makes a pre-release.

```sh
git tag -a v0.1.0 -m "LoRaWAN Gateway OS 0.1.0"
git push origin v0.1.0
```

## What the release holds

| File | What |
|---|---|
| `lorawan-gateway-image-bobcat-g285.rootfs.wic.xz` | the SD card image (docs/flashing.md) |
| `lorawan-gateway-image-bobcat-g285.rootfs.wic.bmap` | block map for `bmaptool copy` |
| `lorawan-gateway-image-bobcat-g285.rootfs.manifest` | the installed packages and versions |
| `lorawan-gateway-bundle-bobcat-g285.raucb` | the update bundle, signed with the project's key (docs/updates.md) |
| `SHA256SUMS` | checksums of the above |

Checking a download:

```sh
sha256sum -c SHA256SUMS
gh attestation verify lorawan-gateway-bundle-bobcat-g285.raucb -R kajoj2/bobcat-lorawan-os
```

The attestation proves the file came out of this repository's release workflow, from the tagged commit.

## Keys and settings, once

The release uses the update keys made with `tools/rauc-keys.sh` (docs/updates.md). The image only takes bundles
signed under the CA it carries, so these must be the keys the gateways in use already trust.

| Setting | Where | Value |
|---|---|---|
| variable `RAUC_CA_CERT` | repository (Settings, Secrets and variables, Actions, Variables) | `ca.cert.pem` (public) |
| environment `release` | Settings, Environments | required reviewers: the maintainers; deployment tags: `v*` |
| secret `RAUC_SIGNING_KEY` | environment `release` | `key.pem` |
| variable `RAUC_SIGNING_CERT` | environment `release` | `cert.pem` (public) |

With the GitHub CLI, from the directory that holds the keys:

```sh
gh variable set RAUC_CA_CERT < ca.cert.pem
gh secret set RAUC_SIGNING_KEY --env release < key.pem
gh variable set RAUC_SIGNING_CERT --env release < cert.pem
```

`ca.key.pem` never goes to GitHub: it only issues a new signing certificate, which needs no new image. Only the
jobs of the `release` environment see the signing key, after approval, and only for tags `v*`. If it leaks anyway,
every gateway keeps accepting bundles signed with it until its certificate expires (RAUC checks no revocation list
here): issue a new key and certificate under the same CA, replace the secret and the variable, and treat bundles
from elsewhere with suspicion until then.
