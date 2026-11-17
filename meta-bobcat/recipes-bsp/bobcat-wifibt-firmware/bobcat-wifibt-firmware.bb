# SPDX-License-Identifier: MIT
SUMMARY = "Wi-Fi NVRAM and Bluetooth patch file for the Bobcat G285's AzureWave module (BCM43430A1)"
DESCRIPTION = "The calibration file (NVRAM) and Bluetooth firmware the G285's AzureWave module needs, under the \
names mainline brcmfmac and btbcm look for. The chip's Wi-Fi firmware itself comes from linux-firmware. \
These files are not kept in this repository: they are fetched from a public copy of Rockchip's rkwifibt \
package, pinned to a commit and checksums. The NVRAM is byte-identical to the one in the board's factory \
firmware. No license is stated for them upstream."
HOMEPAGE = "https://github.com/Inspectron/rkwifibt"
# No license is stated for the firmware files in rkwifibt (its NOTICE covers the Android scripts only).
LICENSE = "CLOSED"

RKWIFIBT_REV = "9a862443dd3ee9b392e240bf920932540748daa0"
RKWIFIBT_URI = "https://raw.githubusercontent.com/Inspectron/rkwifibt/${RKWIFIBT_REV}/firmware/broadcom/AW-NB197"

SRC_URI = " \
    ${RKWIFIBT_URI}/wifi/nvram_azw372.txt;name=nvram;downloadfilename=rkwifibt-${RKWIFIBT_REV}-nvram_azw372.txt \
    ${RKWIFIBT_URI}/bt/BCM4343A1.hcd;name=hcd;downloadfilename=rkwifibt-${RKWIFIBT_REV}-BCM4343A1.hcd \
"
SRC_URI[nvram.sha256sum] = "143d2c92b65f5e398cde2f20e56f3fc68d33d983b4a443deac17f9628a8e5575"
SRC_URI[hcd.sha256sum] = "dbd2928b074ce4af6976957f67d9d019335fcfa5752d7c547afae9929e0dbc72"

S = "${UNPACKDIR}"

COMPATIBLE_MACHINE = "bobcat"
PACKAGE_ARCH = "${MACHINE_ARCH}"

# brcmfmac asks for brcmfmac43430-sdio.<first DT compatible>.txt, btbcm for BCM43430A1.hcd
BOBCAT_DT_COMPATIBLE ?= "bobcat,miner300-g285"

do_install() {
    install -d ${D}${nonarch_base_libdir}/firmware/brcm
    install -m 0644 ${UNPACKDIR}/rkwifibt-${RKWIFIBT_REV}-nvram_azw372.txt \
        ${D}${nonarch_base_libdir}/firmware/brcm/brcmfmac43430-sdio.${BOBCAT_DT_COMPATIBLE}.txt
    install -m 0644 ${UNPACKDIR}/rkwifibt-${RKWIFIBT_REV}-BCM4343A1.hcd \
        ${D}${nonarch_base_libdir}/firmware/brcm/BCM43430A1.hcd
}

FILES:${PN} = "${nonarch_base_libdir}/firmware/brcm"
