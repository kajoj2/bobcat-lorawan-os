# SPDX-License-Identifier: MIT
"""Runtime tests for lorawan-gateway-image: the gateway stack minus the radio.

Run by testimage (kas/qemuarm64.yml). QEMU has no concentrator, so chirpstack-concentratord is expected to fail;
everything else must come up as on the board.
"""

import time

from oeqa.core.decorator.depends import OETestDepends
from oeqa.runtime.case import OERuntimeTestCase

# Arm64 under emulation is slow: the Rust daemons and ChirpStack's database migrations take a while on first boot.
STARTUP_TIMEOUT = 600

SERVICES = [
    "lorawan-secrets.service",
    "redis.service",
    "mosquitto.service",
    "chirpstack.service",
    "chirpstack-mqtt-forwarder.service",
    "systemd-networkd.service",
    "avahi-daemon.service",
]

# No SX1302 in QEMU.
EXPECTED_FAILURES = {"chirpstack-concentratord.service"}


class LorawanGatewayTest(OERuntimeTestCase):

    def wait_for(self, cmd, timeout=STARTUP_TIMEOUT, interval=10):
        """Run cmd until it succeeds; return (status, output) of the last run."""
        deadline = time.time() + timeout
        while True:
            status, output = self.target.run(cmd)
            if status == 0 or time.time() > deadline:
                return status, output
            time.sleep(interval)

    def journal(self, unit):
        return self.target.run("journalctl --no-pager -b -n 40 -u %s" % unit)[1]

    @OETestDepends(["ssh.SSHTest.test_ssh"])
    def test_boot_finished(self):
        # 'running' or 'degraded' (degraded: concentratord failed, checked below).
        status, output = self.target.run("systemctl is-system-running --wait", STARTUP_TIMEOUT)
        self.assertIn(output.strip(), ("running", "degraded"), msg="system state: %s" % output)

    @OETestDepends(["lorawan.LorawanGatewayTest.test_boot_finished"])
    def test_services_active(self):
        for unit in SERVICES:
            status, output = self.wait_for("systemctl is-active %s" % unit)
            self.assertEqual(status, 0, msg="%s is %s\n%s" % (unit, output, self.journal(unit)))

    @OETestDepends(["lorawan.LorawanGatewayTest.test_boot_finished"])
    def test_no_unexpected_failures(self):
        status, output = self.target.run("systemctl list-units --failed --plain --no-legend --no-pager")
        failed = {line.split()[0] for line in output.splitlines() if line.strip()}
        unexpected = failed - EXPECTED_FAILURES
        self.assertFalse(unexpected, msg="failed units: %s\n%s" % (
            ", ".join(sorted(unexpected)), "\n".join(self.journal(u) for u in sorted(unexpected))))

    @OETestDepends(["lorawan.LorawanGatewayTest.test_services_active"])
    def test_secret_generated(self):
        status, output = self.target.run("stat -c '%a %s' /var/lib/lorawan-secrets/chirpstack.env")
        self.assertEqual(status, 0, msg=output)
        mode, size = output.split()
        self.assertEqual(mode, "600")
        self.assertGreater(int(size), len("CHIRPSTACK_API_SECRET=") + 40)
        status, output = self.target.run("grep -c '^CHIRPSTACK_API_SECRET=.\\{40,\\}$' /var/lib/lorawan-secrets/chirpstack.env")
        self.assertEqual(output.strip(), "1")

    @OETestDepends(["lorawan.LorawanGatewayTest.test_services_active"])
    def test_chirpstack_database(self):
        # Created by ChirpStack's migrations on its first start.
        status, output = self.wait_for("test -s /var/lib/chirpstack/chirpstack.sqlite")
        self.assertEqual(status, 0, msg=self.journal("chirpstack"))

    @OETestDepends(["lorawan.LorawanGatewayTest.test_services_active"])
    def test_chirpstack_web(self):
        status, output = self.wait_for("wget -q -O - http://127.0.0.1:8080/")
        self.assertEqual(status, 0, msg="no answer on :8080\n%s" % self.journal("chirpstack"))
        self.assertIn("<html", output.lower(), msg=output[:500])

    @OETestDepends(["lorawan.LorawanGatewayTest.test_services_active"])
    def test_chirpstack_on_mqtt(self):
        # ChirpStack subscribes to the gateway topics of its region; Mosquitto logs each client that connects.
        status, output = self.wait_for("journalctl --no-pager -b -u mosquitto | grep -q 'New client connected'")
        self.assertEqual(status, 0, msg="no client on the broker\n%s\n%s" % (
            self.journal("mosquitto"), self.journal("chirpstack")))

    @OETestDepends(["lorawan.LorawanGatewayTest.test_services_active"])
    def test_mqtt_roundtrip(self):
        cmd = ("timeout 30 mosquitto_sub -h 127.0.0.1 -t lorawan-test/ping -C 1 & sleep 3; "
               "mosquitto_pub -h 127.0.0.1 -t lorawan-test/ping -m pong; wait")
        status, output = self.target.run(cmd)
        self.assertIn("pong", output)

    @OETestDepends(["lorawan.LorawanGatewayTest.test_boot_finished"])
    def test_brokers_local_only(self):
        # Mosquitto (1883) and Redis (6379) must not listen on other addresses than loopback.
        # busybox netstat or iproute2 ss; the local address is the fourth column in both.
        status, output = self.target.run("netstat -ltn 2>/dev/null || ss -Hltn")
        self.assertEqual(status, 0, msg=output)
        for line in output.splitlines():
            fields = line.split()
            if len(fields) < 4 or not fields[3].endswith((":1883", ":6379")):
                continue
            self.assertRegex(fields[3], r"^(127\.0\.0\.1|\[?::1\]?):", msg="listens beyond localhost: %s" % line)

    @OETestDepends(["lorawan.LorawanGatewayTest.test_boot_finished"])
    def test_sshd_board_policy(self):
        # The board's key-only drop-in is installed; on QEMU lorawan-test-access overrides it (and is QEMU only).
        status, output = self.target.run("cat /etc/ssh/sshd_config.d/50-lorawan-sshd.conf")
        self.assertEqual(status, 0, msg=output)
        self.assertIn("PasswordAuthentication no", output)
        self.assertIn("PermitEmptyPasswords no", output)

    @OETestDepends(["lorawan.LorawanGatewayTest.test_services_active"])
    def test_concentratord_configured(self):
        # Fails at runtime without hardware, but its configuration must be complete and point at the region files.
        for path in ("concentratord.toml", "region.toml", "channels.toml"):
            status, output = self.target.run("test -s /etc/chirpstack-concentratord/%s" % path)
            self.assertEqual(status, 0, msg="missing or empty: %s" % path)

    @OETestDepends(["lorawan.LorawanGatewayTest.test_boot_finished"])
    def test_watchdog_configured(self):
        # QEMU has no watchdog device; check that systemd took the setting (on the board it also pets /dev/watchdog0).
        status, output = self.target.run("systemctl show -p RuntimeWatchdogUSec --value")
        self.assertEqual(output.strip(), "30s", msg=output)

    @OETestDepends(["lorawan.LorawanGatewayTest.test_boot_finished"])
    def test_logs_in_ram(self):
        # SD card wear: /var/log is a tmpfs (oe-core's VOLATILE_LOG_DIR), so journald writes no persistent journal.
        status, output = self.target.run(
            "readlink -f /var/log | grep -q '^/var/volatile/' && grep -q ' /var/volatile tmpfs ' /proc/mounts")
        self.assertEqual(status, 0, msg=self.target.run("ls -ld /var/log; grep volatile /proc/mounts")[1])
