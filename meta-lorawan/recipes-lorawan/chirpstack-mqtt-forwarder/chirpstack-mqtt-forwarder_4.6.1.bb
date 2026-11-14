# SPDX-License-Identifier: MIT
SUMMARY = "ChirpStack MQTT Forwarder"
DESCRIPTION = "Forwards LoRa frames between Concentratord (ZeroMQ) and an MQTT broker, where ChirpStack picks them up."
HOMEPAGE = "https://github.com/chirpstack/chirpstack-mqtt-forwarder"
LICENSE = "MIT"
LIC_FILES_CHKSUM = "file://LICENSE;md5=bc4546f147d6f9892ca1b7d23bf41196"

inherit cargo cargo-update-recipe-crates systemd

SRC_URI = " \
    git://github.com/chirpstack/chirpstack-mqtt-forwarder.git;protocol=https;nobranch=1;tag=v${PV} \
    file://chirpstack-mqtt-forwarder.service \
"
SRCREV = "25ee84684a77b2cae160bc392d7f1d7e0fa0385b"

require ${BPN}-crates.inc

DEPENDS += "protobuf-native zeromq"
export PROTOC = "${STAGING_BINDIR_NATIVE}/protoc"

do_install:append() {
    install -d ${D}${systemd_system_unitdir}
    install -m 0644 ${UNPACKDIR}/chirpstack-mqtt-forwarder.service ${D}${systemd_system_unitdir}/
}

SYSTEMD_SERVICE:${PN} = "chirpstack-mqtt-forwarder.service"
