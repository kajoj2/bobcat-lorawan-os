#!/bin/sh
# SPDX-License-Identifier: MIT
# Create the keys that sign update bundles (docs/updates.md), once per builder:
#
#   DIR/ca.cert.pem   built into the image as RAUC's keyring: the image accepts bundles signed under this CA
#   DIR/ca.key.pem    the CA's key: only needed to issue a new signing certificate; keep it offline
#   DIR/key.pem       signs bundles (private: keep it safe and out of any repository)
#   DIR/cert.pem      its certificate, issued by the CA, goes into each bundle
#
# A gateway only takes bundles signed under the CA it carries, so keep DIR: new keys mean a new image installed by
# hand (SD card) on every gateway.
#
#   tools/rauc-keys.sh DIR [NAME]
set -eu
dir=${1:?usage: tools/rauc-keys.sh DIR [NAME]}
name=${2:-LoRaWAN Gateway OS}
for f in ca.key.pem ca.cert.pem key.pem cert.pem; do
    if [ -e "$dir/$f" ]; then
        echo "$dir/$f exists; not overwriting" >&2
        exit 1
    fi
done
mkdir -p "$dir"
umask 077
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
ec="-newkey ec -pkeyopt ec_paramgen_curve:prime256v1 -nodes"

# shellcheck disable=SC2086
openssl req -x509 $ec -days 36500 -subj "/CN=$name update CA" \
    -addext "basicConstraints=critical,CA:true,pathlen:0" -addext "keyUsage=critical,keyCertSign,cRLSign" \
    -keyout "$dir/ca.key.pem" -out "$dir/ca.cert.pem" 2>/dev/null
# shellcheck disable=SC2086
openssl req $ec -subj "/CN=$name update signing" -keyout "$dir/key.pem" -out "$tmp/req.pem" 2>/dev/null
printf 'basicConstraints=critical,CA:false\nkeyUsage=critical,digitalSignature\nextendedKeyUsage=critical,codeSigning\n' \
    > "$tmp/ext.cnf"
openssl x509 -req -in "$tmp/req.pem" -CA "$dir/ca.cert.pem" -CAkey "$dir/ca.key.pem" -CAcreateserial \
    -days 36500 -extfile "$tmp/ext.cnf" -out "$dir/cert.pem" 2>/dev/null
rm -f "$dir/ca.cert.srl"
chmod 644 "$dir/ca.cert.pem" "$dir/cert.pem"
openssl verify -CAfile "$dir/ca.cert.pem" -purpose any "$dir/cert.pem" >/dev/null
echo "created in $dir: ca.cert.pem (keyring), cert.pem, key.pem and ca.key.pem (private, keep safe)"
openssl x509 -in "$dir/ca.cert.pem" -noout -subject -fingerprint -sha256
