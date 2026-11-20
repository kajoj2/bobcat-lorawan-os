# SPDX-License-Identifier: MIT
SUMMARY = "Back up the Bobcat G285's eMMC and install the system from the SD card onto it"
DESCRIPTION = "bobcat-emmc: eMMC backup items (used blocks only, or the whole device) and an installer that \
writes an A/B layout (uboot, system_a, system_b, data) to the eMMC. Used by lorawan-agent's panel."
LICENSE = "MIT"
LIC_FILES_CHKSUM = "file://${COMMON_LICENSE_DIR}/MIT;md5=0835ade698e0bcf8506ecda2f7b4f302"

SRC_URI = " \
    file://bobcat-emmc \
    file://bobcat-emmc-install.service \
"
S = "${UNPACKDIR}"

inherit systemd allarch

SYSTEMD_SERVICE:${PN} = "bobcat-emmc-install.service"
# Started on request only, never at boot.
SYSTEMD_AUTO_ENABLE = "disable"

do_install() {
    install -D -m 0755 ${S}/bobcat-emmc ${D}${sbindir}/bobcat-emmc
    install -D -m 0644 ${S}/bobcat-emmc-install.service ${D}${systemd_system_unitdir}/bobcat-emmc-install.service
}

RDEPENDS:${PN} = " \
    e2fsprogs \
    e2fsprogs-e2fsck \
    e2fsprogs-mke2fs \
    e2fsprogs-resize2fs \
    e2fsprogs-tune2fs \
    util-linux-sfdisk \
    util-linux-fsfreeze \
    zstd \
"
