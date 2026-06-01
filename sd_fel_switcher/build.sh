#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")"

TARGET=riscv64imac-unknown-none-elf
HOST=$(rustc -vV | sed -n 's/^host: //p')
ELF=target/${TARGET}/release/sd_fel_switcher
EGON_PAD=target/${HOST}/release/egon_pad
BIN=sd_fel_switcher.bin
OBJCOPY=${OBJCOPY:-riscv64-linux-gnu-objcopy}

build() {
  cargo build --release --bin sd_fel_switcher
  cargo build --release --bin egon_pad --target "${HOST}"
  # .text only: newer linkers may place .note.gnu.build-id before .text at 0x20000
  "${OBJCOPY}" -O binary --only-section=.text "${ELF}" "${BIN}"
  "${EGON_PAD}" "${BIN}"
}

clean() {
  cargo clean
  rm -f "${BIN}"
}

case "${1:-build}" in
  build) build ;;
  clean) clean ;;
  *)
    echo "usage: $0 [build|clean]" >&2
    exit 1
    ;;
esac
