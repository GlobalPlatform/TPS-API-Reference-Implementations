#!/bin/bash
#***************************************************************************************************
# Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
# SPDX-License-Identifier: MIT
#**************************************************************************************************/
set -e
QEMU="/c/Program Files/qemu/qemu-system-riscv32.exe"
KERNEL="$1"
shift
"$QEMU" \
  -machine virt \
  -semihosting \
  -global virtio-mmio.force-legacy=false \
  -device virtio-rng-device \
  -serial file:qemu_serial.log \
  -display none \
  -bios "$KERNEL" \
  "$@"
