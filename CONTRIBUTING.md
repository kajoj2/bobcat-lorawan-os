<!-- SPDX-License-Identifier: MIT -->
# Contributing

Thanks for helping. The project is young; the most valuable contributions right now are **facts from real
hardware**: boot logs, `dmesg`, photos, register dumps, test results, especially from board revisions other than
G285.

## Ground rules

- **Evidence over assumptions.** A hardware fact in `docs/hardware-g285.md` needs a source: a log, a measurement, a
  device tree from a board, a datasheet. Say how you know.
- **No proprietary binaries in the repository.** Firmware and blobs are fetched by recipes from their upstream
  (e.g. `rkbin`, `linux-firmware`) with pinned revisions.
- **No copying from sources without a license.** Facts are fine (credit them); code and text need a compatible
  license.
- **No personal data** in commits, logs or examples: no serial numbers, MAC addresses, keys, Wi-Fi passwords,
  gateway EUIs of real units. Redact them in logs you attach.
- **Pin everything.** Layer revisions in the kas lock file, sources by `SRCREV`/checksum, no `AUTOREV`, no
  floating tags.

## Pull requests

- One topic per pull request; explain what you tested and on which board revision.
- Every commit carries a `Signed-off-by:` line (`git commit -s`), certifying the
  [Developer Certificate of Origin](https://developercertificate.org/).
<!-- REUSE-IgnoreStart -->
- New files start with an SPDX header (`SPDX-License-Identifier: MIT`; device trees
  `(GPL-2.0-or-later OR MIT)`). CI runs `reuse lint`.
<!-- REUSE-IgnoreEnd -->
- Device tree changes: run `tools/check-dts.sh` (CI does too).
- Shell scripts: POSIX sh, `shellcheck` clean.

## Style

- English everywhere (code, comments, docs, commit messages).
- Commit subject: `<area>: <what>` in the imperative, e.g. `meta-bobcat: fix RTC bus`.
- Kernel-style tabs in device trees; BitBake files follow the
  [OE style guide](https://docs.yoctoproject.org/contributor-guide/recipe-style-guide.html).
