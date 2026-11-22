<!-- SPDX-License-Identifier: MIT -->
# Home Assistant over MQTT

ChirpStack publishes everything about your devices on the gateway's own MQTT broker (Mosquitto, localhost only):
uplinks with the values your codec decoded, joins, acknowledgements, errors. The panel's **Home Assistant (MQTT)**
card bridges these messages to another broker, usually the Mosquitto add-on of Home Assistant. The gateway connects
out to that broker; its own broker stays closed to the network, and nothing has to be opened on the gateway.

```
ChirpStack ──> Mosquitto on the gateway ──bridge (out)──> Home Assistant's broker ──> MQTT integration
                (localhost only)        <─downlinks (in, optional)─
```

## Set up

1. In Home Assistant: install the **Mosquitto broker** add-on (Settings → Add-ons) and the **MQTT** integration,
   if you do not use them yet.
2. A user for the gateway on that broker: a Home Assistant user (Settings → People → Users, e.g. `lorawan`; the
   add-on accepts Home Assistant users), or an entry under the add-on's *Logins* option.
3. On the gateway's panel (`http://<gateway>/`), card **Home Assistant (MQTT)** → *Connect*:
   - **Broker**: Home Assistant's address, e.g. `homeassistant.local` or its IP; port 1883 unless you give one
     (`host:port`), 8883 with **TLS**.
   - **User name** and **Password** of step 2.
   - **Topic prefix**: put in front of every topic on Home Assistant's broker; default `chirpstack/`, so
     `application/...` arrives as `chirpstack/application/...`. Empty: the topics as they are.
   - **Accept downlinks from that broker**: see "Downlinks" below.
4. *Save* asks for a ChirpStack login (the account of the web interface on port 8080) and restarts the gateway's
   broker (a few seconds; ChirpStack and the forwarders reconnect by themselves). The card then shows the state:
   **connected**, or **not connected (retrying)** with the reason in `journalctl -u mosquitto` on the gateway.

The password is stored on the gateway (`/var/lib/lorawan-mqtt-bridge/home-assistant.conf`, readable by root and
Mosquitto only) and never shown again; leave the field empty when changing other settings to keep it.

TLS checks the broker's certificate against the public CAs in the image (`/etc/ssl/certs`). Home Assistant's add-on
with a self-signed certificate does not pass that check; on your LAN, plain MQTT on port 1883 is the usual choice.

## Topics

With the default prefix, on Home Assistant's broker:

| Topic | What |
|---|---|
| `chirpstack/application/<application ID>/device/<DevEUI>/event/up` | Uplink: `object` holds the decoded values, plus RSSI/SNR (`rxInfo`), frame counter, data rate |
| `chirpstack/application/<application ID>/device/<DevEUI>/event/join` | The device joined |
| `.../event/status`, `.../event/ack`, `.../event/txack`, `.../event/log` | Battery/margin, acknowledgements, errors |
| `chirpstack/application/<application ID>/device/<DevEUI>/command/down` | Downlink to the device (only with downlinks accepted) |

The application ID is the UUID in ChirpStack's address bar (Applications → your application); the DevEUI is in
lower case. Payloads are JSON (ChirpStack's `json` marshaler); see ChirpStack's documentation of the MQTT integration
for every field.

## Sensors in Home Assistant

An example for a device whose codec returns `{"temperature": 21.5, "battery": 3.01}`, in `configuration.yaml`:

```yaml
mqtt:
  sensor:
    - name: "Garden temperature"
      unique_id: lorawan_0011223344556677_temperature
      state_topic: "chirpstack/application/APPLICATION-ID/device/0011223344556677/event/up"
      value_template: "{{ value_json.object.temperature }}"
      unit_of_measurement: "°C"
      device_class: temperature
      state_class: measurement
    - name: "Garden sensor RSSI"
      unique_id: lorawan_0011223344556677_rssi
      state_topic: "chirpstack/application/APPLICATION-ID/device/0011223344556677/event/up"
      value_template: "{{ value_json.rxInfo[0].rssi }}"
      unit_of_measurement: "dBm"
      device_class: signal_strength
      entity_category: diagnostic
```

Entities created automatically (MQTT discovery) are not part of this yet.

## Downlinks

With **Accept downlinks from that broker** ticked, a message on
`chirpstack/application/<application ID>/device/<DevEUI>/command/down` reaches ChirpStack's downlink queue, e.g.
from a Home Assistant script:

```yaml
action: mqtt.publish
data:
  topic: "chirpstack/application/APPLICATION-ID/device/0011223344556677/command/down"
  payload: '{"devEui": "0011223344556677", "confirmed": false, "fPort": 10, "data": "SGVsbG8="}'
```

`data` is base64. A class A device receives it after its next uplink. Anyone who can publish to that topic on Home
Assistant's broker can send downlinks to your devices, so leave it off unless you need it.

## How it works

- `lorawan-gateway-config` starts Mosquitto with `/etc/mosquitto/lorawan.conf` instead of `mosquitto.conf`: no
  listener (Mosquitto's default, localhost:1883 only) and `include_dir /var/lib/lorawan-mqtt-bridge`. tmpfiles.d
  creates that directory as `2750 root:mosquitto`, so what the panel writes there is readable by Mosquitto only.
- The panel (`lorawan-agent`, docs/lorawan-agent.md) writes one bridge section there (`connection home-assistant`,
  client ID `lorawan-<host name>`, MQTT 3.1.1, clean session) with `topic application/# out` and, with downlinks,
  `topic application/+/device/+/command/down in`, then restarts Mosquitto. *Turn off* deletes the file.
- The bridge's state comes from Mosquitto's own notifications on the local topic `lorawan-agent/bridge/state`
  (`1` connected, `0` not), which the panel follows.
- API: `GET /api/bridge` (settings without the password, state; no login), `GET /api/bridge/login` and
  `POST /api/bridge/save` (form fields `server`, `username`, `password`, `prefix`, `tls=on`, `downlinks=on`, or
  `action=off`) behind the ChirpStack login.
