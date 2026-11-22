# SPDX-License-Identifier: MIT
SUMMARY = "System settings of the gateway: firewall, compressed swap, journal limits, periodic TRIM"
DESCRIPTION = "nftables rules that let in only the gateway's own services (lorawan-firewall), half the RAM as zstd \
compressed swap (lorawan-zram), size limits for a journal kept on disk, and the weekly fstrim of util-linux for \
the SD card and eMMC."
HOMEPAGE = "https://github.com/kajoj2/bobcat-lorawan-os"
LICENSE = "MIT"
LIC_FILES_CHKSUM = "file://${COMMON_LICENSE_DIR}/MIT;md5=0835ade698e0bcf8506ecda2f7b4f302"

inherit systemd allarch

# Only local files, unpacked straight into UNPACKDIR
S = "${UNPACKDIR}"

SRC_URI = " \
    file://lorawan-firewall.nft \
    file://lorawan-firewall.service \
    file://lorawan-zram \
    file://lorawan-zram.service \
    file://10-lorawan-journald.conf \
    file://lorawan-fstrim.service \
    file://lorawan-fstrim.timer \
"

do_install() {
    install -d ${D}${datadir}/lorawan-firewall ${D}${libexecdir} ${D}${systemd_system_unitdir}
    install -m 0644 ${UNPACKDIR}/lorawan-firewall.nft ${D}${datadir}/lorawan-firewall/firewall.nft
    install -m 0755 ${UNPACKDIR}/lorawan-zram ${D}${libexecdir}/
    install -m 0644 ${UNPACKDIR}/lorawan-firewall.service ${UNPACKDIR}/lorawan-zram.service \
        ${UNPACKDIR}/lorawan-fstrim.service ${UNPACKDIR}/lorawan-fstrim.timer ${D}${systemd_system_unitdir}/

    install -d ${D}${systemd_unitdir}/journald.conf.d
    install -m 0644 ${UNPACKDIR}/10-lorawan-journald.conf ${D}${systemd_unitdir}/journald.conf.d/
}

# util-linux does not install its fstrim units, hence our own timer
SYSTEMD_SERVICE:${PN} = "lorawan-firewall.service lorawan-zram.service lorawan-fstrim.timer"

FILES:${PN} += "${datadir}/lorawan-firewall ${systemd_system_unitdir} ${systemd_unitdir}/journald.conf.d"
RDEPENDS:${PN} = "nftables util-linux-fstrim util-linux-zramctl util-linux-mkswap util-linux-swaponoff kmod"
