/***************************************************************************************************
 * Copyright (c) 2021-2022, Qualcomm Innovation Center, Inc. All rights reserved.
 *
 * Permission is hereby granted, free of charge, to any person obtaining a copy of this software
 * and associated documentation files (the “Software”), to deal in the Software without
 * restriction, including without limitation the rights to use, copy, modify, merge, publish,
 * distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the
 * Software is furnished to do so, subject to the following conditions:
 *
 * The above copyright notice and this permission notice (including the next
 * paragraph) shall be included in all copies or substantial portions of the
 * Software.
 *
 * THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING
 * BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
 * NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM,
 * DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
 * OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
 **************************************************************************************************/
/***************************************************************************************************
 * tps_client_common
 *
 * Definitions (constants and structures) shared between the C language interface to the TPS Client
 * API and the Rust implementation. These are split into a separate crate (which contains
 * essentially no code) to avoid creating circular dependencies.
 *
 **************************************************************************************************/
/***************************************************************************************************
 * Exported error codes
 **************************************************************************************************/

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
    pub const ERROR_NOT_IMPLEMENTED: u32 = 0xF0000004;

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
}

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
    pub const LOGIN_APPLICATION: u32 = 0x00000001;

    /// The client is authenticated based on the platform user identity (uid on Unix system) and the
    /// application identity provided by the platform.
    pub const LOGIN_USER_APPLICATION: u32 = 0x00000001;

    /// The client is authenticated based on the platform group identity (gid on Unix system) and
    /// the application identify provided by the platform.
    pub const LOGIN_GROUP_APPLICATION: u32 = 0x00000002;

    /// No additional data is required for a `TPSC_ConnectionData` structure
    pub const CONNECTIONDATA_NONE: u32 = 0;

    /// The `TPSC_ConnectionData` structure includes a UNix Group ID (gid)
    pub const CONNECTIONDATA_GID: u32 = 1;

    /// This is the last value reserved in the standard for `TPSC_ConnectionData` additional
    /// information.
    pub const CONNECTIONDATA_LAST_ITEM: u32 = 0x7fffffff;
}

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
            0x99, 0x13, 0x67, 0x3c, 0x23, 0x32, 0x42, 0x2c, 0x82, 0x13, 0x1e, 0xc1, 0xf7, 0x49,
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

pub mod c_structs {
    use super::c_priv::*;
    use crate::c_uuid::UUID_NIL;
    use std::cmp::Ordering;
    use std::os::raw::c_void;

    /// Connection information used to establish a connection to a Secure Component.
    #[repr(C)]
    #[derive(Clone, Debug)]
    pub enum ConnectionData {
        None,
        GID(u32),
        Proprietary(*const c_void),
    }

    /// TPSC_ServiceBounds specifies service version bounds. Bounds may be inclusive or exclusive.
    #[repr(C)]
    #[derive(Clone, Debug)]
    pub enum ServiceBounds {
        Inclusive(ServiceVersion),
        Exclusive(ServiceVersion),
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

    impl ServiceIdentifier {
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
        pub lowest_acceptable_version: ServiceBounds,
        /// Specifies the lowest version of a service implementation to be excluded from the
        /// results returned by service discovery.
        pub first_excluded_version: ServiceBounds,
        /// Specifies the highest version of a service implementation to be excluded from the
        /// results returned by service discovery.
        pub last_excluded_version: ServiceBounds,
        /// Specifies the highest acceptable version of a service implementation to be returned in
        /// service discovery.
        pub highest_acceptable_version: ServiceBounds,
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
    #[derive(Clone, Debug, Eq, Ord)]
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

    // Used in `version_segment_test`
    impl PartialEq for ServiceVersion {
        fn eq(&self, other: &Self) -> bool {
            self.major_version == other.major_version
                && self.minor_version == other.minor_version
                && self.patch_version == other.patch_version
        }
    }

    // User in `version_segment_test`
    impl PartialOrd for ServiceVersion {
        fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
            match (
                self.major_version.cmp(&other.major_version),
                self.minor_version.cmp(&other.minor_version),
                self.patch_version.cmp(&other.patch_version),
            ) {
                (Ordering::Greater, _, _) => Some(Ordering::Greater),
                (Ordering::Less, _, _) => Some(Ordering::Less),
                (Ordering::Equal, Ordering::Greater, _) => Some(Ordering::Greater),
                (Ordering::Equal, Ordering::Less, _) => Some(Ordering::Less),
                (Ordering::Equal, Ordering::Equal, Ordering::Greater) => Some(Ordering::Greater),
                (Ordering::Equal, Ordering::Equal, Ordering::Less) => Some(Ordering::Less),
                (_, _, _) => Some(Ordering::Equal),
            }
        }
    }

    /// TPSC_Session denotes an active session between a TPS Client and a TPS Service implementation.
    #[repr(C)]
    #[derive(Clone, Debug)]
    pub struct Session {
        /// TPS Service being used in this session.
        pub service_id: *const UUID,

        /// Session ID
        pub session_id: u32,

        /// Internal implementation defined data. The caller must not access this information
        pub imp: SessionPriv,
    }

    /// TPSC_Transaction is a container for TPS Service Request and Response messages.
    #[repr(C)]
    #[derive(Clone, Debug)]
    pub struct MessageBuffer {
        /// Mutable pointer to the message buffer
        pub message: *mut u8,
        /// Size of the message
        pub size: usize,
        /// Size of the message buffer
        pub maxsize: usize,

        /// Internal implementation defined data. The caller must not access this information.
        pub imp: MessageBufferPriv,
    }

    /// TPSC_UUID encapsulates a UUID value
    #[repr(C)]
    #[derive(Clone, Debug, Eq, PartialEq)]
    pub struct UUID {
        pub bytes: [u8; 16],
    }
}

/***************************************************************************************************
 * Private internals (not exported by cbindgen)
 **************************************************************************************************/

pub mod c_priv {

    const SERVICE_SPEC_GUARD: u32 = 0xca5caded;
    const SESSION_GUARD: u32 = 0xacce55ed;
    const TRANSACTION_GUARD: u32 = 0x5ca1ab1e;

    // ServiceSpecPriv is a set of implementation-dependent fields that must not be read by the
    // caller. Fields are private so that they can only be set and extracted using the defined
    // functions.
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

    // SessionPriv is a set of implementation-dependent fields that must not be read by the
    // caller. Fields are private so that they can only be set and extracted using the defined
    // functions.
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

    // TransactionPriv is a set of implementation-dependent fields that must not be read by the
    // caller. Fields are private so that they can only be set and extracted using the defined
    // functions.
    #[repr(C)]
    #[derive(Clone, Debug)]
    pub struct MessageBufferPriv {
        guard: u32,
    }

    impl MessageBufferPriv {
        /// Construct a SessionPriv instance
        pub fn new() -> Self {
            MessageBufferPriv {
                guard: TRANSACTION_GUARD,
            }
        }

        /// Check if the guard value is good
        pub fn check(&self) -> bool {
            self.guard == TRANSACTION_GUARD
        }
    }
}
