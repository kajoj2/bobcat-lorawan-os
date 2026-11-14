#!/bin/sh
# SPDX-License-Identifier: MIT
# Generate this gateway's secrets on its first boot, so no two gateways (and no image) share them.
set -eu
dir=/var/lib/lorawan-secrets
umask 077
mkdir -p "$dir"
if [ ! -s "$dir/chirpstack.env" ]; then
	secret=$(head -c 32 /dev/urandom | base64 | tr -d '\n')
	printf 'CHIRPSTACK_API_SECRET=%s\n' "$secret" > "$dir/chirpstack.env.tmp"
	mv "$dir/chirpstack.env.tmp" "$dir/chirpstack.env"
	echo "lorawan-secrets: generated the ChirpStack API secret"
fi
# ChirpStack reads its file as the chirpstack user (EnvironmentFile is read by systemd as root, but keep it tidy).
chmod 0600 "$dir/chirpstack.env"
