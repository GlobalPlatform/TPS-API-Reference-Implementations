/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
// This crate is `#![no_std]`, but tests run under the host's std test harness
// (`libtock_unittest` is itself a plain std crate, pulled in only as a
// dev-dependency), so pull `std` back in for this module.
extern crate std;

use super::*;
use libtock_platform::{CommandReturn, ErrorCode, Syscalls as _};
use libtock_unittest::{
    command_return, fake, DriverInfo, DriverShareRef, ExpectedSyscall, RoAllowBuffer,
    RwAllowBuffer,
};
use std::cell::{Cell, RefCell};
use std::vec::Vec;

type Syscalls = fake::Syscalls;

// -----------------------------------------------------------------------------
// Fake IPC driver.
//
// `libtock_unittest::fake` has no IPC driver upstream, and (unlike a real
// second Tock process) `fake::Kernel` only ever models this one process, so
// this fake can't bounce a notify off an actual peer. Instead it plays both
// roles itself: on `NOTIFY_SERVICE` it immediately writes a configured reply
// into the shared buffer and schedules the matching upcall in-line, standing
// in for "the service ran and replied".
// -----------------------------------------------------------------------------

struct Ipc {
    share_ref: DriverShareRef,
    search_buffer: Cell<RoAllowBuffer>,
    rw_buffer: RefCell<RwAllowBuffer>,
    service_name: Vec<u8>,
    service_index: u32,
    service_reply: RefCell<Option<Vec<u8>>>,
}

impl Ipc {
    fn new(service_name: &[u8], service_index: u32) -> std::rc::Rc<Ipc> {
        std::rc::Rc::new(Ipc {
            share_ref: Default::default(),
            search_buffer: Default::default(),
            rw_buffer: Default::default(),
            service_name: service_name.to_vec(),
            service_index,
            service_reply: Default::default(),
        })
    }

    /// Configures the bytes written into the shared buffer the next time a
    /// `NOTIFY_SERVICE` command is received, simulating the service's reply.
    fn set_service_reply(&self, reply: &[u8]) {
        *self.service_reply.borrow_mut() = Some(reply.to_vec());
    }
}

impl fake::SyscallDriver for Ipc {
    fn info(&self) -> DriverInfo {
        DriverInfo::new(DRIVER_NUM).upcall_count(8)
    }

    fn register(&self, share_ref: DriverShareRef) {
        self.share_ref.replace(share_ref);
    }

    fn allow_readonly(
        &self,
        buffer_num: u32,
        buffer: RoAllowBuffer,
    ) -> Result<RoAllowBuffer, (RoAllowBuffer, ErrorCode)> {
        if buffer_num == ro_allow::SEARCH {
            Ok(self.search_buffer.replace(buffer))
        } else {
            Err((buffer, ErrorCode::Invalid))
        }
    }

    fn allow_readwrite(
        &self,
        _buffer_num: u32,
        buffer: RwAllowBuffer,
    ) -> Result<RwAllowBuffer, (RwAllowBuffer, ErrorCode)> {
        Ok(self.rw_buffer.replace(buffer))
    }

    fn command(&self, command_id: u32, argument0: u32, _argument1: u32) -> CommandReturn {
        match command_id {
            0 => command_return::success(),
            command::DISCOVER => {
                let buffer = self.search_buffer.take();
                let found = &*buffer == self.service_name.as_slice();
                self.search_buffer.set(buffer);
                if found {
                    command_return::success_u32(self.service_index)
                } else {
                    command_return::failure(ErrorCode::NoDevice)
                }
            }
            command::NOTIFY_SERVICE => {
                if let Some(reply) = self.service_reply.borrow_mut().take() {
                    let mut buf = self.rw_buffer.borrow_mut();
                    let n = reply.len().min(buf.len());
                    buf[..n].copy_from_slice(&reply[..n]);
                }
                let len = self.rw_buffer.borrow().len() as u32;
                self.share_ref
                    .schedule_upcall(argument0, (self.service_index, len, 0))
                    .expect("bad subscribe num in test fake");
                command_return::success()
            }
            command::NOTIFY_CLIENT => {
                self.share_ref
                    .schedule_upcall(argument0, (argument0, 0, 0))
                    .expect("bad subscribe num in test fake");
                command_return::success()
            }
            _ => command_return::failure(ErrorCode::NoSupport),
        }
    }
}

// -----------------------------------------------------------------------------
// discover
// -----------------------------------------------------------------------------

#[test]
fn discover_success() {
    let kernel = fake::Kernel::new();
    let driver = Ipc::new(b"rot13_service", 3);
    kernel.add_driver(&driver);

    let id = discover::<Syscalls>(b"rot13_service").unwrap();
    assert_eq!(id.as_raw(), 3);
}

#[test]
fn discover_not_found() {
    let kernel = fake::Kernel::new();
    let driver = Ipc::new(b"rot13_service", 3);
    kernel.add_driver(&driver);

    assert_eq!(discover::<Syscalls>(b"other"), Err(ErrorCode::NoDevice));
}

#[test]
fn discover_no_driver() {
    let _kernel = fake::Kernel::new();
    assert_eq!(
        discover::<Syscalls>(b"rot13_service"),
        Err(ErrorCode::NoDevice)
    );
}

#[test]
fn discover_allow_ro_failure() {
    let kernel = fake::Kernel::new();
    let driver = Ipc::new(b"rot13_service", 3);
    kernel.add_driver(&driver);
    kernel.add_expected_syscall(ExpectedSyscall::AllowRo {
        driver_num: DRIVER_NUM,
        buffer_num: ro_allow::SEARCH,
        return_error: Some(ErrorCode::NoMem),
    });

    assert_eq!(discover::<Syscalls>(b"rot13_service"), Err(ErrorCode::NoMem));
}

// -----------------------------------------------------------------------------
// call (client side)
// -----------------------------------------------------------------------------

#[test]
fn call_round_trip() {
    let kernel = fake::Kernel::new();
    let driver = Ipc::new(b"rot13_service", 3);
    kernel.add_driver(&driver);
    driver.set_service_reply(b"reply!!");

    let service = discover::<Syscalls>(b"rot13_service").unwrap();
    let mut buf = *b"request";
    call::<Syscalls>(service, &mut buf).unwrap();
    assert_eq!(&buf, b"reply!!");
}

#[test]
fn call_allow_rw_failure() {
    let kernel = fake::Kernel::new();
    let driver = Ipc::new(b"rot13_service", 3);
    kernel.add_driver(&driver);

    let service = discover::<Syscalls>(b"rot13_service").unwrap();
    kernel.add_expected_syscall(ExpectedSyscall::AllowRw {
        driver_num: DRIVER_NUM,
        buffer_num: service.as_raw(),
        return_error: Some(ErrorCode::NoMem),
    });

    let mut buf = *b"request";
    assert_eq!(call::<Syscalls>(service, &mut buf), Err(ErrorCode::NoMem));
}

#[test]
fn call_subscribe_failure_unallows_buffer() {
    let kernel = fake::Kernel::new();
    let driver = Ipc::new(b"rot13_service", 3);
    kernel.add_driver(&driver);

    let service = discover::<Syscalls>(b"rot13_service").unwrap();
    // The fake kernel's expected-syscall queue is popped on every intercepted
    // syscall regardless of kind, so `call`'s preceding AllowRw needs its own
    // (successful) entry before the Subscribe failure we're actually testing.
    kernel.add_expected_syscall(ExpectedSyscall::AllowRw {
        driver_num: DRIVER_NUM,
        buffer_num: service.as_raw(),
        return_error: None,
    });
    kernel.add_expected_syscall(ExpectedSyscall::Subscribe {
        driver_num: DRIVER_NUM,
        subscribe_num: service.as_raw(),
        skip_with_error: Some(ErrorCode::NoMem),
    });

    let mut buf = *b"request";
    assert_eq!(call::<Syscalls>(service, &mut buf), Err(ErrorCode::NoMem));
}

// -----------------------------------------------------------------------------
// subscribe_service / notify_client (service side)
// -----------------------------------------------------------------------------

#[test]
fn subscribe_service_receives_notify() {
    let kernel = fake::Kernel::new();
    let driver = Ipc::new(b"rot13_service", 3);
    kernel.add_driver(&driver);
    let own = discover::<Syscalls>(b"rot13_service").unwrap();

    let received: Cell<Option<ServiceUpcallArgs>> = Cell::new(None);
    unsafe { subscribe_service::<Syscalls>(own, &received) }.unwrap();

    // Simulate the kernel notifying this service, as it would if a client had
    // called `call`. The fake driver enqueues the upcall on the command call;
    // yield_wait pops and invokes it, just as a real kernel would deliver it.
    Syscalls::command(DRIVER_NUM, command::NOTIFY_SERVICE, own.as_raw(), 0)
        .to_result::<(), ErrorCode>()
        .unwrap();
    Syscalls::yield_wait();

    assert_eq!(received.get(), Some((3, 0, 0)));
    unsubscribe_service::<Syscalls>(own);
}

#[test]
fn notify_client_wakes_client_upcall() {
    let kernel = fake::Kernel::new();
    let driver = Ipc::new(b"rot13_service", 3);
    kernel.add_driver(&driver);
    let client = discover::<Syscalls>(b"rot13_service").unwrap();

    // `notify_client` and `subscribe_service` both key off a plain process
    // index, so reusing `subscribe_service` here exercises the exact same
    // raw_subscribe path a real client-notify upcall registration would.
    let received: Cell<Option<ServiceUpcallArgs>> = Cell::new(None);
    unsafe { subscribe_service::<Syscalls>(client, &received) }.unwrap();

    notify_client::<Syscalls>(client).unwrap();
    Syscalls::yield_wait();

    assert!(received.get().is_some());
    unsubscribe_service::<Syscalls>(client);
}

// -----------------------------------------------------------------------------
// with_shared_buffer
//
// `with_shared_buffer` takes `ptr` as `u32` to match Tock's actual (32-bit)
// process ABI. On a 64-bit host, `some_pointer as u32` truncates the address,
// so reconstructing a slice from that truncated value is unsound — this test
// only runs on 32-bit hosts, where the round trip is exact.
// -----------------------------------------------------------------------------

#[cfg(target_pointer_width = "32")]
#[test]
fn with_shared_buffer_reads_and_writes_in_place() {
    let mut backing = *b"hello!!";
    let result = unsafe {
        with_shared_buffer(backing.len() as u32, backing.as_mut_ptr() as u32, |buf| {
            buf[0] = b'H';
            buf.len()
        })
    };
    assert_eq!(result, 7);
    assert_eq!(&backing, b"Hello!!");
}
