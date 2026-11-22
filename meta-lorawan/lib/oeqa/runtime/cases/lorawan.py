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
        # 32 random bytes in hex, one line.
        self.assertEqual(int(size), len("CHIRPSTACK_API_SECRET=") + 64 + 1)
        status, output = self.target.run("grep -c '^CHIRPSTACK_API_SECRET=[0-9a-f]\\{64\\}$' /var/lib/lorawan-secrets/chirpstack.env")
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
        cmd = ("mosquitto_sub -h 127.0.0.1 -t lorawan-test/ping -C 1 -W 30 & sleep 3; "
               "mosquitto_pub -h 127.0.0.1 -t lorawan-test/ping -m pong; wait")
        status, output = self.target.run(cmd)
        self.assertIn("pong", output)

    @OETestDepends(["lorawan.LorawanGatewayTest.test_boot_finished"])
    def test_brokers_local_only(self):
        # Mosquitto (1883) and Redis (6379) must not listen on other addresses than loopback.
        # No netstat/ss in the image: read the kernel's socket tables. State 0A is LISTEN; addresses are hex.
        status, output = self.target.run("cat /proc/net/tcp /proc/net/tcp6")
        self.assertEqual(status, 0, msg=output)
        loopback = {"0100007F", "00000000000000000000000001000000"}
        ports = {0x075B: "Mosquitto (1883)", 0x18EB: "Redis (6379)"}
        seen = set()
        for line in output.splitlines():
            fields = line.split()
            if len(fields) < 4 or fields[3] != "0A" or ":" not in fields[1]:
                continue
            addr, port = fields[1].split(":")
            port = int(port, 16)
            if port in ports:
                seen.add(port)
                self.assertIn(addr, loopback, msg="%s listens beyond localhost (%s)" % (ports[port], addr))
        self.assertEqual(seen, set(ports), msg="not listening: %s" % [ports[p] for p in set(ports) - seen])

    @OETestDepends(["lorawan.LorawanGatewayTest.test_services_active"])
    def test_mqtt_bridge_setup(self):
        # Mosquitto reads our file with the panel's bridge directory; the directory is Mosquitto's group only.
        status, output = self.target.run("cat /proc/$(systemctl show -p MainPID --value mosquitto)/cmdline | tr '\\0' ' '")
        self.assertIn("/etc/mosquitto/lorawan.conf", output)
        status, output = self.target.run("stat -c '%a %U %G' /var/lib/lorawan-mqtt-bridge")
        self.assertEqual(output.strip(), "2750 root mosquitto", msg=output)
        # No bridge until one is set on the panel
        status, output = self.target.run("wget -qO- http://127.0.0.1/api/bridge")
        self.assertEqual(status, 0, msg=output)
        self.assertIn('"configured":false', output)

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

    @OETestDepends(["lorawan.LorawanGatewayTest.test_boot_finished"])
    def test_udp_forwarder_off_by_default(self):
        # Installed and enabled, but skipped until the panel writes a server list (docs/forwarding.md).
        status, output = self.target.run("chirpstack-udp-forwarder --version")
        self.assertEqual(status, 0, msg=output)
        status, output = self.target.run("test -e /var/lib/lorawan-forward/servers.toml")
        self.assertNotEqual(status, 0, msg="a server list exists on a fresh image")
        status, output = self.target.run(
            "systemctl show -p UnitFileState -p ConditionResult -p ActiveState chirpstack-udp-forwarder")
        props = dict(l.split("=", 1) for l in output.splitlines() if "=" in l)
        self.assertEqual(props.get("UnitFileState"), "enabled", msg=output)
        self.assertEqual(props.get("ConditionResult"), "no", msg=output)
        self.assertEqual(props.get("ActiveState"), "inactive", msg=output)

    @OETestDepends(["lorawan.LorawanGatewayTest.test_chirpstack_web"])
    def test_forward_panel(self):
        status, output = self.wait_for("wget -q -O - http://127.0.0.1/api/forward")
        self.assertEqual(status, 0, msg=self.journal("lorawan-agent"))
        self.assertIn('"servers":[]', output)
        self.assertIn("eu1.cloud.thethings.network:1700", output)
        # Fresh image: ChirpStack still has admin/admin, which the card reports.
        self.assertIn('"default_password":true', output)
        # Turn forwarding on (no login yet): the list is written and the unit started. Without a concentrator in
        # QEMU the forwarder cannot get the gateway ID and keeps restarting; only the file and the start are checked.
        status, output = self.target.run(
            "wget -q -O - --post-data 'server=127.0.0.1%3A1700&server=bad host' http://127.0.0.1/api/forward 2>&1")
        self.assertNotEqual(status, 0, msg="an invalid server was accepted: %s" % output)
        status, output = self.target.run(
            "wget -q -O - --post-data 'server=127.0.0.1%3A1700' http://127.0.0.1/api/forward")
        self.assertEqual(status, 0, msg="%s\n%s" % (output, self.journal("lorawan-agent")))
        status, output = self.target.run("cat /var/lib/lorawan-forward/servers.toml")
        self.assertIn('server = "127.0.0.1:1700"', output)
        status, output = self.target.run("systemctl show -p ConditionResult --value chirpstack-udp-forwarder")
        self.assertEqual(output.strip(), "yes")
        # And off again: file gone, unit stopped.
        status, output = self.target.run("wget -q -O - --post-data '' http://127.0.0.1/api/forward")
        self.assertEqual(status, 0, msg=output)
        status, output = self.target.run("test -e /var/lib/lorawan-forward/servers.toml")
        self.assertNotEqual(status, 0, msg="the list is still there after turning forwarding off")
        status, output = self.target.run("systemctl is-active chirpstack-udp-forwarder")
        self.assertEqual(output.strip(), "inactive", msg=output)

    @OETestDepends(["lorawan.LorawanGatewayTest.test_chirpstack_web"])
    def test_setup_wizard(self):
        status, output = self.wait_for("wget -q -O - http://127.0.0.1/setup")
        self.assertEqual(status, 0, msg=self.journal("lorawan-agent"))
        self.assertIn("Gateway setup", output)
        status, output = self.target.run("wget -q -O - http://127.0.0.1/api/setup")
        self.assertEqual(status, 0, msg=output)
        # Fresh image: not set up yet, no setup access point (QEMU has no wlan0)
        self.assertIn('"done":false', output)
        self.assertIn('"via_ap":false', output)
        self.assertIn('"active":false', output)
        # An invalid request is refused before anything is written
        status, output = self.target.run(
            "wget -q -O - --post-data 'ssid=&psk=x' http://127.0.0.1/api/setup/wifi 2>&1")
        self.assertNotEqual(status, 0, msg="an empty network name was accepted: %s" % output)
        # Finished: remembered, and from the LAN the wizard's changes now need the ChirpStack login
        status, output = self.target.run("wget -q -O - --post-data '' http://127.0.0.1/api/setup/done")
        self.assertEqual(status, 0, msg=output)
        status, output = self.target.run("wget -q -O - http://127.0.0.1/api/setup")
        self.assertIn('"done":true', output)
        status, output = self.target.run(
            "wget -S -q -O - --post-data 'ssid=x&psk=12345678' http://127.0.0.1/api/setup/wifi 2>&1")
        self.assertIn("401", output)
        self.target.run("rm -f /var/lib/lorawan-setup/done")

    @OETestDepends(["lorawan.LorawanGatewayTest.test_boot_finished"])
    def test_gnu_tools(self):
        # GNU tools instead of busybox (packagegroup-lorawan-base-utils), bash for root
        status, output = self.target.run("test ! -e /bin/busybox && readlink -f /bin/sh && grep ^root: /etc/passwd")
        self.assertEqual(status, 0, msg="busybox is installed\n%s" % output)
        self.assertIn("/bin/bash", output)
        for cmd in ("head -c 4 /dev/zero | base64", "timeout 5 true", "printenv PATH", "install -d -m 700 /tmp/t && find /tmp/t -delete"):
            status, output = self.target.run(cmd)
            self.assertEqual(status, 0, msg="%s: %s" % (cmd, output))

    @OETestDepends(["lorawan.LorawanGatewayTest.test_boot_finished"])
    def test_firewall(self):
        # The rule set is loaded and refuses ports nothing should offer; SSH (this test's own connection) works.
        status, output = self.target.run("systemctl is-active lorawan-firewall && nft list table inet lorawan")
        self.assertEqual(status, 0, msg="%s\n%s" % (output, self.journal("lorawan-firewall")))
        self.assertIn("policy drop", output)
        self.assertIn("tcp dport { 22, 80, 8080 } accept", output)
        # Extra rules from the data partition are read
        status, output = self.target.run(
            "echo 'tcp dport 9999 accept' > /var/lib/lorawan-firewall/test.nft && systemctl reload lorawan-firewall "
            "&& nft list chain inet lorawan input; rm -f /var/lib/lorawan-firewall/test.nft; "
            "systemctl reload lorawan-firewall")
        self.assertIn("tcp dport 9999 accept", output)

    @OETestDepends(["lorawan.LorawanGatewayTest.test_boot_finished"])
    def test_root_noatime(self):
        status, output = self.target.run("findmnt -no OPTIONS /")
        self.assertIn("noatime", output)

    @OETestDepends(["lorawan.LorawanGatewayTest.test_boot_finished"])
    def test_zram_swap(self):
        # The QEMU kernel may come without zram; then the unit does nothing and stays successful.
        status, output = self.target.run("systemctl is-active lorawan-zram")
        self.assertEqual(status, 0, msg="%s\n%s" % (output, self.journal("lorawan-zram")))
        status, output = self.target.run("test -e /sys/class/zram-control && swapon --show=NAME --noheadings")
        if status == 0:
            self.assertIn("/dev/zram", output)

    @OETestDepends(["lorawan.LorawanGatewayTest.test_services_active"])
    def test_metrics_and_devices(self):
        status, output = self.wait_for("wget -q -O - http://127.0.0.1/metrics")
        self.assertEqual(status, 0, msg=output)
        self.assertIn("# TYPE lorawan_rx_frames_total counter", output)
        self.assertIn('lorawan_service_up{unit="chirpstack"} 1', output)
        status, output = self.target.run("wget -q -O - http://127.0.0.1/api/devices")
        self.assertEqual(status, 0, msg=output)
        self.assertIn('"threshold_min":60', output)
        # ChirpStack's own metrics through the panel (monitoring endpoint on localhost)
        status, output = self.wait_for("wget -q -O - http://127.0.0.1/metrics/chirpstack", timeout=120)
        self.assertEqual(status, 0, msg=output)

    @OETestDepends(["lorawan.LorawanGatewayTest.test_services_active"])
    def test_backup_round_trip(self):
        status, output = self.target.run(
            "/usr/libexec/lorawan-backup create /tmp/b.tar.gz && /usr/libexec/lorawan-backup check /tmp/b.tar.gz "
            "&& tar -tzf /tmp/b.tar.gz")
        self.assertEqual(status, 0, msg=output)
        self.assertIn("chirpstack/chirpstack.sqlite", output)
        self.assertIn("lorawan-secrets/", output)
        # A file with anything else in it is refused
        status, output = self.target.run(
            "mkdir -p /tmp/x/etc && echo format=lorawan-backup 1 > /tmp/x/manifest.txt && touch /tmp/x/etc/passwd "
            "&& tar -czf /tmp/bad.tar.gz -C /tmp/x manifest.txt etc && /usr/libexec/lorawan-backup check /tmp/bad.tar.gz")
        self.assertNotEqual(status, 0, msg=output)
        self.target.run("rm -rf /tmp/b.tar.gz /tmp/bad.tar.gz /tmp/x")

    @OETestDepends(["lorawan.LorawanGatewayTest.test_services_active"])
    def test_device_profiles_imported(self):
        status, output = self.wait_for(
            "cmp /usr/share/chirpstack-device-profiles/REVISION /var/lib/chirpstack/device-profiles.revision")
        self.assertEqual(status, 0, msg=self.journal("chirpstack-device-profiles-import"))

    @OETestDepends(["lorawan.LorawanGatewayTest.test_services_active"])
    def test_wireguard_card(self):
        status, output = self.target.run("wget -q -O - http://127.0.0.1/api/wireguard")
        self.assertIn('"available":true', output)
        self.assertIn('"configured":false', output)
