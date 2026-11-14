#!/bin/sh
# SPDX-License-Identifier: MIT
#
# Compile the board device trees the way kbuild does (cpp, then dtc), against a
# Linux source tree, without building a kernel. Catches syntax errors, unknown
# labels and pin groups, and dtc warnings early (CI and local use).
#
#   tools/check-dts.sh [LINUX_SRC]
#
# Without LINUX_SRC it makes a sparse, blob-less clone of the Linux tag below
# (device tree sources and dt-bindings only, ~30 MB) in a temporary directory.
set -eu

LINUX_TAG=${LINUX_TAG:-v6.18}
here=$(cd "$(dirname "$0")/.." && pwd)
dts_dir="$here/meta-bobcat/recipes-kernel/linux/files/dts"

if [ $# -ge 1 ]; then
	linux=$1
else
	tmp=$(mktemp -d)
	trap 'rm -rf "$tmp"' EXIT
	linux="$tmp/linux"
	git clone -q --depth 1 --filter=blob:none --sparse -b "$LINUX_TAG" \
		https://github.com/torvalds/linux.git "$linux"
	git -C "$linux" sparse-checkout set arch/arm64/boot/dts include/dt-bindings include/uapi/linux scripts/dtc/include-prefixes
fi

out=$(mktemp -d)
status=0
for dts in "$dts_dir"/*.dts; do
	name=$(basename "$dts" .dts)
	cp "$dts" "$linux/arch/arm64/boot/dts/rockchip/"
	${CC:-cc} -E -nostdinc -undef -D__DTS__ -x assembler-with-cpp \
		-I "$linux/arch/arm64/boot/dts/rockchip" \
		-I "$linux/scripts/dtc/include-prefixes" \
		-I "$linux/include" \
		-o "$out/$name.pre.dts" "$linux/arch/arm64/boot/dts/rockchip/$name.dts"
	# The same checks the kernel turns off for a W=0 build
	if dtc -I dts -O dtb -o "$out/$name.dtb" \
		-Wno-unit_address_vs_reg -Wno-avoid_unnecessary_addr_size \
		-Wno-alias_paths -Wno-simple_bus_reg \
		"$out/$name.pre.dts"; then
		echo "ok   $name.dtb ($(wc -c <"$out/$name.dtb" | tr -d ' ') bytes)"
	else
		echo "FAIL $name"
		status=1
	fi
	rm -f "$linux/arch/arm64/boot/dts/rockchip/$name.dts"
done
rm -rf "$out"
exit $status
