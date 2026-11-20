# SPDX-License-Identifier: MIT
SUMMARY = "Semtech SX1302/SX1303 LoRa concentrator HAL (ChirpStack fork)"
DESCRIPTION = "Static libloragw for the SX1302 + SX1250 concentrators, as linked by ChirpStack Concentratord, and \
the chip_id utility for checking a concentrator during bring-up. ChirpStack's fork of Semtech's sx1302_hal, which \
also tolerates boards without the STTS751 temperature sensor."
HOMEPAGE = "https://github.com/brocaar/sx1302_hal"
LICENSE = "BSD-3-Clause"
LIC_FILES_CHKSUM = "file://LICENSE.TXT;md5=d2119120bd616e725f4580070bd9ee19"

SRC_URI = "git://github.com/brocaar/sx1302_hal.git;protocol=https;nobranch=1;tag=V${PV}"
SRCREV = "587f4c28ecab91a96e65ac3b2686101a2b768727"

# The Makefiles set CC with ':=' from CROSS_COMPILE; a command-line CC overrides that and keeps their CFLAGS.
EXTRA_OEMAKE = "CC='${CC}' AR='${AR}'"

do_compile() {
    oe_runmake -C libtools
    oe_runmake -C libloragw libloragw.a
    oe_runmake -C util_chip_id CC='${CC} ${LDFLAGS}'
}

do_install() {
    install -d ${D}${libdir} ${D}${includedir}/libloragw-sx1302 ${D}${bindir}
    install -m 0644 libloragw/libloragw.a ${D}${libdir}/libloragw-sx1302.a
    install -m 0644 libtools/libtinymt32.a libtools/libparson.a libtools/libbase64.a ${D}${libdir}/
    install -m 0644 libloragw/inc/*.h ${D}${includedir}/libloragw-sx1302/
    install -m 0644 libtools/inc/*.h ${D}${includedir}/
    install -m 0755 util_chip_id/chip_id ${D}${bindir}/sx1302-chip-id
}

PACKAGES =+ "${PN}-utils"
# nooelint: oelint.var.filesoverride
FILES:${PN}-utils = "${bindir}/sx1302-chip-id"
# Static libraries only; the -staticdev package is what Concentratord builds against.
ALLOW_EMPTY:${PN} = "1"
