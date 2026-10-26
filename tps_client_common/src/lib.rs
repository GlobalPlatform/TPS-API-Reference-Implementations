/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
/***************************************************************************************************
 * tps_client_common
 *
 * Definitions (constants and structures) shared between the C language interface to the TPS Client
 * API and the Rust implementation. These are split into a separate crate (which contains
 * essentially no code) to avoid creating circular dependencies.
 *
 **************************************************************************************************/
#![no_std]

//! Definitions (constants and structures) shared between the C language interface to the TPS
//! Client API and the Rust implementation.
//!
//! Split into a separate crate (which contains essentially no code) to avoid creating circular
//! dependencies between `tps_client_c`/`tps_connector` and `tps_client_api`.

#[cfg(any(feature = "std", test))]
extern crate std;
#[cfg(all(not(feature = "std"), not(test)))]
extern crate core as std;

/***************************************************************************************************
 * Exported error codes
 **************************************************************************************************/

/// Numerical error codes shared between the C and Rust TPS Client API surfaces (spec Table 4-2).
pub mod c_errors {
    /// The operation was successful.
    pub const SUCCESS: u32 = 0x00000000;

    /// Error with non-specific cause.
    pub const ERROR_GENERIC: u32 = 0xF0090000;

    /// Error due to insufficient access privileges.
    pub const ERROR_ACCESS_DENIED: u32 = 0xF0090001;

    /// The operation was cancelled.
    pub const ERROR_CANCEL: u32 = 0xF0090002;

    /// Error due to incorrectly formatted input data.
    pub const ERROR_BAD_FORMAT: u32 = 0xF0090003;

    /// The requested operation is specified and should exist, but is not yet implemented.
    pub const ERROR_NOT_IMPLEMENTED: u32 = 0xF0090004;

    /// The requested operation is valid, but not supported by this implementation.
    pub const ERROR_NOT_SUPPORTED: u32 = 0xF0090005;

    /// The requested operation failed because expected data was missing.
    pub const ERROR_NO_DATA: u32 = 0xF0090006;

    /// The requested operation failed because the system ran out of memory resources.
    pub const ERROR_OUT_OF_MEMORY: u32 = 0xF0090007;

    /// The requested operation failed because the system was busy.
    pub const ERROR_BUSY: u32 = 0xF0090008;

    /// The requested operation failed due to a communication error with the service implementation.
    pub const ERROR_COMMUNICATION: u32 = 0xF0090009;

    /// A security fault was detected. The integrity of the returned value cannot be guaranteed.
    pub const ERROR_SECURITY: u32 = 0xF009000A;

    /// The supplied buffer is too small to contain the requested data.
    pub const ERROR_SHORT_BUFFER: u32 = 0xF009000B;

    /// The called API is deprecated. Caller can assume that the returned result is valid and correct.
    pub const ERROR_DEPRECATED: u32 = 0xF009000C;

    /// The supplied UUID is not recognised for the requested usage
    pub const ERROR_BAD_IDENTIFIER: u32 = 0xF009000D;

    /// A NULL pointer was passed by the caller where a valid pointer is required
    pub const ERROR_NULL_POINTER: u32 = 0xF009000E;

    /// A Function was called when the API was in the wrong state
    pub const ERROR_BAD_STATE: u32 = 0xF009000F;

    /// The requested operation could not complete within the allotted time.
    pub const ERROR_TIMEOUT: u32 = 0xF0090010;

    /// The requested operation failed due to an error in the underlying platform.
    pub const ERROR_PLATFORM: u32 = 0xF0090011;

    /// The requested operation failed due to an unexpected runtime error.
    pub const ERROR_RUNTIME_ERROR: u32 = 0xF0090012;
}

/// Login method and connection-data constants shared between the C and Rust TPS Client API
/// surfaces (spec Table 4-3, §4.3.5).
pub mod c_login {
    /***********************************************************************************************
     * Exported login methods
     **********************************************************************************************/

    /// No login data is provided: client is unauthenticated.
    pub const LOGIN_PUBLIC: u32 = 0x00000000;

    /// The client is authenticated based on the platform user identity (uid on Unix system)
    pub const LOGIN_USER: u32 = 0x00000001;

    /// The client is authenticated based on the platform group identity (gid on Unix system)
    pub const LOGIN_GROUP: u32 = 0x00000002;

    /// The client is authenticated based on the application identity provided by the platform.
    pub const LOGIN_APPLICATION: u32 = 0x00000003;

    /// The client is authenticated based on the platform user identity (uid on Unix system) and the
    /// application identity provided by the platform.
    pub const LOGIN_USER_APPLICATION: u32 = 0x00000004;

    /// The client is authenticated based on the platform group identity (gid on Unix system) and
    /// the application identify provided by the platform.
    pub const LOGIN_GROUP_APPLICATION: u32 = 0x00000005;

    /// No additional data is required for a `TPSC_ConnectionData` structure
    pub const CONNECTIONDATA_NONE: u32 = 0;

    /// The `TPSC_ConnectionData` structure includes a UNix Group ID (gid)
    pub const CONNECTIONDATA_GID: u32 = 1;

    /// This is the last value reserved in the standard for `TPSC_ConnectionData` additional
    /// information.
    pub const CONNECTIONDATA_LAST_ITEM: u32 = 0x7fffffff;
}

/// Standardized `UUID` constants used across the TPS Client API (spec §3.2.3.2.1, Annex A.4.3,
/// Table 3-1).
pub mod c_uuid {
    use super::c_structs::UUID;

    /***********************************************************************************************
     * Standardized UUID values
     **********************************************************************************************/
    // cbindgen doesn't know how to parse these, so they are included manually form cbindgen.toml

    /// The NIL UUID is used where a UUID is required and no value is known.
    /// cbindgen:ignore
    pub const UUID_NIL: UUID = UUID { bytes: [0; 16] };

    /// UUID_NAMESPACE is the UUID used to derive other UUIDs in the TPS Client API namespace
    /// cbindgen:ignore
    pub const UUID_NAMESPACE: UUID = UUID {
        bytes: [
            0x99, 0x13, 0x67, 0x3c, 0x23, 0x3e, 0x42, 0x2c, 0x82, 0x13, 0x1e, 0xc1, 0xf7, 0x49,
            0x36, 0xe8,
        ],
    };

    /// Indicates tps-secure-component-type "GPD-TEE"
    /// cbindgen:ignore
    pub const UUID_SC_TYPE_GPD_TEE: UUID = UUID {
        bytes: [
            0x59, 0x84, 0x68, 0x75, 0x1e, 0x02, 0x53, 0xc8, 0x92, 0x2f, 0x5d, 0x60, 0xdd, 0x10,
            0x3a, 0x58,
        ],
    };

    /// Indicates tps-secure-component-type "GPC-SE"
    /// cbindgen:ignore
    pub const UUID_SC_TYPE_GPC_SE: UUID = UUID {
        bytes: [
            0xbd, 0xd6, 0x58, 0xfa, 0x44, 0xc1, 0x5e, 0x59, 0xb3, 0xa1, 0x1a, 0x8f, 0x03, 0x8c,
            0xeb, 0x50,
        ],
    };

    /// Indicates tps-secure-component-type "GPP-REE"
    /// cbindgen:ignore
    pub const UUID_SC_TYPE_GPP_REE: UUID = UUID {
        bytes: [
            0xd2, 0xdc, 0x12, 0x0c, 0x3e, 0x4a, 0x5b, 0x1f, 0xbe, 0xce, 0xdf, 0x38, 0x25, 0xc9,
            0x33, 0xae,
        ],
    };
}

/***************************************************************************************************
 * Exported data Structures
 **************************************************************************************************/

/// `#[repr(C)]` data structures shared between the C and Rust TPS Client API surfaces.
pub mod c_structs {
    use super::c_priv::*;
    use crate::c_uuid::UUID_NIL;
    use std::ffi::c_void;

    /// Connection information used to establish a connection to a Secure Component.
    #[repr(C)]
    #[derive(Clone, Debug)]
    pub enum ConnectionData {
        /// No additional authorization data is provided; the callee ignores the other variants.
        NoConnectionData,
        /// A Unix group ID authorizing the connection (spec §4.4.2, Table 4-1).
        GID(u32),
        /// An implementation-defined value understood by the callee.
        Proprietary(*const c_void),
    }

    /// TPSC_ServiceBound specifies service version bounds. Bounds may be inclusive or exclusive.
    #[repr(C)]
    #[derive(Clone, Debug)]
    pub enum ServiceBound {
        /// The bound version itself is included in the matching range (spec §3.2.3.4.4).
        Inclusive(ServiceVersion),
        /// The bound version itself is excluded from the matching range (spec §3.2.3.4.4).
        Exclusive(ServiceVersion),
        /// No version bound is specified.
        NoBounds,
    }

    /// TPSC_ServiceIdentifier denotes a TPS Service instance, the logical container identifying a
    /// particular TPS Service implementation on the Platform.
    #[repr(C)]
    #[derive(Clone, Debug)]
    pub struct ServiceIdentifier {
        /// A TPSC_UUID which uniquely distinguishes a particular TPS Service on a given platform.
        pub service_instance: UUID,
        /// a TPSC_UUID that identifies the service being provided
        pub service_id: UUID,
        /// A TPSC_UUID that identifies the Secure Component type providing a service
        pub secure_component_type: UUID,
        /// A TPSC_UUID that identifies the instance of a Secure Component on the platform that provides
        /// the service
        pub secure_component_instance: UUID,
        /// The version of the service
        pub service_version: ServiceVersion,
    }

    impl Default for ServiceIdentifier {
        fn default() -> Self {
            Self::new()
        }
    }

    impl ServiceIdentifier {
        /// Construct a `ServiceIdentifier` with all fields set to their nil/zero defaults.
        pub const fn new() -> Self {
            ServiceIdentifier {
                service_instance: UUID_NIL,
                service_id: UUID_NIL,
                secure_component_type: UUID_NIL,
                secure_component_instance: UUID_NIL,
                service_version: ServiceVersion {
                    major_version: 0,
                    minor_version: 0,
                    patch_version: 0,
                },
            }
        }
    }

    /// `ServiceRange` allows a caller to specify which versions of a TPS Service are acceptable to
    /// it, defining version constraints that are used to constrain the results of service
    /// discovery.
    #[repr(C)]
    #[derive(Clone, Debug)]
    pub struct ServiceRange {
        /// Specifies the lowest acceptable version of a service implementation to be returned in
        /// service discovery.
        pub lowest_acceptable_version: ServiceBound,
        /// Specifies the lowest version of a service implementation to be excluded from the
        /// results returned by service discovery.
        pub first_excluded_version: ServiceBound,
        /// Specifies the highest version of a service implementation to be excluded from the
        /// results returned by service discovery.
        pub last_excluded_version: ServiceBound,
        /// Specifies the highest acceptable version of a service implementation to be returned in
        /// service discovery.
        pub highest_acceptable_version: ServiceBound,
    }

    /// `ServiceSelector` is used in the ServiceDiscovery API call to constrain the list of returned
    /// services to those of interest to the caller.
    #[repr(C)]
    #[derive(Clone, Debug)]
    pub struct ServiceSelector {
        /// If not `UUID_NIL`, the returned list of services will only include those whose service_id
        /// matches the provided value. If `UUID_NIL`, all service_id values will be returned.
        pub service_id: UUID,
        /// If not `UUID_NIL`, the returned list of services will only include those whose
        /// secure_component_type matches the provided value. If `UUID_NIL`, all secure components
        /// will be returned.
        pub secure_component_type: UUID,
        /// If not `UUID_NIL`, the returned list of services will only include those whose
        /// secure_component_instance matches the provided value. If `UUID_NIL`, all secure component
        /// instances will be returned.
        pub secure_component_instance: UUID,
        /// Indicate the acceptable range of versions to provide the requested service
        pub service_version_range: ServiceRange,
    }

    /// TPSC_ServiceVersion defines the version of a TPS Service following semantic versioning rules.
    #[repr(C)]
    #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
    pub struct ServiceVersion {
        /// The major version of a TPS Service, according to
        /// [Semantic Versioning](https://semver.org/)
        pub major_version: u32,
        /// The minor version of a TPS Service, according to
        /// [Semantic Versioning](https://semver.org/)
        pub minor_version: u32,
        /// The patch version of a TPS Service, according to
        /// [Semantic Versioning](https://semver.org/)
        pub patch_version: u32,
    }

    /// TPSC_Session denotes an active session between a TPS Client and a TPS Service implementation.
    ///
    /// `service_id` is a reference rather than an owned `UUID` so that the C ABI layer
    /// (`tps_client_c`) can still hand this struct across the FFI boundary as a raw pointer sized
    /// the same as `*const UUID` (no ABI change), while the Rust API layer (`tps_client_api`) gets
    /// the pointed-to `UUID`'s lifetime enforced by the borrow checker instead of by caller
    /// discipline alone.
    #[repr(C)]
    #[derive(Clone, Debug)]
    pub struct Session<'a> {
        /// TPS Service being used in this session.
        pub service_id: &'a UUID,

        /// Session ID
        pub session_id: u32,

        /// Internal implementation defined data. The caller must not access this information
        pub imp: SessionPriv,
    }

    /// TPSC_Transaction is a container for TPS Service Request and Response messages.
    ///
    /// # Safety invariant
    ///
    /// STAGE 3 FINDING (tps_client_common #1): `message` and `maxsize` are public and
    /// independently mutable, and this type derives `Clone` (which duplicates the raw pointer
    /// verbatim rather than the memory it points to). Callers must maintain the invariant that
    /// `message` points to a live allocation of at least `maxsize` readable/writable bytes for as
    /// long as this `MessageBuffer` (or any clone of it) exists — [`MessageBuffer::finalize`]'s
    /// `unsafe` write is only sound under that invariant, and nothing in this type enforces it.
    /// In particular:
    /// - Never mutate `message`/`maxsize` independently in a way that breaks this invariant (e.g.
    ///   growing `maxsize` without growing the backing allocation, or pointing `message` at a
    ///   buffer shorter than `maxsize`).
    /// - A `Clone` of a `MessageBuffer` aliases the same backing buffer as the original; calling
    ///   `finalize` on both is a double-write to the same memory, and using either after the
    ///   backing buffer's owner (e.g. the `&mut [u8]` passed to [`MessageBuffer::init`]) is
    ///   dropped or reused is a dangling-pointer write.
    ///
    /// The user has ruled that this invariant is documented rather than enforced structurally
    /// (fields stay `pub`, `Clone` is unchanged), to preserve `#[repr(C)]`/cbindgen compatibility
    /// and the existing public API shape.
    #[repr(C)]
    #[derive(Clone, Debug)]
    pub struct MessageBuffer {
        /// Mutable pointer to the message buffer.
        ///
        /// # Safety
        ///
        /// Must point to a live allocation of at least `maxsize` bytes for as long as this value
        /// (or any clone of it) exists. See the [`MessageBuffer`] type-level safety invariant.
        pub message: *mut u8,
        /// Size of the message
        pub size: usize,
        /// Size of the message buffer.
        ///
        /// # Safety
        ///
        /// Must not exceed the number of bytes actually allocated at `message`. See the
        /// [`MessageBuffer`] type-level safety invariant.
        pub maxsize: usize,

        /// Internal implementation defined data. The caller must not access this information.
        pub imp: MessageBufferPriv,
    }

    impl MessageBuffer {
        /// Initialize a `MessageBuffer` over the given backing buffer. Equivalent to
        /// `TPSC_InitializeTransaction`.
        pub fn init(buffer: &mut [u8]) -> Self {
            MessageBuffer {
                message: buffer.as_mut_ptr(),
                size: 0,
                maxsize: buffer.len(),
                imp: MessageBufferPriv::new(),
            }
        }

        /// Sanitize the backing buffer and reset message size. Equivalent to
        /// `TPSC_FinalizeTransaction`.
        ///
        /// # Safety (caller-maintained invariant, not enforced by this function)
        ///
        /// Writes `maxsize` zero bytes through `message`. Sound only if `message` still points to
        /// a live allocation of at least `maxsize` bytes — see the [`MessageBuffer`] type-level
        /// safety invariant. Calling this after that allocation has been freed, shrunk below
        /// `maxsize`, or on a stale `Clone` sharing the same pointer with another live
        /// `MessageBuffer` that has already been finalized/reused, is undefined behavior.
        pub fn finalize(&mut self) {
            unsafe {
                self.message.write_bytes(0, self.maxsize);
            }
            self.size = 0;
            self.imp.clear_transaction();
        }
    }

    /// TPSC_UUID encapsulates a UUID value
    #[repr(C)]
    #[derive(Clone, Debug, Eq, PartialEq)]
    pub struct UUID {
        /// The 16 raw bytes of the UUID value.
        pub bytes: [u8; 16],
    }
}

/***************************************************************************************************
 * Private internals (not exported by cbindgen)
 **************************************************************************************************/

/// Implementation-opaque state carried alongside the public TPS Client API structures.
///
/// Each `*Priv` type in this module exists so that an implementation has somewhere to hold and
/// pass its own client context information, if it needs to. The fields are implementation-defined
/// and private: callers construct and inspect a `*Priv` value only through the methods each type
/// provides (`new`/`check`/`into_inner`, etc.), never by reading its fields directly. An
/// implementation is free to redefine the contents of these structs as its own needs dictate, as
/// long as the public methods keep their documented behavior (Stage 1 Finding 2 — human ruling).
pub mod c_priv {

    const SERVICE_SPEC_GUARD: u32 = 0xca5caded;
    const SESSION_GUARD: u32 = 0xacce55ed;
    const TRANSACTION_GUARD: u32 = 0x5ca1ab1e;

    /// Opaque, implementation-defined context associated with a `ServiceSpec`.
    ///
    /// Fields are private and never read by outside callers; only this type's own methods may
    /// construct or inspect one.
    #[repr(C)]
    #[derive(Clone, Debug)]
    pub struct ServiceSpecPriv {
        id: u32,
        guard: u32,
    }
    impl ServiceSpecPriv {
        /// Construct a ServiceSpecPriv instance
        pub fn new(id: u32) -> Self {
            ServiceSpecPriv {
                id,
                guard: SERVICE_SPEC_GUARD,
            }
        }

        /// Return true if the guard value is good
        pub fn check(&self) -> bool {
            self.guard == SERVICE_SPEC_GUARD
        }

        /// Return the inner value
        pub fn into_inner(&self) -> u32 {
            self.id
        }
    }

    /// Opaque, implementation-defined context associated with a `Session`.
    ///
    /// Fields are private and never read by outside callers; only this type's own methods may
    /// construct or inspect one.
    #[repr(C)]
    #[derive(Clone, Debug)]
    pub struct SessionPriv {
        connection_id: u32,
        guard: u32,
    }

    impl SessionPriv {
        /// Construct a SessionPriv instance
        pub fn new(connection_id: u32) -> Self {
            SessionPriv {
                connection_id,
                guard: SESSION_GUARD,
            }
        }

        /// Return true if the guard value is good
        pub fn check(&self) -> bool {
            self.guard == SESSION_GUARD
        }

        /// Return the inner value
        pub fn into_inner(&self) -> u32 {
            self.connection_id
        }
    }

    /// Opaque, implementation-defined context associated with a `MessageBuffer`.
    ///
    /// Fields are private and never read by outside callers; only this type's own methods may
    /// construct or inspect one. Unlike `ServiceSpecPriv`/`SessionPriv`, this type also tracks
    /// the in-flight transaction (if any) issued against the buffer, needed to support
    /// cancellation.
    #[repr(C)]
    #[derive(Clone, Debug)]
    pub struct MessageBufferPriv {
        guard: u32,
        /// True if a transaction started by the last `execute_transaction` call on this buffer is
        /// still outstanding. `Option<u32>` has no representation cbindgen can synthesize as a
        /// concrete C type (Stage 1 Finding 1, tps_client_c), so the "is a transaction in flight"
        /// and "what is its id" facts are tracked as two separate, individually C-representable
        /// fields rather than one `Option<u32>`.
        has_transaction: bool,
        /// Identifies the in-flight transaction started by the last `execute_transaction` call on
        /// this buffer. Only meaningful when `has_transaction` is true.
        transaction_id: u32,
        /// The service that the in-flight transaction (if any) was issued against, needed to find
        /// the owning Connector when cancelling.
        ///
        /// # Safety invariant
        ///
        /// STAGE 3 FINDING (tps_client_common #2): stored and handed back verbatim by
        /// [`MessageBufferPriv::set_transaction`]/[`MessageBufferPriv::transaction`] with no
        /// liveness tracking in this crate. Callers must ensure the pointer passed to
        /// `set_transaction` remains valid for as long as a transaction is in flight (i.e. until
        /// the matching `clear_transaction` call) — every consumer that dereferences the value
        /// returned by `transaction()` trusts this invariant without a compiler-enforced lifetime.
        /// Same root cause and same "document, don't restructure" disposition as the
        /// [`MessageBuffer`] type-level safety invariant.
        service_id: *const super::c_structs::UUID,
    }

    impl Default for MessageBufferPriv {
        fn default() -> Self {
            Self::new()
        }
    }

    impl MessageBufferPriv {
        /// Construct a SessionPriv instance
        pub fn new() -> Self {
            MessageBufferPriv {
                guard: TRANSACTION_GUARD,
                has_transaction: false,
                transaction_id: 0,
                service_id: core::ptr::null(),
            }
        }

        /// Check if the guard value is good
        pub fn check(&self) -> bool {
            self.guard == TRANSACTION_GUARD
        }

        /// Record the in-flight transaction started against `service_id`.
        ///
        /// # Safety (caller-maintained invariant, not enforced by this function)
        ///
        /// `service_id` must remain valid until the matching [`MessageBufferPriv::clear_transaction`]
        /// call — see the `service_id` field's type-level safety
        /// invariant. This function does not and cannot check that.
        pub fn set_transaction(&mut self, transaction_id: u32, service_id: *const super::c_structs::UUID) {
            self.has_transaction = true;
            self.transaction_id = transaction_id;
            self.service_id = service_id;
        }

        /// Clear any recorded in-flight transaction.
        pub fn clear_transaction(&mut self) {
            self.has_transaction = false;
            self.transaction_id = 0;
            self.service_id = core::ptr::null();
        }

        /// Return the in-flight transaction id and the service it was issued against, if any.
        ///
        /// # Safety (caller-maintained invariant, not enforced by this function)
        ///
        /// The returned pointer is only valid if the caller of [`MessageBufferPriv::set_transaction`]
        /// upheld its invariant. Dereferencing it without that guarantee is undefined behavior.
        pub fn transaction(&self) -> Option<(u32, *const super::c_structs::UUID)> {
            if self.has_transaction {
                Some((self.transaction_id, self.service_id))
            } else {
                None
            }
        }
    }
}

/***************************************************************************************************
 * Regression tests
 *
 * These pin the raw values/behaviors that `tps_client_common` defines against the spec sections
 * cited in each test's doc comment, so a future accidental edit (e.g. a transposed hex/byte) is
 * caught here rather than surfacing only as a hard-to-diagnose failure several crates downstream.
 **************************************************************************************************/
#[cfg(test)]
mod tests {
    use super::c_errors::*;
    use super::c_login::*;
    use super::c_priv;
    use super::c_structs;
    use super::c_structs::{ServiceBound, ServiceVersion};
    use super::c_uuid::*;

    /// Pin `c_errors` against the normative Table 4-2 (§4.4.1) values. E2 already ruled that
    /// Table 4-2 (not the informative Annex A.4.1) is authoritative; this only re-confirms the
    /// values already fixed under that ruling, it does not re-litigate it.
    #[test]
    fn c_error_constants_match_table_4_2() {
        assert_eq!(SUCCESS, 0x0000_0000);
        assert_eq!(ERROR_GENERIC, 0xF009_0000);
        assert_eq!(ERROR_ACCESS_DENIED, 0xF009_0001);
        assert_eq!(ERROR_CANCEL, 0xF009_0002);
        assert_eq!(ERROR_BAD_FORMAT, 0xF009_0003);
        // E2 ruling: Table 4-2 (normative) is 0xF0090004, not Annex A.4.1's 0xF0000004.
        assert_eq!(ERROR_NOT_IMPLEMENTED, 0xF009_0004);
        assert_eq!(ERROR_NOT_SUPPORTED, 0xF009_0005);
        assert_eq!(ERROR_NO_DATA, 0xF009_0006);
        assert_eq!(ERROR_OUT_OF_MEMORY, 0xF009_0007);
        assert_eq!(ERROR_BUSY, 0xF009_0008);
        assert_eq!(ERROR_COMMUNICATION, 0xF009_0009);
        assert_eq!(ERROR_SECURITY, 0xF009_000A);
        assert_eq!(ERROR_SHORT_BUFFER, 0xF009_000B);
        assert_eq!(ERROR_DEPRECATED, 0xF009_000C);
        assert_eq!(ERROR_BAD_IDENTIFIER, 0xF009_000D);
        assert_eq!(ERROR_NULL_POINTER, 0xF009_000E);
        assert_eq!(ERROR_BAD_STATE, 0xF009_000F);
        assert_eq!(ERROR_TIMEOUT, 0xF009_0010);
        assert_eq!(ERROR_PLATFORM, 0xF009_0011);
        assert_eq!(ERROR_RUNTIME_ERROR, 0xF009_0012);
    }

    /// Pin `c_login`'s Session Login Method values against normative Table 4-3 (§4.4.2). R2 already
    /// fixed `LOGIN_APPLICATION`/`LOGIN_USER_APPLICATION`/`LOGIN_GROUP_APPLICATION` to 3/4/5 (they
    /// previously collided with USER/GROUP at 1/1/2); this pins that fix so it cannot regress, and
    /// is not a new finding.
    #[test]
    fn c_login_constants_match_table_4_3() {
        assert_eq!(LOGIN_PUBLIC, 0x0000_0000);
        assert_eq!(LOGIN_USER, 0x0000_0001);
        assert_eq!(LOGIN_GROUP, 0x0000_0002);
        assert_eq!(LOGIN_APPLICATION, 0x0000_0003);
        assert_eq!(LOGIN_USER_APPLICATION, 0x0000_0004);
        assert_eq!(LOGIN_GROUP_APPLICATION, 0x0000_0005);
    }

    /// Pin the `TPSC_ConnectionData` tag constants (§4.3.2 / Annex A.4.2 `mod c_login`) — these are
    /// distinct from the Session Login Method values above and share the same module only because
    /// the spec's Annex A.4.2 groups them together.
    #[test]
    fn c_connectiondata_constants_match_spec() {
        assert_eq!(CONNECTIONDATA_NONE, 0);
        assert_eq!(CONNECTIONDATA_GID, 1);
        assert_eq!(CONNECTIONDATA_LAST_ITEM, 0x7fff_ffff);
    }

    /// Pin the standardized UUID constants (Annex A.4.3 `mod c_uuid` / Table 3-1) against their
    /// exact byte arrays as given in the spec.
    ///
    /// `UUID_NAMESPACE` is asserted against the byte-for-byte value from Annex A.4.3's Rust listing,
    /// which is internally consistent with §3.2.3.2.1's string form `9913673c-233e-422c-8213-
    /// 1ec1f74936e8` (Stage 1 Finding 1, fixed — byte 5 was `0x32`, corrected to `0x3e`).
    #[test]
    fn uuid_namespace_matches_spec_3_2_3_2_1_and_annex_a_4_3() {
        assert_eq!(
            UUID_NAMESPACE.bytes,
            [
                0x99, 0x13, 0x67, 0x3c, 0x23, 0x3e, 0x42, 0x2c, 0x82, 0x13, 0x1e, 0xc1, 0xf7, 0x49,
                0x36, 0xe8,
            ],
            "UUID_NAMESPACE must match spec section 3.2.3.2.1 / Annex A.4.3 exactly \
             (9913673c-233e-422c-8213-1ec1f74936e8); see Stage 1 finding"
        );
    }

    /// Pin the Table 3-1 secure-component-type UUIDv5 values, which — unlike `UUID_NAMESPACE` above
    /// — are correct in this crate.
    #[test]
    fn uuid_secure_component_type_constants_match_table_3_1() {
        assert_eq!(
            UUID_SC_TYPE_GPD_TEE.bytes,
            [
                0x59, 0x84, 0x68, 0x75, 0x1e, 0x02, 0x53, 0xc8, 0x92, 0x2f, 0x5d, 0x60, 0xdd, 0x10,
                0x3a, 0x58,
            ]
        );
        assert_eq!(
            UUID_SC_TYPE_GPC_SE.bytes,
            [
                0xbd, 0xd6, 0x58, 0xfa, 0x44, 0xc1, 0x5e, 0x59, 0xb3, 0xa1, 0x1a, 0x8f, 0x03, 0x8c,
                0xeb, 0x50,
            ]
        );
        assert_eq!(
            UUID_SC_TYPE_GPP_REE.bytes,
            [
                0xd2, 0xdc, 0x12, 0x0c, 0x3e, 0x4a, 0x5b, 0x1f, 0xbe, 0xce, 0xdf, 0x38, 0x25, 0xc9,
                0x33, 0xae,
            ]
        );
    }

    /// `UUID_NIL` (used throughout as a "no filter"/"unset" sentinel by `tps_client_api::services`)
    /// must be all-zero per Annex A.4.3.
    #[test]
    fn uuid_nil_is_all_zero() {
        assert_eq!(UUID_NIL.bytes, [0u8; 16]);
    }

    /// `ServiceBound` (renamed from `ServiceBounds` per R3) must structurally match §4.3.5 /
    /// Annex A.3.1: an `Inclusive(ServiceVersion)` / `Exclusive(ServiceVersion)` / `NoBounds` enum.
    /// This is a compile-time-shape test (it exercises construction/pattern-matching of all three
    /// variants) rather than a runtime assertion, since the "bug" R3 fixed was purely nominal.
    #[test]
    fn service_bound_variants_match_spec_4_3_5() {
        let v = ServiceVersion {
            major_version: 1,
            minor_version: 0,
            patch_version: 0,
        };
        let inclusive = ServiceBound::Inclusive(v.clone());
        let exclusive = ServiceBound::Exclusive(v.clone());
        let no_bounds = ServiceBound::NoBounds;

        assert!(matches!(inclusive, ServiceBound::Inclusive(ref mv) if *mv == v));
        assert!(matches!(exclusive, ServiceBound::Exclusive(ref mv) if *mv == v));
        assert!(matches!(no_bounds, ServiceBound::NoBounds));
    }

    /// `ServiceVersion`'s manual `PartialOrd`/`PartialEq` (used by `tps_client_api::services::
    /// version_segment_test`/`matches_version`, see R13) must order lexicographically by
    /// (major, minor, patch) per §3.2.3.4/§4.3.9. This crate only defines the ordering; the
    /// inclusive/exclusive *bound* semantics consumed from this ordering are R13's fix, which lives
    /// in `tps_client_api::services::matches_version`, not here — this test only covers the
    /// ordering primitive this crate is responsible for.
    #[test]
    fn service_version_ordering_is_lexicographic_major_minor_patch() {
        use core::cmp::Ordering;

        let v = |major, minor, patch| ServiceVersion {
            major_version: major,
            minor_version: minor,
            patch_version: patch,
        };

        assert_eq!(v(1, 0, 0).partial_cmp(&v(1, 0, 0)), Some(Ordering::Equal));
        assert_eq!(v(2, 0, 0).partial_cmp(&v(1, 9, 9)), Some(Ordering::Greater));
        assert_eq!(v(1, 0, 0).partial_cmp(&v(2, 0, 0)), Some(Ordering::Less));
        assert_eq!(v(1, 2, 0).partial_cmp(&v(1, 1, 9)), Some(Ordering::Greater));
        assert_eq!(v(1, 1, 0).partial_cmp(&v(1, 2, 0)), Some(Ordering::Less));
        assert_eq!(v(1, 1, 2).partial_cmp(&v(1, 1, 1)), Some(Ordering::Greater));
        assert_eq!(v(1, 1, 1).partial_cmp(&v(1, 1, 2)), Some(Ordering::Less));
        assert_eq!(v(1, 0, 0), v(1, 0, 0));
        assert_ne!(v(1, 0, 0), v(1, 0, 1));
    }

    /// `MessageBuffer::init`/`finalize` (R6's RAII substitute for `TPSC_InitializeTransaction`/
    /// `TPSC_FinalizeTransaction`) must, per §3.3.3.5 (Finalization) and §4.3.3's description of
    /// `size`, expose the full backing buffer as `maxsize` with `size` starting at 0, and `finalize`
    /// must zero the buffer contents and reset `size` back to 0 (sanitization on finalize).
    #[test]
    fn message_buffer_init_sets_maxsize_and_zero_size() {
        let mut backing = [0xAAu8; 16];
        let mb = c_structs::MessageBuffer::init(&mut backing);
        assert_eq!(mb.maxsize, 16);
        assert_eq!(mb.size, 0);
    }

    #[test]
    fn message_buffer_finalize_zeroes_buffer_and_resets_size() {
        let mut backing = [0xAAu8; 8];
        let mut mb = c_structs::MessageBuffer::init(&mut backing);
        mb.size = 8;
        mb.imp.set_transaction(42, core::ptr::null());

        mb.finalize();

        assert_eq!(mb.size, 0);
        assert!(mb.imp.transaction().is_none());
        // SAFETY: `backing` is still alive; `finalize` wrote through `mb.message`, which aliases
        // `backing`'s storage, so re-reading `backing` directly observes the zeroing.
        assert_eq!(backing, [0u8; 8]);
    }

    /// `MessageBufferPriv`'s guard word must start valid (`check() == true`) immediately after
    /// `new()`, and its `set_transaction`/`transaction`/`clear_transaction` round trip must behave
    /// as a plain `Option`, since `tps_client_api::services::cancel_transaction` (R#Phase 3 item)
    /// relies on this to find the connector owning an in-flight transaction.
    #[test]
    fn message_buffer_priv_guard_and_transaction_round_trip() {
        let mut priv_ = c_priv::MessageBufferPriv::new();
        assert!(priv_.check());
        assert!(priv_.transaction().is_none());

        let uuid_ptr = &UUID_NIL as *const c_structs::UUID;
        priv_.set_transaction(7, uuid_ptr);
        let (id, ptr) = priv_.transaction().expect("transaction should be recorded");
        assert_eq!(id, 7);
        assert_eq!(ptr, uuid_ptr);

        priv_.clear_transaction();
        assert!(priv_.transaction().is_none());
        // Guard must remain valid across the round trip - clearing a transaction is not the same
        // as corrupting the structure.
        assert!(priv_.check());
    }

    /// `SessionPriv`'s guard/`connection_id` round trip, exercised the same way as
    /// `MessageBufferPriv` above, since both are the only two `*Priv` guard types this crate
    /// defines and both back §3.3.1/§3.3.2's "not directly constructible by the client" invariant.
    #[test]
    fn session_priv_guard_and_inner_value_round_trip() {
        let priv_ = c_priv::SessionPriv::new(99);
        assert!(priv_.check());
        assert_eq!(priv_.into_inner(), 99);
    }

    /// `ServiceSpecPriv` is defined in this crate (guard-checked the same way as `SessionPriv`/
    /// `MessageBufferPriv`) but, unlike those two, has no consumer anywhere in the workspace outside
    /// its own definition and tests (confirmed by a workspace-wide grep for `ServiceSpecPriv` during
    /// the Stage 1 audit). This is recorded as a Stage 1 finding (unused/dead public API, not a
    /// correctness bug) rather than removed here, since Stage 1 is additive-only. This test simply
    /// pins that the guard mechanism itself behaves consistently with its two siblings, in case a
    /// future consumer starts relying on it.
    #[test]
    fn service_spec_priv_guard_and_inner_value_round_trip() {
        let priv_ = c_priv::ServiceSpecPriv::new(123);
        assert!(priv_.check());
        assert_eq!(priv_.into_inner(), 123);
    }
}
