#!/bin/bash
#***************************************************************************************************
# Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
# SPDX-License-Identifier: MIT
#**************************************************************************************************/
# Produces a flashable UF2 for the Raspberry Pi Pico (Phase 6, task #27): merges
# build/pico/apps.bin (client + service TBFs, see build_pico.sh) into the built Pico kernel's
# `.apps` ELF section, then converts the result to UF2 for BOOTSEL drag-and-drop flashing.
#
# Run ./build_pico.sh first if either app's source has changed, and build the Pico kernel first
# (inside WSL2 — the board Makefile's linker-script wiring is required, plain `cargo build` at the
# board level fails with undefined-symbol errors):
#   wsl -d Ubuntu-24.04
#   cd third_party/tock/boards/raspberry_pi_pico && make
#
# This script must run where `objcopy` (GNU binutils) and `elf2uf2-rs` are on PATH — on Windows,
# that means under WSL2. LLVM's `llvm-objcopy` (the one bundled with the Rust toolchain) cannot do
# this: it refuses to grow the kernel's `.apps` placeholder section in place because that section
# sits inside an already-loaded ELF segment. GNU objcopy handles it. `elf2uf2-rs` can be installed
# with `cargo install elf2uf2-rs --no-default-features` (the default `--features` pulls in
# `libudev` for BOOTSEL-drive auto-detection, which isn't needed for this manual flow).
set -euo pipefail
cd "$(dirname "$0")"

TARGET=thumbv6m-none-eabi
KERNEL=third_party/tock/target/$TARGET/release/raspberry_pi_pico.elf
APPS_BIN=build/pico/apps.bin
OUT=build/pico
KERNEL_WITH_APPS="$OUT/raspberry_pi_pico-with-apps.elf"
UF2="$OUT/raspberry_pi_pico-with-apps.uf2"

if [ ! -f "$KERNEL" ]; then
  echo "error: kernel not found at $KERNEL — build raspberry_pi_pico first (see header comment)" >&2
  exit 1
fi
if [ ! -f "$APPS_BIN" ]; then
  echo "error: $APPS_BIN not found — run ./build_pico.sh first" >&2
  exit 1
fi
if ! command -v objcopy >/dev/null; then
  echo "error: objcopy (GNU binutils) not found on PATH — run this under WSL2" >&2
  exit 1
fi
if ! command -v elf2uf2-rs >/dev/null; then
  echo "error: elf2uf2-rs not found on PATH — install with:" >&2
  echo "  cargo install elf2uf2-rs --no-default-features" >&2
  exit 1
fi

cp "$KERNEL" "$KERNEL_WITH_APPS"
objcopy --update-section .apps="$APPS_BIN" "$KERNEL_WITH_APPS"
elf2uf2-rs "$KERNEL_WITH_APPS" "$UF2"

echo "=== $UF2 ready to flash ==="
echo "Hold BOOTSEL on the Pico, plug it in (it mounts as a USB mass-storage drive named RPI-RP2),"
echo "then copy $UF2 onto that drive. The Pico reboots into the new kernel+apps automatically."
