#!/usr/bin/env bash
# Dump full SPI NOR via xfel_spi_nor (device in FEL mode, USB connected).
#
# Prereq: board in FEL (e.g. sd_fel_switcher), release layout with bin/xfel_spi_nor
#
# Usage:
#   ./xfel_spinor_dump.sh [output.bin]
#   ./xfel_spinor_dump.sh -h
set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "$0")" && pwd)
XFEL=${SCRIPT_DIR}/bin/xfel_spi_nor

usage() {
  sed -n '2,9p' "$0" | sed 's/^# \{0,1\}//'
  exit "${1:-0}"
}

OUT=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    -h|--help) usage 0 ;;
    -*) echo "unknown option: $1" >&2; usage 1 ;;
    *)
      [[ -z ${OUT} ]] || { echo "extra argument: $1" >&2; exit 1; }
      OUT=$1
      shift
      ;;
  esac
done

OUT=${OUT:-nor_dump_$(date +%Y%m%d).bin}

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

parse_spinor_size() {
  local out=$1
  if [[ ${out} =~ with[[:space:]]+([0-9]+)[[:space:]]+bytes ]]; then
    echo "${BASH_REMATCH[1]}"
  elif [[ ${out} =~ ^[^[:space:]]+[[:space:]]+([0-9]+)[[:space:]]*$ ]]; then
    echo "${BASH_REMATCH[1]}"
  else
    return 1
  fi
}

echo "==> ${XFEL} version"
run_xfel version

echo "==> ${XFEL} spinor"
spinor_out=$(run_xfel spinor 2>&1) || {
  echo "${spinor_out}" >&2
  exit 1
}
echo "${spinor_out}"

size=$(parse_spinor_size "${spinor_out}") || {
  echo "could not parse NOR size from device" >&2
  exit 1
}

echo "==> read ${size} bytes @ 0 -> ${OUT}"
run_xfel read 0 "${size}" "${OUT}"

bytes=$(wc -c <"${OUT}" | tr -d ' ')
echo "==> done: ${OUT} (${bytes} bytes)"
if [[ "${bytes}" -ne "${size}" ]]; then
  echo "warning: file size != requested length" >&2
  exit 1
fi
