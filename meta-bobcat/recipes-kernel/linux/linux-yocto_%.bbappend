# SPDX-License-Identifier: MIT
# Bobcat boards on linux-yocto: our device tree (kept as a plain .dts so it can be read, reviewed and later sent
# upstream as is) and a config fragment for the board's peripherals.

FILESEXTRAPATHS:prepend:bobcat := "${THISDIR}/files:"

# meta-rockchip lists its own boards one by one; linux-yocto itself only allows the QEMU machines.
COMPATIBLE_MACHINE:bobcat = "bobcat"

SRC_URI:append:bobcat = " \
    file://dts/rk3566-bobcat-g285.dts \
    file://bobcat.cfg \
    file://patches/0001-net-phy-motorcomm-add-YT8512-and-YT8512B.patch \
"

# Put the board DTS into the kernel tree next to the other Rockchip boards and list it in the Makefile, so the
# kernel's own dtb rules build it (KERNEL_DEVICETREE).
do_configure:prepend:bobcat() {
    dtsdir=${S}/arch/arm64/boot/dts/rockchip
    for dts in ${UNPACKDIR}/dts/*.dts; do
        name=$(basename "$dts" .dts)
        install -m 0644 "$dts" "$dtsdir/"
        grep -q "^dtb-\$(CONFIG_ARCH_ROCKCHIP) += $name.dtb$" "$dtsdir/Makefile" ||
            echo "dtb-\$(CONFIG_ARCH_ROCKCHIP) += $name.dtb" >> "$dtsdir/Makefile"
    done
}
