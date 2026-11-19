<!-- SPDX-License-Identifier: MIT -->
# Experimental: boot firmware

**Status: experimental, on the branch `experimental/boot-firmware` only.** main keeps the boot chain that is known
to work (generic U-Boot, Rockchip's BL31 blob, DDR at 1056 MHz).

First board test, 2026-11-18 (all three switches on, without a serial console):
- boots from SD to Linux, Ethernet up, warm `reboot` comes back (Wi-Fi: see the second test, it was not up);
- BL31 on the card is TF-A `v2.14.1(release)`; Linux sees PSCI 1.1, SMCCC 1.5 and all 4 cores; cpufreq lists
  408..1800 MHz;
- the DDR PHY clock under Linux is 666 MHz (`clk_ddr1x`, half the DDR clock): the memory runs at 1332 MHz;
  `memtester 1200M 1` passed its first 13 tests (running);
- U-Boot on the card is the board build (its device tree and the button command are in `u-boot.itb`);
- not checked yet: the boot LED (that build pointed at LEDs the board does not have, fixed since), the button
  and USB mass storage, `poweroff`, the overnight memory test.

Second test, 2026-11-19 (serial console, boot firmware swapped on the card one part at a time):
- U-Boot/TF-A banners as expected: DDR 1332 MHz from the DDR blob, `BL31: v2.14.1(release)`, U-Boot 2026.01 with
  the G285 device tree; Ethernet 100/Full with DHCP, also over warm reboots;
- **Wi-Fi did not come up**: `mmc2: error -84 whilst initialising SDIO card` at 50 MHz on every boot. It came up
  at 25 MHz (`max-frequency` in the DTB), with the generic U-Boot, and with the board U-Boot once U-Boot left the
  SDIO bus alone (`&sdmmc1` disabled in `rk3566-bobcat-g285-u-boot.dtsi`), with TF-A from source and with the
  rkbin BL31 alike. The DDR clock and the RK809 (registers and regulator voltages identical between a good and a
  bad boot) were not the cause. U-Boot now leaves `sdmmc1` disabled.

The RK3566 boot chain:

```
boot ROM -> TPL: Rockchip DDR blob (rkbin) -> U-Boot SPL -> BL31 (TF-A) + U-Boot -> extlinux.conf -> Linux
```

The branch changes three parts of it. Each one has a switch in `conf/machine/bobcat-g285.conf`, so a board that
does not boot can be bisected by turning them back one at a time (in `local.conf` or a kas `local_conf_header`):

| Switch | Branch default | main | What it changes |
|---|---|---|---|
| `BOBCAT_UBOOT` | `board` | `generic` | U-Boot built with the board's own device tree |
| `BOBCAT_TFA` | `upstream` | `rkbin` | BL31 built from TF-A sources instead of Rockchip's blob |
| `BOBCAT_DDR_MHZ` | `1332` | `1056` | LPDDR4X clock in the DDR blob |

## U-Boot for the G285 (`BOBCAT_UBOOT = "board"`)

`bobcat-g285-rk3566_defconfig` is mainline's `generic-rk3568_defconfig` with:

- the board's device tree: U-Boot builds the same `rk3566-bobcat-g285.dts` as the kernel (`CONFIG_OF_UPSTREAM`),
  plus `rk3566-bobcat-g285-u-boot.dtsi`;
- the front RGB LED red from U-Boot's start (`LED_BOOT`), so a board stuck before Linux can be told from a dead one
  (Linux then switches to the green heartbeat);
- the button (SARADC, `adc-keys`): held at power-on, U-Boot shows the SD card and the eMMC as USB disks on the
  micro USB port (`ums 0 mmc 1,0`), so an image can be written from a computer without taking the card out or
  using Maskrom mode. Ctrl+C on the console ends it;
- the RK809 PMIC and its regulators, I2C, LED, button and regulator commands;
- the Wi-Fi SDIO controller (`sdmmc1`) disabled: probing it from U-Boot breaks Wi-Fi in Linux (second test above);
- no SPI flash support (the board has none).

To test:
- the board boots from SD as before;
- `led list` and `button list` show the LEDs and the button;
- with the button held at power-on, a computer on the micro USB port sees two disks;
- `poweroff` works.

Not done yet: Ethernet in U-Boot (network boot during development).

## TF-A from source (`BOBCAT_TFA = "upstream"`)

BL31 is the code that stays resident at the processor's highest privilege level (EL3) and handles PSCI requests from
Linux: starting and stopping CPU cores, reboot, power off. Rockchip ships it as a closed blob (rkbin
`rk3568_bl31_v1.44.elf`); upstream TF-A supports the RK3568/RK3566 as `PLAT=rk3568`, and meta-arm builds it
(`trusted-firmware-a` 2.14). With it, all firmware that runs after DDR set-up is open source.

To test:
- `nproc` shows 4 cores (PSCI CPU_ON);
- `reboot` and `poweroff` work;
- cpufreq still changes the CPU clock under load;
- no change in temperature or power draw at idle.

Known limits: upstream TF-A has no suspend support for this SoC; the gateway does not suspend anyway.

## DDR clock (`BOBCAT_DDR_MHZ`)

Rockchip's DDR blob initialises and trains the LPDDR4X before anything else runs. Its RK3566 builds stop at
1056 MHz, which is what the factory firmware uses. The branch writes a different LPDDR4X clock into the same blob
with rkbin's `ddrbin_tool` (meta-rockchip's `RKBIN_DDR_RECONFIGURE`); training and the rest of its settings stay.

This runs the memory above what Rockchip and Bobcat set for this board. Gains for the gateway itself are nil (the
CPU is mostly idle); it would matter for memory-bound work (NPU, compression, several containers). Risks: the
board does not start, or memory errors show up only under load or heat, silently corrupting data.

To test, in this order:
1. The console prints `DDR ... 1332MHz` early in the boot log and the board boots.
2. `memtester 1500M 5` with no failures.
3. Overnight: `stress-ng --vm 2 --vm-bytes 75% --verify --timeout 8h` plus a CPU load, with the board in its case.
4. Only then think about keeping it, and keep the factory 1056 MHz as the default for everyone else.
