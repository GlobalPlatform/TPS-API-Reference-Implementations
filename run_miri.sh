#!/usr/bin/env bash
#***************************************************************************************************
# Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
# SPDX-License-Identifier: MIT
#**************************************************************************************************/
#
# Run the test suites of the pointer-handling crates under Miri, to check the unsafe code at the
# C FFI boundary for undefined behaviour that normal compilation cannot detect.
#
# Miri interprets MIR rather than running native code, so it catches aliasing and pointer
# provenance violations (Stacked Borrows / Tree Borrows), out-of-bounds accesses, use of
# uninitialized memory, invalid alignment and data races. These are exactly the failure modes the
# raw pointers crossing `extern "C"` in this workspace are exposed to, and all of them can be
# present in code that compiles without warnings and passes `cargo test`.
#
# Usage:
#   ./run_miri.sh                 # all Miri-capable crates, Stacked Borrows (the default model)
#   ./run_miri.sh -t              # same, but with the Tree Borrows aliasing model
#   ./run_miri.sh -p tps_client_c # a single crate
#   ./run_miri.sh -- some_filter  # pass a test-name filter through to the harness
#
# Requires a nightly toolchain with the `miri` component:
#   rustup toolchain install nightly
#   rustup component add --toolchain nightly miri rust-src
#
set -euo pipefail

cd "$(dirname "$0")"

# Crates whose test suites exercise raw pointers, `extern "C"` boundaries or `unsafe` blocks.
# The remaining workspace members are either pure safe Rust (`tps_minicbor`, `tps_cose`,
# `tps_cddl`, the CLI/TUI tools) or link C code that Miri cannot execute (`tree-sitter-cddl`,
# `tree-sitter-edn`, and so anything depending on them).
DEFAULT_PACKAGES=(
    tps_client_common
    tps_error
    tps_connector
    tps_client_api
    tps_client_c
    rot13_connector
)

# `-Zmiri-disable-isolation` is required because several tests read the cbindgen-generated
# `tpsc_client_api.h` from disk, and Miri denies filesystem access by default. It grants host
# access only; it does not relax any of the UB checks.
MIRI_BASE_FLAGS="-Zmiri-disable-isolation"

TREE_BORROWS=0
PACKAGES=()
FILTER_ARGS=()

while [[ $# -gt 0 ]]; do
    case "$1" in
        -t|--tree-borrows) TREE_BORROWS=1; shift ;;
        -p|--package)      PACKAGES+=("$2"); shift 2 ;;
        --)                shift; FILTER_ARGS=("$@"); break ;;
        -h|--help)         sed -n '7,25p' "$0"; exit 0 ;;
        *)                 echo "unknown argument: $1" >&2; exit 2 ;;
    esac
done

if [[ ${#PACKAGES[@]} -eq 0 ]]; then
    PACKAGES=("${DEFAULT_PACKAGES[@]}")
fi

MIRIFLAGS="$MIRI_BASE_FLAGS"
MODEL="Stacked Borrows"
if [[ $TREE_BORROWS -eq 1 ]]; then
    MIRIFLAGS="$MIRIFLAGS -Zmiri-tree-borrows"
    MODEL="Tree Borrows"
fi
export MIRIFLAGS

if ! cargo +nightly miri --version >/dev/null 2>&1; then
    cat >&2 <<'EOF'
error: cargo-miri is not available on the `nightly` toolchain.

Install it with:
    rustup toolchain install nightly
    rustup component add --toolchain nightly miri rust-src
EOF
    exit 1
fi

echo "Miri: $(cargo +nightly miri --version)"
echo "Aliasing model: $MODEL"
echo "MIRIFLAGS=$MIRIFLAGS"
echo

# Tests that cannot run under Miri are marked `#[cfg_attr(miri, ignore = "...")]` in the source
# rather than filtered here, so that the reason travels with the test. They are reported as
# "ignored" in the output below.
failed=()
for pkg in "${PACKAGES[@]}"; do
    echo "===== $pkg ====="
    if cargo +nightly miri test -p "$pkg" --features std -- "${FILTER_ARGS[@]+${FILTER_ARGS[@]}}"; then
        echo "----- $pkg: ok"
    else
        echo "----- $pkg: FAILED"
        failed+=("$pkg")
    fi
    echo
done

if [[ ${#failed[@]} -ne 0 ]]; then
    echo "Miri reported problems in: ${failed[*]}" >&2
    exit 1
fi

echo "Miri found no undefined behaviour in: ${PACKAGES[*]}"
