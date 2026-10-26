#!/bin/bash
#***************************************************************************************************
# Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
# SPDX-License-Identifier: MIT
#**************************************************************************************************/
# Automated Ibex/QEMU round-trip test (Phase 6, task #26): boots the earlgrey-cw310 kernel with
# tock_rot13_client + tock_rot13_service loaded, captures serial output, and asserts the client's
# PASS marker appears. Run `./build_opentitan.sh` first if either app's source has changed.
#
# QEMU itself is not vendored here — pass its path via QEMU_BIN, or rely on it being on WSL2's
# PATH (this project's QEMU runs under WSL2 via a Pavona-vendored lowRISC build).
set -euo pipefail
cd "$(dirname "$0")"

TIMEOUT_SECS="${TIMEOUT_SECS:-45}"
QEMU_BIN="${QEMU_BIN:-qemu-system-riscv32}"
KERNEL=third_party/tock/target/riscv32imc-unknown-none-elf/release/earlgrey-cw310
APPS_BIN=build/opentitan/apps.bin

if [ ! -f "$KERNEL" ]; then
  echo "error: kernel not found at $KERNEL — build earlgrey-cw310 first" >&2
  exit 1
fi
if [ ! -f "$APPS_BIN" ]; then
  echo "error: $APPS_BIN not found — run ./build_opentitan.sh first" >&2
  exit 1
fi

OUT="$(mktemp)"
trap 'rm -f "$OUT"' EXIT

set +e
# `-icount shift=0` runs QEMU in deterministic instruction-counted mode. Without it, this
# specific board/scheduler/WSL2 combination is flaky: the client's IPC notify to the service
# reliably enqueues (confirmed via kernel-side tracing), but the service's queued upcall is not
# always dispatched before the run's timeout — a QEMU/host-scheduling timing issue, not a kernel
# or app bug (see ruling in specifications/TPS_CLIENT_API_PLAN.md). `-icount shift=0` has produced
# PASS on every run tested; without it, the round trip stalls after "ready" on most runs.
env -i PATH="/c/Windows/system32:/c/Windows" wsl -d Ubuntu-24.04 bash -c "
  cd /mnt/c/Users/jodonogh/Code/OpenSource/TPS-API-Reference-Implementations/tock &&
  timeout $TIMEOUT_SECS $QEMU_BIN \
    -M opentitan \
    -kernel $KERNEL \
    -device loader,file=$APPS_BIN,addr=0x20030000 \
    -nographic -serial mon:stdio \
    -icount shift=0 \
    -global driver=riscv.lowrisc.ibex.soc,property=resetvec,value=0x20000400
" < /dev/null > "$OUT" 2>&1
set -e

cat "$OUT"

if grep -q "tock_rot13_client: PASS" "$OUT"; then
  echo "=== PASS: tock_rot13_client reported PASS ==="
  exit 0
elif grep -q "tock_rot13_client: FAIL" "$OUT"; then
  echo "=== FAIL: tock_rot13_client reported FAIL ===" >&2
  exit 1
else
  echo "=== FAIL: no PASS/FAIL marker found in captured output (timeout or crash?) ===" >&2
  exit 1
fi
