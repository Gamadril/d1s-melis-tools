#!/usr/bin/env bash
# Write SPI NOR image via xfel_spi_nor (device in FEL mode, USB connected).
#
# Usage:
#   ./xfel_spinor_recovery.sh flash_dump.bin
set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "$0")" && pwd)
XFEL=${SCRIPT_DIR}/bin/xfel_spi_nor

[[ $# -eq 1 ]] || { echo "usage: $0 <flash_dump.bin>" >&2; exit 1; }
IMG=$1
[[ -f ${IMG} ]] || { echo "not a file: ${IMG}" >&2; exit 1; }

[[ -x ${XFEL} ]] || {
  echo "xfel_spi_nor not found: ${XFEL}" >&2
  exit 1
}

run_xfel() {
  if [[ ${EUID} -eq 0 ]]; then
    "${XFEL}" "$@"
  else
    sudo "${XFEL}" "$@"
  fi
}

bytes=$(wc -c <"${IMG}" | tr -d ' ')
echo "==> ${XFEL} version"
run_xfel version
echo "==> write ${bytes} bytes @ 0 <- ${IMG}"
read -r -p "This will ERASE and overwrite SPI NOR. Continue? [y/N] " ans
[[ ${ans} == [yY] || ${ans} == [yY][eE][sS] ]] || exit 1
run_xfel write 0 "${IMG}"
echo "==> done"
