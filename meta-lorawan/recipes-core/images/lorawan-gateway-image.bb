# SPDX-License-Identifier: MIT
SUMMARY = "LoRaWAN gateway: concentrator daemon, MQTT forwarder and ChirpStack network server on one board"
LICENSE = "MIT"

inherit core-image extrausers

IMAGE_FEATURES += "ssh-server-openssh"

# Access: the root account has an empty password, which only the local consoles (serial, USB gadget) accept;
# sshd takes keys only (lorawan-gateway-config). Put your public key(s) into LORAWAN_SSH_AUTHORIZED_KEYS (one
# string, keys separated by newlines) in local.conf or a kas overlay to log in over the network.
# nooelint: oelint.var.badimagefeature.empty-root-password
IMAGE_FEATURES += "empty-root-password"
LORAWAN_SSH_AUTHORIZED_KEYS ??= ""

IMAGE_INSTALL += " \
    packagegroup-core-boot \
    lorawan-gateway-config \
    lorawan-system \
    lorawan-agent \
    chirpstack \
    chirpstack-concentratord \
    chirpstack-mqtt-forwarder \
    chirpstack-udp-forwarder \
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
# Setup wizard's access point and Wi-Fi join (docs/setup.md), on boards with Wi-Fi
IMAGE_INSTALL += "${@bb.utils.contains('DISTRO_FEATURES', 'wifi', 'lorawan-setup', '', d)}"

# Bring-up and diagnostic tools, off by default: LORAWAN_DEBUG_TOOLS = "1" (or kas/debug-tools.yml)
LORAWAN_DEBUG_TOOLS ??= "0"
IMAGE_INSTALL += "${@oe.utils.conditional('LORAWAN_DEBUG_TOOLS', '1', 'packagegroup-lorawan-debug', '', d)}"

# zstd (here) and pigz/xz (packagegroup-lorawan-base-utils): multi-threaded compression for disk images, e.g.
# backing up the factory eMMC over the network; a single-threaded gzip managed ~8 MB/s on the RK3566.

# The distro has no "pam", but systemd's logind still recommends a PAM module, which brings libpam along unused
BAD_RECOMMENDATIONS += "pam-plugin-umask"

# bash for root's logins (bash-completion is in packagegroup-lorawan-base-utils)
EXTRA_USERS_PARAMS = "usermod -s /bin/bash root;"

# No access times on the root file system: fewer writes to the SD card or eMMC (/var/lib has its own mount)
fstab_noatime() {
    f=${IMAGE_ROOTFS}${sysconfdir}/fstab
    awk '$1 == "/dev/root" && $2 == "/" && $4 !~ /noatime/ { $4 = $4 ",noatime" } { print }' "$f" > "$f.new"
    mv "$f.new" "$f"
    grep -q '^/dev/root.*noatime' "$f" || bbfatal "fstab: root entry not found"
}
ROOTFS_POSTPROCESS_COMMAND += "fstab_noatime"

# Redis and Mosquitto keep their packaged defaults: both listen on localhost only.

install_authorized_keys() {
    if [ -n "${LORAWAN_SSH_AUTHORIZED_KEYS}" ]; then
        install -d -m 0700 ${IMAGE_ROOTFS}/root/.ssh
        printf '%s\n' "${LORAWAN_SSH_AUTHORIZED_KEYS}" > ${IMAGE_ROOTFS}/root/.ssh/authorized_keys
        chmod 0600 ${IMAGE_ROOTFS}/root/.ssh/authorized_keys
    fi
}
ROOTFS_POSTPROCESS_COMMAND += "install_authorized_keys"
