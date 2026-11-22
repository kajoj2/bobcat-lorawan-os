# SPDX-License-Identifier: MIT
SUMMARY = "Command line tools of the gateway, in place of busybox"
DESCRIPTION = "The GNU and util-linux tools a shell and the gateway's scripts expect (the distro's \
VIRTUAL-RUNTIME_base-utils): a smaller pick than oe-core's packagegroup-core-base-utils, which also brings a DHCP \
client next to systemd-networkd, telnet, tftp and vim."
LICENSE = "MIT"

inherit packagegroup

RDEPENDS:${PN} = " \
    bash \
    bash-completion \
    coreutils \
    diffutils \
    file \
    findutils \
    gawk \
    grep \
    sed \
    tar \
    gzip \
    pigz \
    xz \
    bzip2 \
    less \
    which \
    procps \
    psmisc \
    kmod \
    iproute2 \
    iproute2-ss \
    iputils-ping \
    util-linux \
    ncurses-tools \
    curl \
    wget \
"
