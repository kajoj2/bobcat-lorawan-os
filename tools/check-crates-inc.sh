#!/bin/sh
# SPDX-License-Identifier: MIT
#
# Checks that every Rust recipe's <name>-crates.inc matches the Cargo.lock of the upstream revision the recipe
# fetches (SRCREV), by regenerating it with tools/cargo-crates-inc.py. Catches a version bump without a new crate
# list (the build would fail late, in do_compile, on a missing or wrong crate).
#
#   tools/check-crates-inc.sh
set -eu

cd "$(dirname "$0")/.."
status=0
for inc in $(git ls-files 'meta-*/*-crates.inc'); do
    dir=$(dirname "$inc")
    name=$(basename "$inc" -crates.inc)
    bb=$(find "$dir" -maxdepth 1 -name "${name}_*.bb" | head -n 1)
    [ -n "$bb" ] || { echo "$inc: no ${name}_<version>.bb next to it"; status=1; continue; }
    repo=$(sed -n 's|.*git://github\.com/\([A-Za-z0-9._-]*/[A-Za-z0-9._-]*\)\.git;.*|\1|p' "$bb" | head -n 1)
    rev=$(sed -n 's|^SRCREV = "\([0-9a-f]\{40\}\)"|\1|p' "$bb")
    if [ -z "$repo" ] || [ -z "$rev" ]; then
        echo "$bb: no GitHub SRC_URI or SRCREV found"
        status=1
        continue
    fi
    if tools/cargo-crates-inc.py "https://raw.githubusercontent.com/$repo/$rev/Cargo.lock" "$name" | diff -u "$inc" -; then
        echo "$inc: matches $repo@${rev%"${rev#????????????}"}"
    else
        echo "$inc: differs from the Cargo.lock of $repo@$rev (regenerate it, docs/building.md)"
        status=1
    fi
done
exit $status
