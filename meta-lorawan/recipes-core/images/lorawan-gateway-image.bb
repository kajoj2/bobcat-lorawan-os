# SPDX-License-Identifier: MIT
SUMMARY = "LoRaWAN gateway: concentrator daemon, MQTT forwarder and ChirpStack network server on one board"
LICENSE = "MIT"

inherit core-image

IMAGE_FEATURES += "ssh-server-openssh"

# Access: the root account has an empty password, which only the local consoles (serial, USB gadget) accept;
# sshd takes keys only (lorawan-gateway-config). Put your public key(s) into LORAWAN_SSH_AUTHORIZED_KEYS (one
# string, keys separated by newlines) in local.conf or a kas overlay to log in over the network.
IMAGE_FEATURES += "empty-root-password"
LORAWAN_SSH_AUTHORIZED_KEYS ??= ""

IMAGE_INSTALL += " \
    packagegroup-core-boot \
    lorawan-gateway-config \
    lorawan-agent \
    chirpstack \
    chirpstack-concentratord \
    chirpstack-mqtt-forwarder \
    libloragw-sx1302-utils \
    mosquitto \
    mosquitto-clients \
    redis \
    avahi-daemon \
    libgpiod-tools \
    i2c-tools \
    ethtool \
    htop \
    nano \
    zstd \
"
# Bring-up and diagnostic tools, off by default: LORAWAN_DEBUG_TOOLS = "1" (or kas/debug-tools.yml)
LORAWAN_DEBUG_TOOLS ??= "0"
IMAGE_INSTALL += "${@oe.utils.conditional('LORAWAN_DEBUG_TOOLS', '1', 'packagegroup-lorawan-debug', '', d)}"

# zstd: multi-threaded compression (zstd -T0) for disk images, e.g. backing up the factory eMMC over the network:
# busybox gzip uses one core and limits such a backup to ~8 MB/s on the RK3566.

# Redis and Mosquitto keep their packaged defaults: both listen on localhost only.

install_authorized_keys() {
    if [ -n "${LORAWAN_SSH_AUTHORIZED_KEYS}" ]; then
        install -d -m 0700 ${IMAGE_ROOTFS}/root/.ssh
        printf '%s\n' "${LORAWAN_SSH_AUTHORIZED_KEYS}" > ${IMAGE_ROOTFS}/root/.ssh/authorized_keys
        chmod 0600 ${IMAGE_ROOTFS}/root/.ssh/authorized_keys
    fi
}
ROOTFS_POSTPROCESS_COMMAND += "install_authorized_keys"
