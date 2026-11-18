# SPDX-License-Identifier: MIT
# EXPERIMENTAL (branch experimental/boot-firmware, docs/experimental-boot-firmware.md): run Rockchip's DDR blob at
# BOBCAT_DDR_MHZ instead of its 1056 MHz. The RK3566 blobs in rkbin stop at 1056 MHz; the clock is changed with
# rkbin's own ddrbin_tool (meta-rockchip's RKBIN_DDR_RECONFIGURE), everything else in the blob stays as it is.

FILESEXTRAPATHS:prepend:bobcat := "${THISDIR}/files:"

BOBCAT_DDR_MHZ ??= "1056"
RKBIN_DDR_RECONFIGURE:bobcat = "${@'0' if d.getVar('BOBCAT_DDR_MHZ') == '1056' else '1'}"

do_configure:prepend:bobcat() {
	if [ "${RKBIN_DDR_RECONFIGURE}" = "1" ]; then
		sed -i "s/@BOBCAT_DDR_MHZ@/${BOBCAT_DDR_MHZ}/" ${UNPACKDIR}/ddrbin_param.txt
	fi
}
