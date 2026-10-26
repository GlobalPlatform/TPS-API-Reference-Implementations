#!/bin/bash
#***************************************************************************************************
# Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
# SPDX-License-Identifier: MIT
#**************************************************************************************************/
# Builds tock_rot13_client and tock_rot13_service for the earlgrey-cw310 (Ibex/QEMU) target,
# packages both as TBFs via elf2tab, and concatenates them into build/opentitan/apps.bin at their
# fixed flash slots (client @ 0x20030000, service @ 0x20038000, 0x8000 bytes each) for
# `run_opentitan_test.sh` to load. Must be re-run whenever either app's source changes.
set -euo pipefail
cd "$(dirname "$0")"

TARGET=riscv32imc-unknown-none-elf
OUT=build/opentitan
mkdir -p "$OUT"

LIBTOCK_LINKER_FLASH=0x20030000 LIBTOCK_LINKER_FLASH_LENGTH=0x8000 \
LIBTOCK_LINKER_RAM=0x10006000 LIBTOCK_LINKER_RAM_LENGTH=32K \
  cargo build --release -p tock_rot13_client --target "$TARGET"

LIBTOCK_LINKER_FLASH=0x20038000 LIBTOCK_LINKER_FLASH_LENGTH=0x8000 \
LIBTOCK_LINKER_RAM=0x1000E000 LIBTOCK_LINKER_RAM_LENGTH=32K \
  cargo build --release -p tock_rot13_service --target "$TARGET"

elf2tab --kernel-major 2 --kernel-minor 0 -n tock_rot13_client \
  -o "$OUT/tock_rot13_client.tab" --stack 8192 --app-heap 1024 --kernel-heap 1024 \
  "target/$TARGET/release/tock_rot13_client,riscv32imc"

elf2tab --kernel-major 2 --kernel-minor 0 -n tock_rot13_service \
  -o "$OUT/tock_rot13_service.tab" --stack 4096 --app-heap 1024 --kernel-heap 1024 \
  "target/$TARGET/release/tock_rot13_service,riscv32imc"

python3 - "$OUT/tock_rot13_client.tab" "$OUT/tock_rot13_service.tab" "$OUT/apps.bin" <<'PYEOF'
import sys, tarfile, struct

client_tab, service_tab, out_path = sys.argv[1:4]
SLOT = 0x8000

def extract_tbf(tab_path):
    with tarfile.open(tab_path) as t:
        for m in t.getmembers():
            if m.name.endswith('.tbf'):
                return t.extractfile(m).read()
    raise RuntimeError("no .tbf member in " + tab_path)

def make_padding(total_size):
    version, header_size = 2, 16
    word0 = version | (header_size << 16)
    word1 = total_size
    word2 = 0  # flags
    word3 = word0 ^ word1 ^ word2
    return struct.pack('<IIII', word0, word1, word2, word3) + b'\x00' * (total_size - 16)

client_tbf = extract_tbf(client_tab)
service_tbf = extract_tbf(service_tab)
if len(client_tbf) > SLOT:
    raise RuntimeError(f"client TBF ({len(client_tbf)}B) exceeds slot size ({SLOT}B)")
if len(service_tbf) > SLOT:
    raise RuntimeError(f"service TBF ({len(service_tbf)}B) exceeds slot size ({SLOT}B)")

# A padding TBF header itself needs 16 bytes, so it can only fill a *nonzero* gap; if the client's
# TBF exactly fills the slot, the service TBF simply starts immediately after with no gap at all.
remaining = SLOT - len(client_tbf)
padding = make_padding(remaining) if remaining > 0 else b''
with open(out_path, 'wb') as f:
    f.write(client_tbf + padding + service_tbf)

print(f"{out_path}: client {len(client_tbf)}B + padding {len(padding)}B + service {len(service_tbf)}B "
      f"= {len(client_tbf) + len(padding) + len(service_tbf)}B")
PYEOF
