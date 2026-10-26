/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
use std::marker::PhantomData;
use std::ops::{Deref, DerefMut};

use tps_client_common::c_structs::MessageBuffer;

/// RAII wrapper around [`MessageBuffer`] for Rust callers of the TPS Client API.
///
/// Constructing a [`MessageBuf`] performs the equivalent of `TPSC_InitializeTransaction`; dropping
/// it performs the equivalent of `TPSC_FinalizeTransaction` (sanitizing the backing buffer and
/// resetting the message size). Per Annex A.3.2's informative `SafeMessageBuf` sketch, this
/// removes the need for callers to invoke initialize/finalize explicitly.
#[repr(transparent)]
#[derive(Debug)]
pub struct MessageBuf<'a> {
    inner: MessageBuffer,
    _borrow: PhantomData<&'a mut [u8]>,
}

impl<'a> MessageBuf<'a> {
    /// Initialize a new transaction buffer over `buffer`.
    pub fn new(buffer: &'a mut [u8]) -> Self {
        MessageBuf {
            inner: MessageBuffer::init(buffer),
            _borrow: PhantomData,
        }
    }
}

impl Drop for MessageBuf<'_> {
    fn drop(&mut self) {
        self.inner.finalize();
    }
}

impl Deref for MessageBuf<'_> {
    type Target = MessageBuffer;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl DerefMut for MessageBuf<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}
