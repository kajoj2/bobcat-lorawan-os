# SPDX-License-Identifier: MIT
SUMMARY = "Root login over SSH with the empty password, for Yocto's runtime tests on QEMU"
DESCRIPTION = "The test harness logs in as root without a key. This package lets sshd accept that; it is limited to \
the QEMU machines so it can never end up in an image for real hardware."
LICENSE = "MIT"
LIC_FILES_CHKSUM = "file://${COMMON_LICENSE_DIR}/MIT;md5=0835ade698e0bcf8506ecda2f7b4f302"

COMPATIBLE_MACHINE = "^qemu.*"
PACKAGE_ARCH = "${MACHINE_ARCH}"

SRC_URI = "file://10-test-access.conf"

do_install() {
    install -d ${D}${sysconfdir}/ssh/sshd_config.d
    install -m 0644 ${UNPACKDIR}/10-test-access.conf ${D}${sysconfdir}/ssh/sshd_config.d/
}

RDEPENDS:${PN} = "openssh-sshd"
