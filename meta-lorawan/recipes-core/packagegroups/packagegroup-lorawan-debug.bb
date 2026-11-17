# SPDX-License-Identifier: MIT
SUMMARY = "Bring-up and diagnostic tools for gateway boards"
DESCRIPTION = "Not part of the default image. PHY registers (phytool), RAM and load tests (memtester, \
stress-ng), input events (evtest, for the board's button), packet capture and throughput (tcpdump, iperf3), \
eMMC health (mmc-utils) and SPI access (spitools). Enabled with LORAWAN_DEBUG_TOOLS = \"1\" or \
kas/debug-tools.yml."
LICENSE = "MIT"

inherit packagegroup

RDEPENDS:${PN} = " \
    phytool \
    memtester \
    stress-ng \
    evtest \
    tcpdump \
    iperf3 \
    mmc-utils \
    spitools \
"
