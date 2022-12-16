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

extern crate tps_client_common;
extern crate tps_error;

use crate::connector::{connect, disconnect, service_discovery};
use crate::services::VersionTest::InBounds;
use std::ops::DerefMut;

use tps_client_common::c_login::LOGIN_PUBLIC;
use tps_client_common::c_structs::{
    ServiceBounds, ServiceIdentifier, ServiceRange, ServiceSelector, ServiceVersion, UUID,
};
use tps_client_common::c_uuid::UUID_NIL;
use tps_connector::{Connector, TPSC_GetConnectorAPI};
use tps_error::TPSError;

use once_cell::sync::Lazy;
use spin::Mutex;
use state::Storage;

/***************************************************************************************************
 * Debug tracing support under `trace` feature
 **************************************************************************************************/
#[cfg(feature = "trace")]
use func_trace::trace;

#[cfg(feature = "trace")]
func_trace::init_depth_var!();

/***************************************************************************************************
 * Connectors
 **************************************************************************************************/
/// Initialize the connectors available to this TPS Client API instance.
///
/// The implementation used stores connector instances in an `Option<&Connector>` structure in
/// which the [[`Connector`]] reference is obtained by calling [[`TPSC_GetConnectorAPI`]].
///
/// The call to [[`Lazy::new`]] ensures that the code to initialize `CONNECTORS` executes only once
/// (the `Lazy::sync` wraps this initialization in a mutex to ensure execution once on a multi-
/// threaded system).
///
/// # Static linking
///
/// This implementation uses static linking, which means that we can only have a single instance
/// of the symbol `TPSC_GetConnectorAPI`.
///
/// Implementations requiring multiple connectors with static linking will need to arrange for
/// different exported function names to get the connector API.
///
/// # Dynamic linking
///
/// Where dynamic linking is available, it is possible for each connector to export the same
/// `TPSC_GetConnectorAPI name, and for `dlopen()` and `dlsym()`, or their equivalents, to be used
/// to initialize the connector array, `CONNECTORS`
static CONNECTORS: Lazy<&[Option<&Connector>]> = Lazy::new(|| unsafe {
    static mut INSTANCE: [Option<&Connector>; 3] = [None, None, None];

    INSTANCE[0] = TPSC_GetConnectorAPI().as_ref();
    INSTANCE.as_slice()
});

/***************************************************************************************************
 * Services
 **************************************************************************************************/

/// `Service` encapsulates the mapping from [`UUID`] to [`Connector`]
struct Service {
    pub uuid: UUID,
    pub connector: &'static Connector,
}

/// `Services` holds [`Service`] instances.
///
/// It has a fixed size (initially of 10 - this can be increased by modifying the size of the
/// backing array).
///
/// Values are all initialized to `None` because this is the default for the [`Option`] type.
struct Database {
    pub inner: [Option<Service>; 10],
}

// This is the global store for our Services array
static SERVICES: Storage<Mutex<Database>> = Storage::new();

fn init_service() {
    SERVICES.set(Mutex::new(Database {
        inner: [None, None, None, None, None, None, None, None, None, None],
    }));
}

#[cfg_attr(feature = "trace", trace)]
fn add_service(uuid: &UUID, connector: &'static Connector) -> Result<(), TPSError> {
    // Service will be initialized exactly once
    let _ = init_service();

    if find_service(uuid).is_none() {
        let mut services_guard = SERVICES.get().lock();
        let services = services_guard.deref_mut();
        let services_array = services.inner.as_mut();
        for slot in services_array {
            if slot.is_none() {
                *slot = Some(Service {
                    uuid: uuid.clone(),
                    connector,
                });
                return Ok(());
            }
        }
    }
    Err(TPSError::GenericError)
}

#[cfg_attr(feature = "trace", trace)]
pub(crate) fn find_service(uuid: &UUID) -> Option<&'static Connector> {
    let mut services_guard = SERVICES.get().lock();
    let services = services_guard.deref_mut();
    let services_array = services.inner.as_mut();
    for slot in services_array {
        if let Some(svc) = slot {
            if matches_uuid(&svc.uuid, uuid) {
                return Some(svc.connector);
            } else {
                continue;
            }
        }
    }
    None
}

#[cfg_attr(feature = "trace", trace)]
fn remove_service(_uuid: &UUID) -> Result<(), TPSError> {
    Err(TPSError::NotImplemented)
}

//#[cfg_attr(feature = "trace", trace)]
//pub fn get_connector_by_service_name

/// Populate [`service_array`] with the a list of all of the services supported by the connectors.
#[cfg_attr(feature = "trace", trace)]
pub fn populate_services_array(service_array: &mut [ServiceIdentifier]) -> Result<usize, TPSError> {
    let mut service_count: usize = 0;
    let connectors = CONNECTORS.into_iter();
    // Fetch the set of services from all connectors
    for maybe_connector in connectors {
        if let Some(connector_instance) = *maybe_connector {
            // Connect to the connector. Public login should be sufficient
            let conn_id = connect(connector_instance, LOGIN_PUBLIC, None)?;
            // Perform service discovery
            let items_copied =
                service_discovery(connector_instance, &mut service_array[service_count..])?;
            // Add the service instances to the services database
            for svc in service_array[service_count..service_count + items_copied].iter() {
                add_service(&svc.service_instance, connector_instance)?;
            }
            service_count += items_copied;
            // Disconnect once finished
            disconnect(connector_instance, conn_id)?;
        }
    }
    Ok(service_count)
}

/// Populate `selected_services` with the services from `all_services` that match the criteria given
/// in `selector`.
///
/// The selector operates as follows: a service is matched if *all* of the following criteria are
/// true:
///
/// - service_id UUID is matched, or UUID_NIL
/// - secure_component_type is matched, or UUID_NIL
/// - secure_component_instance is matched, or UUID_NIL
/// - service_version_range is matched
#[cfg_attr(feature = "trace", trace)]
pub fn select_matched_services(
    all_services: &[ServiceIdentifier],
    selector: &ServiceSelector,
    selected_services: &mut [ServiceIdentifier],
) -> Result<usize, TPSError> {
    let mut services_copied = 0;
    for service in all_services.iter() {
        if matches_uuid(&service.service_id, &selector.service_id)
            && matches_uuid(
                &service.secure_component_type,
                &selector.secure_component_type,
            )
            && matches_uuid(
                &service.secure_component_instance,
                &selector.secure_component_instance,
            )
            && matches_version(&service.service_version, &selector.service_version_range)
        {
            if services_copied < selected_services.len() {
                selected_services[services_copied] = service.clone();
            }
            services_copied += 1;
        }
    }
    if services_copied < selected_services.len() {
        Ok(services_copied)
    } else {
        Err(TPSError::ShortBuffer(services_copied))
    }
}

/// Determine whether `service_uuid` matches `match_uuid`, or `match_uuid` is `UUID_NIL`, which
/// acts as a wildcard matching any `service_uuid`.
fn matches_uuid(service_uuid: &UUID, match_uuid: &UUID) -> bool {
    if match_uuid == &UUID_NIL {
        true
    } else {
        service_uuid == match_uuid
    }
}

/***************************************************************************************************
 * Version bounds checking
 **************************************************************************************************/

/// Private enum used to assist in version matching
#[derive(PartialEq)]
enum VersionTest {
    InBounds,
    OutOfBounds,
}

/// Private enum used to assist in version matching - defines whether bounds are inclusive or
/// exclusive
enum BoundsType {
    Exclusive,
    Inclusive,
}

/// Private enum used to check whether bounds are above or below a specific type
enum CheckType {
    Above,
    Below,
}

/// Check whether a single segment of `ServiceRange`, defined as a `ServiceVersion` is in or out
/// of bounds.
fn version_segment_test(
    service_ver: &ServiceVersion,
    service_test: &ServiceVersion,
    bounds: BoundsType,
    check_type: CheckType,
) -> VersionTest {
    match (bounds, check_type) {
        (BoundsType::Exclusive, CheckType::Above) => {
            if service_ver > service_test {
                VersionTest::InBounds
            } else {
                VersionTest::OutOfBounds
            }
        }
        (BoundsType::Exclusive, CheckType::Below) => {
            if service_ver < service_test {
                VersionTest::InBounds
            } else {
                VersionTest::OutOfBounds
            }
        }
        (BoundsType::Inclusive, CheckType::Above) => {
            if service_ver >= service_test {
                VersionTest::InBounds
            } else {
                VersionTest::OutOfBounds
            }
        }
        (BoundsType::Inclusive, CheckType::Below) => {
            if service_ver <= service_test {
                VersionTest::InBounds
            } else {
                VersionTest::OutOfBounds
            }
        }
    }
}

/// Checks whether `service_version` matches the specification of `match_range`.
///
/// `service_version` defines the version of a component using semantic versioning. This is tested
/// against the components of `service_range`.
fn matches_version(service_version: &ServiceVersion, match_range: &ServiceRange) -> bool {
    let lowest_ok = match &match_range.lowest_acceptable_version {
        ServiceBounds::Inclusive(mv) => {
            version_segment_test(service_version, mv, BoundsType::Inclusive, CheckType::Above)
        }
        ServiceBounds::Exclusive(mv) => {
            version_segment_test(service_version, mv, BoundsType::Exclusive, CheckType::Above)
        }
        ServiceBounds::NoBounds => InBounds,
    };
    let first_excluded_ok = match &match_range.first_excluded_version {
        ServiceBounds::Inclusive(mv) => {
            version_segment_test(service_version, mv, BoundsType::Inclusive, CheckType::Below)
        }
        ServiceBounds::Exclusive(mv) => {
            version_segment_test(service_version, mv, BoundsType::Exclusive, CheckType::Below)
        }
        ServiceBounds::NoBounds => InBounds,
    };
    let last_excluded_ok = match &match_range.last_excluded_version {
        ServiceBounds::Inclusive(mv) => {
            version_segment_test(service_version, mv, BoundsType::Inclusive, CheckType::Above)
        }
        ServiceBounds::Exclusive(mv) => {
            version_segment_test(service_version, mv, BoundsType::Exclusive, CheckType::Above)
        }
        ServiceBounds::NoBounds => InBounds,
    };
    let highest_ok = match &match_range.highest_acceptable_version {
        ServiceBounds::Inclusive(mv) => {
            version_segment_test(service_version, mv, BoundsType::Inclusive, CheckType::Below)
        }
        ServiceBounds::Exclusive(mv) => {
            version_segment_test(service_version, mv, BoundsType::Exclusive, CheckType::Below)
        }
        ServiceBounds::NoBounds => InBounds,
    };
    lowest_ok == InBounds
        && first_excluded_ok == InBounds
        && last_excluded_ok == InBounds
        && highest_ok == InBounds
}
