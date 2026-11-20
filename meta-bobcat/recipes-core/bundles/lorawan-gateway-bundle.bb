# SPDX-License-Identifier: MIT
SUMMARY = "Update bundle of the gateway image (RAUC)"
DESCRIPTION = "lorawan-gateway-image's root file system as a signed RAUC bundle, for the panel's System update \
card or `rauc install` (docs/updates.md). Built for machines with 'rauc' in DISTRO_FEATURES."
LICENSE = "MIT"

inherit bundle

python () {
    if not bb.utils.contains('DISTRO_FEATURES', 'rauc', True, False, d):
        raise bb.parse.SkipRecipe("needs 'rauc' in DISTRO_FEATURES")
}

RAUC_BUNDLE_FORMAT = "verity"
# Must match the compatible= of the machine's system.conf
RAUC_BUNDLE_COMPATIBLE = "${MACHINE}"
RAUC_BUNDLE_VERSION = "${DISTRO_VERSION}-${DATETIME}"
RAUC_BUNDLE_VERSION[vardepsexclude] = "DATETIME"
RAUC_BUNDLE_DESCRIPTION = "${DISTRO_NAME} for ${MACHINE}"

RAUC_BUNDLE_SLOTS = "rootfs"
RAUC_SLOT_rootfs = "lorawan-gateway-image"
RAUC_SLOT_rootfs[fstype] = "ext4"

# Signing key of this builder (tools/rauc-keys.sh); the image's keyring is its certificate.
RAUC_KEY_FILE = "${BOBCAT_RAUC_KEY_DIR}/key.pem"
RAUC_CERT_FILE = "${BOBCAT_RAUC_KEY_DIR}/cert.pem"
