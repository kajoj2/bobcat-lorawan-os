# SPDX-License-Identifier: MIT
SUMMARY = "Helium gateway (gateway-rs)"
DESCRIPTION = "Connects the gateway to the Helium IoT network as a data-only hotspot with its own key: takes \
Semtech UDP from chirpstack-udp-forwarder on localhost and passes the frames to Helium's packet router. Off until \
Helium is chosen on the panel."
HOMEPAGE = "https://github.com/helium/gateway-rs"
LICENSE = "Apache-2.0"
LIC_FILES_CHKSUM = "file://LICENSE;md5=1c3a461cb3bfa8dedcbf13b9e8a6335a"

inherit cargo cargo-update-recipe-crates systemd

# The last release (v1.3.0, 2023-10) pins a `time` crate that newer Rust no longer compiles; main's head has the
# update and nothing else of weight since.
PV = "1.3.0+git"
SRC_URI = " \
    git://github.com/helium/gateway-rs.git;protocol=https;nobranch=1 \
    git://github.com/helium/proto;protocol=https;nobranch=1;name=helium-proto;destsuffix=helium-proto \
    git://github.com/yoshuawuyts/exponential-backoff;protocol=https;nobranch=1;name=exponential-backoff;destsuffix=exponential-backoff \
    file://helium-gateway.service \
    file://settings.toml \
"
SRCREV = "b736b006af618d67433e0d4c19d626260fad6dcb"
# Git dependencies of Cargo.lock (listed at the end of the crates file). cargo_common.bbclass points Cargo at each
# checkout with a [patch] entry named after the SRC_URI's name=; the URLs above must match Cargo.lock's exactly.
SRCREV_helium-proto = "c828e0399f88ebbd6d13c8de39c25d70b9440f5b"
SRCREV_exponential-backoff = "5e889c9ba87a836456667c3bdeaf19888f89b7e0"
SRCREV_FORMAT = "default"

require ${BPN}-crates.inc

# The proto repository holds two crates: helium-proto at its root (patched by cargo_common) and beacon in a
# subdirectory, which the class cannot express. Add it to the same [patch] table.
helium_gateway_patch_beacon() {
    sed -i '/^\[patch\."https:\/\/github\.com\/helium\/proto"\]$/a beacon = { path = "${UNPACKDIR}/helium-proto/beacon" }' \
        ${CARGO_HOME}/config.toml
    grep -q '^beacon = ' ${CARGO_HOME}/config.toml || bbfatal "no [patch] table for helium/proto in ${CARGO_HOME}/config.toml"
}
do_configure[postfuncs] += "helium_gateway_patch_beacon"

# helium-proto runs prost-build and tonic-build.
DEPENDS += "protobuf-native"
export PROTOC = "${STAGING_BINDIR_NATIVE}/protoc"

# Upstream's release profile strips the binary; leave that to Yocto, which splits the debug info into -dbg.
export CARGO_PROFILE_RELEASE_STRIP = "false"

do_install:append() {
    # Upstream's default path, so `helium_gateway info key name` works without -c.
    install -d ${D}${sysconfdir}/helium_gateway
    sed -e 's|@REGION@|${LORAWAN_REGION}|g' ${UNPACKDIR}/settings.toml > ${D}${sysconfdir}/helium_gateway/settings.toml
    chmod 0644 ${D}${sysconfdir}/helium_gateway/settings.toml
    install -d ${D}${systemd_system_unitdir}
    install -m 0644 ${UNPACKDIR}/helium-gateway.service ${D}${systemd_system_unitdir}/
}

# Same default as lorawan-gateway-config: the region the concentrator runs in.
LORAWAN_REGION ??= "EU868"

CONFFILES:${PN} = "${sysconfdir}/helium_gateway/settings.toml"
# Enabled, but skipped by its ExecCondition until the panel's server list has the Helium entry.
SYSTEMD_SERVICE:${PN} = "helium-gateway.service"
