# SPDX-License-Identifier: MIT
# docs/boot-firmware.md: a U-Boot configuration for
# the G285 with the board's own device tree instead of generic-rk3568_defconfig. Selected with BOBCAT_UBOOT = "board"
# (conf/machine/bobcat-g285.conf). U-Boot builds the same .dts as the kernel; rk3566-bobcat-g285-u-boot.dtsi adds
# what only U-Boot needs.

FILESEXTRAPATHS:prepend:bobcat := "${THISDIR}/files:${THISDIR}/../../recipes-kernel/linux/files/dts:"

SRC_URI:append:bobcat = " \
    file://bobcat-g285-rk3566_defconfig \
    file://rk3566-bobcat-g285-u-boot.dtsi \
    file://rk3566-bobcat-g285.dts \
"

# With CONFIG_OF_UPSTREAM the board .dts sits with the upstream Linux trees in dts/upstream, and U-Boot picks up
# arch/arm/dts/<board>-u-boot.dtsi by name.
do_configure:prepend:bobcat() {
	install -m 0644 ${UNPACKDIR}/bobcat-g285-rk3566_defconfig ${S}/configs/
	install -m 0644 ${UNPACKDIR}/rk3566-bobcat-g285.dts ${S}/dts/upstream/src/arm64/rockchip/
	install -m 0644 ${UNPACKDIR}/rk3566-bobcat-g285-u-boot.dtsi ${S}/arch/arm/dts/
}
