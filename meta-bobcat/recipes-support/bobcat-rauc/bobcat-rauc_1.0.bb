# SPDX-License-Identifier: MIT
SUMMARY = "A/B updates of the Bobcat G285 with RAUC"
DESCRIPTION = "Data partition mount for A/B boots, a read-only root file system and a journal on the data \
partition there, the RAUC post-install handler that carries the gateway's identity over to the new slot, and the \
tools RAUC needs on this board (docs/updates.md)."
LICENSE = "MIT"
LIC_FILES_CHKSUM = "file://${COMMON_LICENSE_DIR}/MIT;md5=0835ade698e0bcf8506ecda2f7b4f302"

inherit systemd

S = "${UNPACKDIR}"

SRC_URI = " \
    file://bobcat-rauc-post-install \
    file://var-lib.mount \
    file://10-bobcat-mark-good.conf \
    file://bobcat-ro-root.service \
    file://bobcat-journal.service \
    file://bobcat-boot-check.service \
    file://bobcat-boot-check.timer \
"

do_install() {
    install -d ${D}${libexecdir} ${D}${systemd_system_unitdir}
    install -m 0755 ${UNPACKDIR}/bobcat-rauc-post-install ${D}${libexecdir}/
    install -m 0644 ${UNPACKDIR}/var-lib.mount ${UNPACKDIR}/bobcat-ro-root.service ${UNPACKDIR}/bobcat-journal.service \
        ${UNPACKDIR}/bobcat-boot-check.service ${UNPACKDIR}/bobcat-boot-check.timer ${D}${systemd_system_unitdir}/
    install -d ${D}${systemd_system_unitdir}/rauc-mark-good.service.d
    install -m 0644 ${UNPACKDIR}/10-bobcat-mark-good.conf ${D}${systemd_system_unitdir}/rauc-mark-good.service.d/
}

SYSTEMD_SERVICE:${PN} = "var-lib.mount bobcat-ro-root.service bobcat-journal.service bobcat-boot-check.timer"
FILES:${PN} += "${systemd_system_unitdir}"

# rauc-service: the D-Bus service the panel talks to; rauc-mark-good: marks a slot good once it has booted;
# u-boot-env: /etc/fw_env.config, libubootenv-bin: fw_printenv/fw_setenv (RAUC's access to the U-Boot environment;
# meta-rauc only adds it when PREFERRED_PROVIDER_virtual/bootloader is set, which meta-rockchip does not).
RDEPENDS:${PN} = "rauc rauc-service rauc-mark-good u-boot-env libubootenv-bin e2fsprogs-resize2fs"
