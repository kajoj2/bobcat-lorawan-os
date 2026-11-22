<!-- SPDX-License-Identifier: MIT -->
# BLE sensors to MQTT

The gateway's Bluetooth (the BCM43430's) can listen for cheap Bluetooth LE sensors next to LoRaWAN and publish their
readings the same way as the LoRaWAN devices' data: on the gateway's MQTT broker, and through the Home Assistant
card to Home Assistant, which creates the sensors by itself.

## Switch it on

Panel, card "BLE sensors": tick "Listen for BLE sensors" (asks for a ChirpStack login). The setting stays across
restarts and updates (`/var/lib/lorawan-ble/enabled`). The card lists every sensor heard, with its readings, RSSI and
when it was last heard. Scanning is passive: the gateway never transmits or connects to anything.

## Sensors it understands

| Format | Typical devices | What is read |
|---|---|---|
| [BTHome](https://bthome.io/) v2, unencrypted | Shelly BLU, Xiaomi thermometers with the pvvx firmware set to BTHome, ESPHome and Zephyr sensors | temperature, humidity, battery, voltage, pressure, illuminance, CO2, moisture, PM, power, energy, current, distance, UV, counts |
| pvvx custom (service data 0x181A, 15 bytes) | LYWSD03MMC, MHO-C401, CGG1 with [pvvx's firmware](https://github.com/pvvx/ATC_MiThermometer) | temperature, humidity, battery, voltage |
| ATC1441 (0x181A, 13 bytes) | the same thermometers with the ATC firmware | temperature, humidity, battery, voltage |
| Xiaomi MiBeacon, unencrypted | older Xiaomi sensors (LYWSDCGQ and similar) | temperature, humidity, battery |
| RuuviTag data format 5 | RuuviTag | temperature, humidity, pressure, voltage |

Encrypted advertisements (BTHome with a key, MiBeacon of newer Xiaomi devices on the stock firmware) are skipped:
flash the pvvx firmware, or use a device that sends in the open. Everything else that advertises (phones,
headphones, watches) is ignored and never published.

## Topics

`lorawan/ble/<MAC without colons, lower case>`, at most every 10 s per sensor when a value changes and at least once a
minute while it is heard, not retained:

```json
{"mac":"A4:C1:38:01:02:03","type":"pvvx","name":"ATC_010203","rssi":-71,"seen":1795392000,
 "temperature":21.53,"humidity":45.12,"voltage":2.958,"battery":87}
```

With the Home Assistant bridge on, the topic arrives there with the bridge's prefix (`chirpstack/lorawan/ble/...` by
default), and a retained discovery entry per value (`homeassistant/sensor/ble_<mac>/<value>/config`, without the
prefix) makes Home Assistant create a device with one sensor per value.

## How it works

`lorawan-ble.service` runs `lorawan-agent ble`. It opens a raw HCI socket on `hci0` (the kernel's Bluetooth socket
interface, no BlueZ D-Bus), brings the controller up if needed, starts a passive LE scan (100 ms interval, 50 ms
window) and decodes the advertising reports. It runs as a dynamic user with only `CAP_NET_RAW` and
`CAP_NET_ADMIN`. The panel reads what it heard from `/run/lorawan-ble/sensors.json`. bluetoothd may keep running.
