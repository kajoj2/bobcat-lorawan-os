# SPDX-License-Identifier: MIT
# The G285's A/B slots (system.conf) and the keyring that update bundles are checked against (docs/updates.md).
# The keyring is the update CA certificate from BOBCAT_RAUC_KEY_DIR (conf/machine/bobcat-g285.conf), made once per
# builder with tools/rauc-keys.sh; it is never part of this repository.

FILESEXTRAPATHS:prepend:bobcat := "${THISDIR}/files:"
# nooelint: oelint.vars.fileextrapathsop
FILESEXTRAPATHS:prepend:bobcat = "${BOBCAT_RAUC_KEY_DIR}:"

RAUC_KEYRING_FILE:bobcat = "ca.cert.pem"

do_install:append:bobcat() {
    sed -i -e 's|@MACHINE@|${MACHINE}|g' ${D}${nonarch_libdir}/rauc/system.conf
}

# nooelint: oelint.vars.noncoreoverride
python () {
    if 'bobcat' not in (d.getVar('MACHINEOVERRIDES') or '').split(':'):
        return
    cert = os.path.join(d.getVar('BOBCAT_RAUC_KEY_DIR'), 'ca.cert.pem')
    if not os.path.exists(cert):
        bb.fatal("No update CA certificate at %s. Create a key pair once with "
                 "`tools/rauc-keys.sh %s` (docs/updates.md), or set BOBCAT_RAUC_KEY_DIR to yours."
                 % (cert, d.getVar('BOBCAT_RAUC_KEY_DIR')))
}
