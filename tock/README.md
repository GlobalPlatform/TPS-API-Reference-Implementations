# TockOS bare-metal build (Phase 6)

Builds and runs the TPS Client API reference implementation (client + ROT13
service, as two separate Tock processes over Tock IPC) on two targets:

- Raspberry Pi Pico (Arm Cortex-M0+, `thumbv6m-none-eabi`) — your hardware, manual flash/observe.
- Pavona/Ibex (`earlgrey-cw310`, RISC-V, `riscv32imc-unknown-none-elf`) — run under Pavona's
  vendored QEMU, automated serial-output check.

See `specifications/TPS_CLIENT_API_PLAN.md` (Phase 6) for the full design and status.

## Layout

- `third_party/tock` — Tock kernel, submodule pinned at Pavona's vendored commit.
- `third_party/libtock-rs` — Tock Rust userspace runtime, submodule pinned at Pavona's vendored commit.
- `third_party/elf2tab` — TBF packaging tool, submodule pinned at Pavona's vendored commit.
- `patches/earlgrey-cw310-enable-ipc.patch` — enables the IPC capsule on the `earlgrey-cw310` board
  (upstream ships it disabled); apply to `third_party/tock` before building for Ibex.
- `tock_ipc`, `tock_connector`, `tock_rot13_client`, `tock_rot13_service` — this project's crates.

This directory is its own Cargo workspace, deliberately isolated from the repo root workspace, so
that the root `cargo build --workspace` (host targets only) is unaffected.

## Required local setup

1. Rust targets: `rustup target add thumbv6m-none-eabi riscv32imc-unknown-none-elf`
2. A toolchain able to satisfy both `third_party/tock`'s `rust-toolchain.toml` (nightly-2023-07-30,
   `rust-src`) and `third_party/libtock-rs`'s (stable 1.70+) — these apply automatically via rustup
   overrides when building inside each submodule.
3. `elf2tab`: build from the submodule (`cargo install --path third_party/elf2tab`), or `cargo
   install elf2tab` if a compatible published version is acceptable.
4. `elf2uf2-rs` (built from crates.io) for flashing the Pico:
   `cargo install elf2uf2-rs --no-default-features` (the default features pull in `libudev`, for
   auto-detecting the BOOTSEL drive — not needed by `flash_pico.sh`, which copies the UF2 manually).
   Also needs GNU `objcopy` (not LLVM's `llvm-objcopy`) — see the Pico section below for why.
5. The lowRISC QEMU binary already vendored by your Pavona checkout
   (`qemu-ot-earlgrey-v9.2.0-2025-02-11-x86_64-unknown-linux-gnu`) — point the Ibex test script at
   it; it is not vendored in this repo.

## Submodules

Initialize/update with:

```
git submodule update --init tock/third_party/tock tock/third_party/libtock-rs tock/third_party/elf2tab
```

Each is pinned at the commit Pavona vendors as of this writing:

- `tock/tock` @ `e81987f6a41e9b92f60fda1d5283f46b3cb597b5`
- `tock/libtock-rs` @ `a2c6ad80648e3ba073e7433b4330706df052a6ae`
- `tock/elf2tab` @ `2f0e2f0ef01e37799850d1b12f48b93a0b32a203`

## Pavona/Ibex (QEMU, automated)

```
./build_opentitan.sh      # builds both apps, packages apps.bin
./run_opentitan_test.sh   # boots the kernel under QEMU, asserts the round-trip PASS marker
```

The `earlgrey-cw310` kernel itself is built separately, via `make` inside WSL2 (board-level `cargo
build` fails with linker errors — the board Makefile supplies required `RUSTFLAGS`/linker-script
wiring that plain `cargo build` does not):

```
wsl -d Ubuntu-24.04
cd third_party/tock/boards/opentitan/earlgrey-cw310
make
```

Apply `patches/earlgrey-cw310-enable-ipc.patch` to the `third_party/tock` submodule first — upstream
ships this board with the IPC capsule compiled out.

## Raspberry Pi Pico (manual, real hardware)

This target needs your physical Pico attached, so it isn't automated — build it, flash it, then
watch the serial console yourself.

1. Build the kernel (inside WSL2, same reason as Ibex above):

   ```
   wsl -d Ubuntu-24.04
   cd third_party/tock/boards/raspberry_pi_pico
   make
   ```

2. Build both apps and package them:

   ```
   ./build_pico.sh
   ```

3. Produce a flashable UF2 (merges `build/pico/apps.bin` into the kernel's `.apps` section, then
   converts to UF2):

   ```
   ./flash_pico.sh
   ```

   Run this under WSL2 — it needs GNU `objcopy` and `elf2uf2-rs` on `PATH`. LLVM's `llvm-objcopy`
   (the one bundled with the Rust toolchain) can't do this: it refuses to grow the kernel's `.apps`
   placeholder section in place, since that section sits inside an already-loaded ELF segment. GNU
   binutils' `objcopy` (already on WSL2's `PATH` via `build-essential`) handles it correctly.

4. Flash: hold the **BOOTSEL** button on the Pico, plug it in over USB while still holding it, then
   release. It mounts as a USB mass-storage drive (`RPI-RP2`). Copy
   `build/pico/raspberry_pi_pico-with-apps.uf2` onto that drive. The Pico reboots automatically into
   the new kernel+apps.

5. Observe: open a serial console on the port the Pico enumerates as (`/dev/ttyACM0` on Linux; under
   WSL2, USB serial devices are not passed through automatically, so you'll most likely need to open
   the console from Windows itself — e.g. via PuTTY or `python -m serial.tools.miniterm` against the
   Pico's `COMn` port — at 115200 baud). Expect the same round-trip sequence as the Ibex/QEMU test:
   `debug: service_discovery found 1`, `debug: open_session ok`, `debug: ciphertext = ...`, and
   finally `tock_rot13_client: PASS` (or `FAIL`, with a preceding `debug:` line indicating which step
   failed).

Both apps' RAM addresses (`0x20004000` for the client, `0x20014000` for the service, each 32K) were
checked against the real linked kernel's `_sappmem`/`_eappmem` bounds (obtained via `nm` on the built
kernel: `0x200034ac`–`0x20042000`) to confirm neither app's RAM region overlaps the kernel's own
memory or the other app's region. This does not match `libtock-rs`'s own hardcoded `PLATFORMS` table
entry for `raspberry_pi_pico` (which claims a RAM start of `0x20012000`) — that table is only
consulted when building via `LIBTOCK_PLATFORM` instead of explicit `LIBTOCK_LINKER_*` addresses
(which is what `build_pico.sh` uses), so the mismatch doesn't affect this build.
