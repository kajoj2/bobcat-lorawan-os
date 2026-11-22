# SPDX-License-Identifier: MIT
SUMMARY = "Setup access point, Wi-Fi join and factory reset for the gateway's setup wizard"
DESCRIPTION = "The privileged half of the setup wizard on the panel (lorawan-agent, /setup): an open setup access \
point with a DHCP server (hostapd, systemd-networkd) while the gateway is being set up, and joining the Wi-Fi \
network chosen there, with a fall back to the access point when that fails; and the factory reset behind the \
board's button. See docs/setup.md."
HOMEPAGE = "https://github.com/kajoj2/bobcat-lorawan-os"
LICENSE = "MIT"
LIC_FILES_CHKSUM = "file://${COMMON_LICENSE_DIR}/MIT;md5=0835ade698e0bcf8506ecda2f7b4f302"

inherit systemd allarch

# Only local files, unpacked straight into UNPACKDIR
S = "${UNPACKDIR}"

SRC_URI = " \
    file://lorawan-setup \
    file://lorawan-setup-ap.service \
    file://lorawan-setup-wifi.service \
    file://lorawan-setup-reset.service \
"

do_install() {
    install -d ${D}${libexecdir} ${D}${systemd_system_unitdir}
    install -m 0755 ${UNPACKDIR}/lorawan-setup ${D}${libexecdir}/
    install -m 0644 ${UNPACKDIR}/lorawan-setup-ap.service ${UNPACKDIR}/lorawan-setup-wifi.service \
        ${UNPACKDIR}/lorawan-setup-reset.service ${D}${systemd_system_unitdir}/
}

# Started by lorawan-agent when needed, never at boot.
SYSTEMD_SERVICE:${PN} = "lorawan-setup-ap.service lorawan-setup-wifi.service lorawan-setup-reset.service"
SYSTEMD_AUTO_ENABLE = "disable"

FILES:${PN} += "${systemd_system_unitdir}"
RDEPENDS:${PN} = "hostapd iw wpa-supplicant wpa-supplicant-cli iproute2-ip"
