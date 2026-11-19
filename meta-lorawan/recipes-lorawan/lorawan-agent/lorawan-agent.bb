# SPDX-License-Identifier: MIT
SUMMARY = "Status web panel, diagnostics, LED status and ChirpStack registration for the gateway"
DESCRIPTION = "lorawan-agent: a status panel on port 80, the lorawan-diag report, the front LED as a health light, \
and the gateway added to the local ChirpStack on the first boot. One small Rust binary without crate dependencies."
HOMEPAGE = "https://github.com/kajoj2/bobcat-lorawan-os"
LICENSE = "MIT"
LIC_FILES_CHKSUM = "file://${COMMON_LICENSE_DIR}/MIT;md5=0835ade698e0bcf8506ecda2f7b4f302"

inherit cargo systemd

# Keep in step with lorawan-agent/Cargo.toml
PV = "0.1.0"

# The source lives in this repository (lorawan-agent/ at the top), not in a fetched tarball.
FILESEXTRAPATHS:prepend := "${THISDIR}/../../..:"
SRC_URI = "file://lorawan-agent/"
S = "${UNPACKDIR}/lorawan-agent"

# No crates to fetch: the crate has no dependencies (keep it that way, or add a -crates.inc).
CARGO_DISABLE_BITBAKE_VENDORING = "1"

do_install:append() {
    ln -sf lorawan-agent ${D}${bindir}/lorawan-diag
    install -d ${D}${systemd_system_unitdir}
    install -m 0644 ${S}/systemd/lorawan-agent.service ${S}/systemd/lorawan-agent-register.service \
        ${D}${systemd_system_unitdir}/
}

SYSTEMD_SERVICE:${PN} = "lorawan-agent.service lorawan-agent-register.service"

# register runs `chirpstack create-api-key` as the chirpstack user; the panel follows Mosquitto.
RDEPENDS:${PN} = "chirpstack mosquitto"
