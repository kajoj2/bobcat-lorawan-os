# SPDX-License-Identifier: MIT
SUMMARY = "Status web panel, diagnostics, LED status and ChirpStack registration for the gateway"
DESCRIPTION = "lorawan-agent: a status panel on port 80 (updates from GitHub releases, backups, device alerts, \
Prometheus metrics, WireGuard remote access, BLE sensors to MQTT), the lorawan-diag report, the front LED as a \
health light, and the gateway added to the local ChirpStack on the first boot. One small Rust binary without crate \
dependencies."
HOMEPAGE = "https://github.com/kajoj2/bobcat-lorawan-os"
LICENSE = "MIT"
LIC_FILES_CHKSUM = "file://${COMMON_LICENSE_DIR}/MIT;md5=0835ade698e0bcf8506ecda2f7b4f302"

inherit cargo systemd

# Keep in step with lorawan-agent/Cargo.toml
PV = "0.1.0"

# The source lives in this repository (lorawan-agent/ at the top), not in a fetched tarball.
# nooelint: oelint.vars.fileextrapaths,oelint.vars.outofcontext
FILESEXTRAPATHS:prepend := "${THISDIR}/../../..:"
SRC_URI = "file://lorawan-agent/"
S = "${UNPACKDIR}/lorawan-agent"

# No crates to fetch: the crate has no dependencies (keep it that way, or add a -crates.inc).
CARGO_DISABLE_BITBAKE_VENDORING = "1"

# GitHub repository whose releases the update card offers (owner/name); empty turns that part of the card off.
LORAWAN_RELEASE_REPO ??= "kajoj2/bobcat-lorawan-os"

do_install:append() {
    ln -sf lorawan-agent ${D}${bindir}/lorawan-diag
    install -d ${D}${systemd_system_unitdir} ${D}${libexecdir} ${D}${datadir}/lorawan-agent
    install -m 0644 ${S}/systemd/lorawan-agent.service ${S}/systemd/lorawan-agent-register.service \
        ${S}/systemd/lorawan-backup.service ${S}/systemd/lorawan-restore.service \
        ${S}/systemd/lorawan-wireguard.service ${S}/systemd/lorawan-ble.service \
        ${D}${systemd_system_unitdir}/
    install -m 0755 ${S}/scripts/lorawan-backup ${D}${libexecdir}/
    echo "${LORAWAN_RELEASE_REPO}" > ${D}${datadir}/lorawan-agent/release-repo
}

# The tunnel and the BLE scanner start at boot only once set on the panel (ConditionPathExists); backup and
# restore are started by the panel.
SYSTEMD_SERVICE:${PN} = "lorawan-agent.service lorawan-agent-register.service lorawan-wireguard.service lorawan-ble.service"
FILES:${PN} += "${systemd_system_unitdir}"

# register runs `chirpstack create-api-key` as the chirpstack user; the panel follows Mosquitto. curl: release
# list, bundle download and webhooks (HTTPS); sqlite3, tar, gzip: backups; wireguard-tools: remote access.
RDEPENDS:${PN} = "chirpstack mosquitto curl ca-certificates sqlite3 tar gzip wireguard-tools"
