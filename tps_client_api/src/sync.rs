/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/

//! Minimal `critical_section`-backed replacements for the `once_cell::sync::Lazy` /
//! `spin::Mutex` / `state::Storage` combination previously used by [`crate::services`].
//!
//! Those crates lean on hardware CAS atomics (`compare_exchange`, `fetch_add`, ...) that are
//! simply not implemented in `core` for targets without a CAS-capable atomic extension — notably
//! Armv6-M (Cortex-M0/M0+, e.g. the Raspberry Pi Pico) and RV32IMC without the 'A' extension (e.g.
//! Ibex). `critical_section` sidesteps this: mutual exclusion comes from disabling preemption for
//! the critical section's duration, so the guarded state only ever needs plain, non-atomic
//! loads/stores.

use core::cell::UnsafeCell;

/// A cell that is initialized at most once, on first access, and is freely, immutably readable
/// afterwards.
pub(crate) struct OnceCell<T> {
    value: UnsafeCell<Option<T>>,
}

// Safety: all mutation happens inside `critical_section::with`, which provides the same mutual
// exclusion a `Sync` bound promises from the caller's perspective.
unsafe impl<T> Sync for OnceCell<T> {}

impl<T> OnceCell<T> {
    /// Construct an empty, uninitialized cell.
    pub(crate) const fn new() -> Self {
        Self {
            value: UnsafeCell::new(None),
        }
    }

    /// Returns the stored value, initializing it via `init` on the first call. `init` may run more
    /// than once as `T` is dropped from the check-then-write path only when the check requires it,
    /// but the critical section ensures only one of those results is ever stored.
    pub(crate) fn get_or_init(&self, init: impl FnOnce() -> T) -> &T {
        critical_section::with(|_cs| {
            // Safety: exclusive access to `value` for the duration of this critical section.
            let slot = unsafe { &mut *self.value.get() };
            if slot.is_none() {
                *slot = Some(init());
            }
        });
        // Safety: the critical section above guarantees the slot is populated by this point, and
        // it is never cleared or overwritten again, so this racing-free (single-core) read is safe.
        unsafe { (*self.value.get()).as_ref().unwrap() }
    }
}

/// A `critical_section`-guarded cell, standing in for `spin::Mutex` without requiring CAS atomics.
pub(crate) struct Mutex<T> {
    value: UnsafeCell<T>,
}

// Safety: as with `OnceCell`, all access to `value` happens inside `critical_section::with`.
unsafe impl<T> Sync for Mutex<T> {}

impl<T> Mutex<T> {
    /// Construct a new guarded cell wrapping `value`.
    pub(crate) const fn new(value: T) -> Self {
        Self {
            value: UnsafeCell::new(value),
        }
    }

    /// Run `f` with exclusive, mutable access to the guarded value inside a critical section.
    pub(crate) fn lock<R>(&self, f: impl FnOnce(&mut T) -> R) -> R {
        critical_section::with(|_cs| {
            // Safety: exclusive access to `value` for the duration of this critical section.
            let value = unsafe { &mut *self.value.get() };
            f(value)
        })
    }
}
