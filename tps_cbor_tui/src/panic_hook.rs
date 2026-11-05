/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
//! The terminal-restoring panic hook, and the flag that switches it off for
//! the duration of a call that catches its own unwinds.
//!
//! The hook and the flag live together because they are one mechanism: a hook
//! installed in one module and a flag it consults from another is a pair that
//! drifts apart the first time either is edited.

use std::cell::Cell;
use std::io;

use crossterm::execute;
use crossterm::terminal::{LeaveAlternateScreen, disable_raw_mode};

thread_local! {
    /// How many [`HookSuppressed`] guards are live on this thread.
    static SUPPRESSED: Cell<u32> = const { Cell::new(0) };
}

/// Suppresses the terminal-restoring panic hook for this guard's lifetime.
///
/// `tps_cbor_core`'s CDDL entry points run `build_cddl` under
/// `catch_unwind` — `tps_cbor_core/src/cddl/validate.rs`'s `guarded` — and
/// report the panic as a diagnostic instead of propagating it, because
/// `build_cddl` documents a panic as possible on malformed-but-grammatical
/// input. [`install`]'s hook cannot tell that unwind from a fatal one: it
/// would disable raw mode and leave the alternate screen while the app went
/// on running and drawing into it. That is strictly worse than what this
/// crate did before it called into core, which was to crash cleanly.
///
/// Hold one of these across every call into a core CDDL entry point. The
/// panic is still reported — core turns it into an error diagnostic, which
/// lands in the pane the user is looking at.
///
/// A depth counter rather than a `bool`: a nested guard's `Drop` would
/// otherwise re-enable the hook while the outer call was still inside its
/// `catch_unwind`. Thread-local is the right scope because `catch_unwind`
/// catches only unwinds on its own thread.
#[must_use = "the hook is only suppressed while the guard is alive"]
pub struct HookSuppressed;

impl HookSuppressed {
    pub fn new() -> Self {
        SUPPRESSED.with(|flag| flag.set(flag.get().saturating_add(1)));
        Self
    }
}

impl Drop for HookSuppressed {
    fn drop(&mut self) {
        SUPPRESSED.with(|flag| flag.set(flag.get().saturating_sub(1)));
    }
}

/// Whether a [`HookSuppressed`] guard is live on this thread.
fn suppressed() -> bool {
    SUPPRESSED.with(|flag| flag.get() > 0)
}

/// Ensure the terminal is restored even if the app panics, so the shell isn't
/// left broken — unless a [`HookSuppressed`] guard says the unwind is about to
/// be caught and the app will carry on.
pub fn install() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if suppressed() {
            // Not even the default hook: its message would be written into the
            // alternate screen, over the UI that is about to keep running.
            return;
        }
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
        default_hook(info);
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_guard_means_the_hook_runs() {
        assert!(!suppressed());
    }

    #[test]
    fn a_guard_suppresses_until_it_is_dropped() {
        {
            let _guard = HookSuppressed::new();
            assert!(suppressed());
        }
        assert!(!suppressed());
    }

    #[test]
    fn an_inner_guard_being_dropped_leaves_the_outer_one_in_force() {
        let _outer = HookSuppressed::new();
        {
            let _inner = HookSuppressed::new();
            assert!(suppressed());
        }
        assert!(
            suppressed(),
            "dropping the inner guard must not re-enable the hook inside the outer call"
        );
    }
}
