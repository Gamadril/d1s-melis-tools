#!/usr/bin/env bash
# Repack ./workspace/ into a SPI NOR dump using dump_tool.
#
# Usage:
#   ./pack_dump.sh [output.bin]
set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "$0")" && pwd)
DUMP_TOOL=${SCRIPT_DIR}/bin/dump_tool
WORKSPACE=${SCRIPT_DIR}/workspace

OUT=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    -h|--help)
      sed -n '2,6p' "$0" | sed 's/^# \{0,1\}//'
      exit 0
      ;;
    -*) echo "unknown option: $1" >&2; exit 1 ;;
    *)
      [[ -z ${OUT} ]] || { echo "extra argument: $1" >&2; exit 1; }
      OUT=$1
      shift
      ;;
  esac
done

OUT=${OUT:-nor_repacked_$(date +%Y%m%d).bin}
[[ -d ${WORKSPACE} ]] || { echo "workspace not found: ${WORKSPACE}" >&2; exit 1; }
[[ -x ${DUMP_TOOL} ]] || { echo "dump_tool not found: ${DUMP_TOOL}" >&2; exit 1; }

if [[ -e ${OUT} ]]; then
  read -r -p "${OUT} exists. Replace? [y/N] " ans
  [[ ${ans} == [yY] || ${ans} == [yY][eE][sS] ]] || exit 1
  rm -f "${OUT}"
fi

echo "==> pack ${WORKSPACE} -> ${OUT}"
"${DUMP_TOOL}" pack "${WORKSPACE}" "${OUT}"
echo "==> done"
