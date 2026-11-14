# SPDX-License-Identifier: MIT
SUMMARY = "ChirpStack open-source LoRaWAN Network Server"
DESCRIPTION = "LoRaWAN network and application server with a web interface, built with the SQLite backend \
(no PostgreSQL), as ChirpStack Gateway OS does."
HOMEPAGE = "https://www.chirpstack.io/"
LICENSE = "MIT"
LIC_FILES_CHKSUM = "file://LICENSE;md5=e52a56a5b272102f5f53d67fdc2dd122"

inherit cargo cargo-update-recipe-crates systemd useradd

SRC_URI = " \
    git://github.com/chirpstack/chirpstack.git;protocol=https;nobranch=1;tag=v${PV} \
    file://chirpstack.service \
"
SRCREV = "8b5688584c60fca78ae756d90a54f681faa5bee6"

require ${BPN}-crates.inc

# C/C++ that crates build themselves (vendored libzmq in zmq-sys, librdkafka, ...) lives in cargo_home under
# UNPACKDIR, which the default prefix maps do not cover, so __FILE__ in its asserts would leak TMPDIR into the
# binary (QA "buildpaths"). An append, because rust-common.bbclass empties DEBUG_PREFIX_MAP_EXTRA for gcc.
DEBUG_PREFIX_MAP:append = " -ffile-prefix-map=${UNPACKDIR}/cargo_home=${TARGET_DBGSRC_DIR}/cargo_home"

CARGO_SRC_DIR = "chirpstack"
CARGO_LOCK_PATH = "${S}/Cargo.lock"
PACKAGECONFIG_CONFARGS = "--no-default-features --features sqlite"

# protoc for the API, libclang for rquickjs' bindgen, cmake for the bundled librdkafka (Kafka integration),
# Node.js for the web interface.
DEPENDS += "protobuf-native clang-native cmake-native sqlite3 nodejs-native"

export PROTOC = "${STAGING_BINDIR_NATIVE}/protoc"
export LIBCLANG_PATH = "${STAGING_LIBDIR_NATIVE}"
export BINDGEN_EXTRA_CLANG_ARGS = "--sysroot=${STAGING_DIR_TARGET} --target=${TARGET_SYS} ${TUNE_CCARGS}"

# The web interface is embedded into the binary (rust-embed of ui/build). It is a pnpm + Vite project whose
# packages come from the npm registry, so this one task needs the network; everything else builds offline from
# the fetched sources. Like upstream's OpenWrt package, it takes the published gRPC-web API package of the same
# version instead of generating it from the .proto files.
PNPM_VERSION = "10.34.9"
do_compile_ui[network] = "1"
do_compile_ui[dirs] = "${S}/ui"
do_compile_ui() {
    export HOME="${WORKDIR}/ui-home"
    export npm_config_cache="${WORKDIR}/ui-home/npm-cache"
    export COREPACK_HOME="${WORKDIR}/ui-home/corepack"
    pnpm="npx --yes pnpm@${PNPM_VERSION}"
    sed -i 's|"@chirpstack/chirpstack-api-grpc-web": "file:../api/grpc-web"|"@chirpstack/chirpstack-api-grpc-web": "${PV}"|' package.json
    $pnpm install --no-frozen-lockfile
    $pnpm build
    test -f build/index.html
}
addtask compile_ui after do_configure before do_compile

do_install:append() {
    install -d ${D}${sysconfdir}/chirpstack
    install -m 0644 ${S}/chirpstack/configuration/region_*.toml ${D}${sysconfdir}/chirpstack/
    install -d ${D}${systemd_system_unitdir}
    install -m 0644 ${UNPACKDIR}/chirpstack.service ${D}${systemd_system_unitdir}/
}

USERADD_PACKAGES = "${PN}"
USERADD_PARAM:${PN} = "--system --home-dir /var/lib/chirpstack --no-create-home --shell /sbin/nologin chirpstack"

SYSTEMD_SERVICE:${PN} = "chirpstack.service"
RDEPENDS:${PN} += "redis mosquitto"
