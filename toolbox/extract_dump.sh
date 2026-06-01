#!/usr/bin/env bash
# Extract SPI NOR dump into ./workspace/ using dump_tool.
#
# Usage:
#   ./extract_dump.sh <dump.bin>
set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "$0")" && pwd)
DUMP_TOOL=${SCRIPT_DIR}/bin/dump_tool
WORKSPACE=${SCRIPT_DIR}/workspace

[[ $# -eq 1 ]] || { echo "usage: $0 <dump.bin>" >&2; exit 1; }
DUMP=$1
[[ -f ${DUMP} ]] || { echo "not a file: ${DUMP}" >&2; exit 1; }
[[ -x ${DUMP_TOOL} ]] || { echo "dump_tool not found: ${DUMP_TOOL}" >&2; exit 1; }

if [[ -e ${WORKSPACE} ]]; then
  read -r -p "workspace exists. Replace? [y/N] " ans
  [[ ${ans} == [yY] || ${ans} == [yY][eE][sS] ]] || exit 1
  rm -rf "${WORKSPACE}"
fi

echo "==> extract ${DUMP} -> ${WORKSPACE}"
"${DUMP_TOOL}" extract "${DUMP}" "${WORKSPACE}"
echo "==> done"
