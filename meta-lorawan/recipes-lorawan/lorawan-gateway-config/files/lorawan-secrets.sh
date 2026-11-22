#!/bin/sh
# SPDX-License-Identifier: MIT
# Generate this gateway's secrets on its first boot, so no two gateways (and no image) share them.
set -eu
dir=/var/lib/lorawan-secrets
umask 077
mkdir -p "$dir"
if [ ! -s "$dir/chirpstack.env" ]; then
	# Two random UUIDs from the kernel, as 64 hex characters (244 random bits). Needs nothing beyond
	# cat and tr, also on a minimal image.
	secret=$(cat /proc/sys/kernel/random/uuid /proc/sys/kernel/random/uuid | tr -d '\n-')
	if [ "${#secret}" -ne 64 ]; then
		echo "lorawan-secrets: could not generate a secret (got ${#secret} characters)" >&2
		exit 1
	fi
	printf 'CHIRPSTACK_API_SECRET=%s\n' "$secret" > "$dir/chirpstack.env.tmp"
	mv "$dir/chirpstack.env.tmp" "$dir/chirpstack.env"
	echo "lorawan-secrets: generated the ChirpStack API secret"
fi
# ChirpStack reads its file as the chirpstack user (EnvironmentFile is read by systemd as root, but keep it tidy).
chmod 0600 "$dir/chirpstack.env"
