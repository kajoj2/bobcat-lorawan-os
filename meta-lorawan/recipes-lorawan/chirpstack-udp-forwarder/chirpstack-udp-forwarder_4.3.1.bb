# SPDX-License-Identifier: MIT
SUMMARY = "ChirpStack UDP Forwarder"
DESCRIPTION = "Forwards LoRa frames between Concentratord (ZeroMQ) and network servers over the Semtech UDP \
protocol, e.g. The Things Network, next to the local ChirpStack. Off until servers are set on the panel."
HOMEPAGE = "https://github.com/chirpstack/chirpstack-udp-forwarder"
LICENSE = "MIT"
LIC_FILES_CHKSUM = "file://LICENSE;md5=bc4546f147d6f9892ca1b7d23bf41196"

inherit cargo cargo-update-recipe-crates systemd

SRC_URI = " \
    git://github.com/chirpstack/chirpstack-udp-forwarder.git;protocol=https;nobranch=1;tag=v${PV} \
    file://chirpstack-udp-forwarder.service \
    file://0001-Send-TX_ACK-without-JSON-when-the-downlink-was-accepted.patch \
"
SRCREV = "2a6980afe52f6f10f01beec21f1ccfe55fcbb955"

require ${BPN}-crates.inc

# Same as chirpstack-mqtt-forwarder: zmq-sys builds its own libzmq inside cargo_home (QA "buildpaths").
DEBUG_PREFIX_MAP:append = " -ffile-prefix-map=${UNPACKDIR}/cargo_home=${TARGET_DBGSRC_DIR}/cargo_home"

# chirpstack_api runs prost-build.
DEPENDS += "protobuf-native"
export PROTOC = "${STAGING_BINDIR_NATIVE}/protoc"

# Upstream's release profile strips the binary; leave that to Yocto, which splits the debug info into -dbg.
export CARGO_PROFILE_RELEASE_STRIP = "false"

do_install:append() {
    install -d ${D}${systemd_system_unitdir}
    install -m 0644 ${UNPACKDIR}/chirpstack-udp-forwarder.service ${D}${systemd_system_unitdir}/
}

# Enabled, but skipped by its ConditionPathExists until the panel writes a server list.
SYSTEMD_SERVICE:${PN} = "chirpstack-udp-forwarder.service"
