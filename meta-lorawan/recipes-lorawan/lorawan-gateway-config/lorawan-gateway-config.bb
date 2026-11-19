# SPDX-License-Identifier: MIT
SUMMARY = "Configuration that turns the services into one LoRaWAN gateway"
DESCRIPTION = "Concentratord board/region configuration, MQTT Forwarder and ChirpStack configuration, first-boot \
secret generation, wired and Wi-Fi networking over DHCP (online when either is), key-only SSH and the hardware watchdog."
LICENSE = "MIT"
LIC_FILES_CHKSUM = "file://${COMMON_LICENSE_DIR}/MIT;md5=0835ade698e0bcf8506ecda2f7b4f302"

inherit systemd

SRC_URI = " \
    file://chirpstack.toml \
    file://concentratord.toml \
    file://chirpstack-mqtt-forwarder.toml \
    file://lorawan-secrets.sh \
    file://lorawan-secrets.service \
    file://80-wired.network \
    file://80-wireless.network \
    file://10-lorawan-wpa-supplicant.conf \
    file://50-lorawan-sshd.conf \
    file://10-lorawan-watchdog.conf \
    file://10-lorawan-wait-online.conf \
"

# Set by the machine (board layer): the Concentratord model of the board's concentrator.
LORAWAN_CONCENTRATOR_MODEL ??= ""
# LoRaWAN region in Concentratord's spelling, and the matching ChirpStack region / channel plan id.
LORAWAN_REGION ??= "EU868"
LORAWAN_REGION_ID ??= "${@d.getVar('LORAWAN_REGION').lower()}"

# The model is machine specific, so is this package.
PACKAGE_ARCH = "${MACHINE_ARCH}"

python () {
    if not d.getVar('LORAWAN_CONCENTRATOR_MODEL'):
        raise bb.parse.SkipRecipe("LORAWAN_CONCENTRATOR_MODEL is not set for this machine")
}

do_install() {
    subst() {
        sed -e 's|@MODEL@|${LORAWAN_CONCENTRATOR_MODEL}|g' \
            -e 's|@REGION@|${LORAWAN_REGION}|g' \
            -e 's|@REGION_ID@|${LORAWAN_REGION_ID}|g' "$1"
    }

    install -d ${D}${sysconfdir}/chirpstack ${D}${sysconfdir}/chirpstack-concentratord ${D}${sysconfdir}/chirpstack-mqtt-forwarder
    subst ${UNPACKDIR}/chirpstack.toml > ${D}${sysconfdir}/chirpstack/chirpstack.toml
    subst ${UNPACKDIR}/concentratord.toml > ${D}${sysconfdir}/chirpstack-concentratord/concentratord.toml
    subst ${UNPACKDIR}/chirpstack-mqtt-forwarder.toml > ${D}${sysconfdir}/chirpstack-mqtt-forwarder/chirpstack-mqtt-forwarder.toml
    chmod 0644 ${D}${sysconfdir}/chirpstack/*.toml ${D}${sysconfdir}/chirpstack-concentratord/*.toml \
        ${D}${sysconfdir}/chirpstack-mqtt-forwarder/*.toml
    # Region and channel plan straight from Concentratord's examples (installed by chirpstack-concentratord)
    ln -sf examples/region_${LORAWAN_REGION_ID}.toml ${D}${sysconfdir}/chirpstack-concentratord/region.toml
    ln -sf examples/channels_${LORAWAN_REGION_ID}.toml ${D}${sysconfdir}/chirpstack-concentratord/channels.toml

    install -d ${D}${libexecdir} ${D}${systemd_system_unitdir}
    install -m 0755 ${UNPACKDIR}/lorawan-secrets.sh ${D}${libexecdir}/lorawan-secrets
    install -m 0644 ${UNPACKDIR}/lorawan-secrets.service ${D}${systemd_system_unitdir}/

    install -d ${D}${sysconfdir}/systemd/network ${D}${sysconfdir}/ssh/sshd_config.d
    install -m 0644 ${UNPACKDIR}/80-wired.network ${UNPACKDIR}/80-wireless.network ${D}${sysconfdir}/systemd/network/
    install -m 0644 ${UNPACKDIR}/50-lorawan-sshd.conf ${D}${sysconfdir}/ssh/sshd_config.d/
    install -d ${D}${systemd_system_unitdir}/systemd-networkd-wait-online.service.d
    install -m 0644 ${UNPACKDIR}/10-lorawan-wait-online.conf ${D}${systemd_system_unitdir}/systemd-networkd-wait-online.service.d/

    # Wi-Fi: wpa_supplicant@wlan0 enabled, started only when its configuration exists
    if ${@bb.utils.contains('DISTRO_FEATURES', 'wifi', 'true', 'false', d)}; then
        install -d ${D}${systemd_system_unitdir}/wpa_supplicant@.service.d ${D}${sysconfdir}/systemd/system/multi-user.target.wants
        install -m 0644 ${UNPACKDIR}/10-lorawan-wpa-supplicant.conf ${D}${systemd_system_unitdir}/wpa_supplicant@.service.d/
        ln -sf ${systemd_system_unitdir}/wpa_supplicant@.service \
            ${D}${sysconfdir}/systemd/system/multi-user.target.wants/wpa_supplicant@wlan0.service
    fi

    install -d ${D}${systemd_unitdir}/system.conf.d
    install -m 0644 ${UNPACKDIR}/10-lorawan-watchdog.conf ${D}${systemd_unitdir}/system.conf.d/
}

SYSTEMD_SERVICE:${PN} = "lorawan-secrets.service"

FILES:${PN} += "${systemd_system_unitdir} ${systemd_unitdir}/system.conf.d"
CONFFILES:${PN} = " \
    ${sysconfdir}/chirpstack/chirpstack.toml \
    ${sysconfdir}/chirpstack-concentratord/concentratord.toml \
    ${sysconfdir}/chirpstack-mqtt-forwarder/chirpstack-mqtt-forwarder.toml \
"
RDEPENDS:${PN} = "chirpstack chirpstack-concentratord chirpstack-mqtt-forwarder"
RRECOMMENDS:${PN} = "${@bb.utils.contains('DISTRO_FEATURES', 'wifi', 'wpa-supplicant', '', d)}"
