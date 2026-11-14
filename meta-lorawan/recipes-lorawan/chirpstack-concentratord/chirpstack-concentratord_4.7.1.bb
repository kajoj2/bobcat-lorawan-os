# SPDX-License-Identifier: MIT
SUMMARY = "ChirpStack Concentratord, LoRa concentrator daemon (SX1302 build)"
DESCRIPTION = "Drives the SX1302 concentrator through libloragw and exposes it over a ZeroMQ API, used by \
ChirpStack MQTT Forwarder. Handles the concentrator's reset and power lines itself (GPIO character device)."
HOMEPAGE = "https://github.com/chirpstack/chirpstack-concentratord"
LICENSE = "MIT"
LIC_FILES_CHKSUM = "file://LICENSE;md5=bc4546f147d6f9892ca1b7d23bf41196"

inherit cargo cargo-update-recipe-crates systemd

SRC_URI = " \
    git://github.com/chirpstack/chirpstack-concentratord.git;protocol=https;nobranch=1;tag=v${PV} \
    file://0001-sx1302-add-Bobcat-Miner-300-G285-model.patch \
    file://chirpstack-concentratord.service \
"
SRCREV = "0904a8ddf4eeb3150b4675b35f067865cb68827d"

require ${BPN}-crates.inc

# C/C++ that crates build themselves (vendored libzmq in zmq-sys, librdkafka, ...) lives in cargo_home under
# UNPACKDIR, which the default prefix maps do not cover, so __FILE__ in its asserts would leak TMPDIR into the
# binary (QA "buildpaths").
DEBUG_PREFIX_MAP_EXTRA = "-ffile-prefix-map=${UNPACKDIR}=${TARGET_DBGSRC_DIR}"

# Build only the SX1302 daemon of the workspace; the lock file is the workspace's.
CARGO_SRC_DIR = "chirpstack-concentratord-sx1302"
CARGO_LOCK_PATH = "${S}/Cargo.lock"

# libloragw-sx1302 (static) is bound with bindgen, which needs libclang; prost-build needs protoc. zmq-sys always
# builds its own libzmq (zeromq-src), so no zeromq dependency.
DEPENDS += "libloragw-sx1302 clang-native protobuf-native"

export PROTOC = "${STAGING_BINDIR_NATIVE}/protoc"
export LIBCLANG_PATH = "${STAGING_LIBDIR_NATIVE}"
export BINDGEN_EXTRA_CLANG_ARGS = "--sysroot=${STAGING_DIR_TARGET} --target=${TARGET_SYS} ${TUNE_CCARGS}"

# The daemon's build.rs links libstdc++ statically for upstream's musl release builds; we link the normal shared
# runtime (OpenWrt's package removes the same file).
do_configure:prepend() {
    rm -f ${S}/chirpstack-concentratord-sx1302/build.rs
}

do_install:append() {
    install -d ${D}${sysconfdir}/chirpstack-concentratord/examples
    install -m 0644 ${S}/chirpstack-concentratord-sx1302/config/*.toml ${D}${sysconfdir}/chirpstack-concentratord/examples/
    install -d ${D}${systemd_system_unitdir}
    install -m 0644 ${UNPACKDIR}/chirpstack-concentratord.service ${D}${systemd_system_unitdir}/
}

SYSTEMD_SERVICE:${PN} = "chirpstack-concentratord.service"
