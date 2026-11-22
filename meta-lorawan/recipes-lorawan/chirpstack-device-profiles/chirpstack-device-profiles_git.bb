# SPDX-License-Identifier: MIT
SUMMARY = "ChirpStack's device profile repository, imported into the local ChirpStack"
DESCRIPTION = "Device profiles with payload codecs for many LoRaWAN devices (chirpstack/chirpstack-device-profiles), \
imported on the first boot and after each update that brings a newer copy, so a device can be added by choosing \
its vendor and model in ChirpStack's web interface instead of writing a codec."
HOMEPAGE = "https://github.com/chirpstack/chirpstack-device-profiles"
LICENSE = "MIT"
LIC_FILES_CHKSUM = "file://LICENSE;md5=${CHIRPSTACK_DEVICE_PROFILES_LICENSE_MD5}"
CHIRPSTACK_DEVICE_PROFILES_LICENSE_MD5 = "b31e05fd817c349de61f1969b179411c"

inherit systemd allarch

PV = "0+git"
SRC_URI = " \
    git://github.com/chirpstack/chirpstack-device-profiles.git;protocol=https;branch=master \
    file://chirpstack-device-profiles-import.service \
"
SRCREV = "8e6f1d4677de31ec0dd61d751c7d2a7ced620708"

do_configure[noexec] = "1"
do_compile[noexec] = "1"

do_install() {
    install -d ${D}${datadir}/chirpstack-device-profiles
    cp -R --no-preserve=ownership ${S}/vendors ${S}/interface ${S}/LICENSE ${D}${datadir}/chirpstack-device-profiles/
    # Product photos are not used by the import
    find ${D}${datadir}/chirpstack-device-profiles -type f \( -name '*.png' -o -name '*.jpg' -o -name '*.jpeg' \) -delete
    echo "${SRCREV}" > ${D}${datadir}/chirpstack-device-profiles/REVISION

    install -d ${D}${systemd_system_unitdir}
    install -m 0644 ${UNPACKDIR}/chirpstack-device-profiles-import.service ${D}${systemd_system_unitdir}/
}

SYSTEMD_SERVICE:${PN} = "chirpstack-device-profiles-import.service"
FILES:${PN} += "${systemd_system_unitdir}"
RDEPENDS:${PN} = "chirpstack"
