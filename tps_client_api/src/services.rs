/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/

extern crate tps_client_common;
extern crate tps_error;

use crate::connector::{connect, disconnect, discover_services};
use crate::services::VersionTest::InBounds;
use crate::sync::{Mutex, OnceCell};

use tps_client_common::c_login::LOGIN_PUBLIC;
use tps_client_common::c_structs::{
    ServiceBound, ServiceIdentifier, ServiceRange, ServiceSelector, ServiceVersion, UUID,
};
use tps_client_common::c_uuid::UUID_NIL;
use tps_connector::{Connector, TPSC_GetConnectorAPI};
use tps_error::TPSError;

/***************************************************************************************************
 * Debug tracing support under `trace` feature
 **************************************************************************************************/
#[cfg(feature = "trace")]
use func_trace::trace;
#[cfg(feature = "trace")]
use std::println;

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
/// [`OnceCell::get_or_init`] ensures that the code to initialize `CONNECTORS` executes only once,
/// under a `critical_section`, regardless of how many times [`connectors`] is called.
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
static CONNECTORS: OnceCell<[Option<&'static Connector>; 3]> = OnceCell::new();

fn connectors() -> &'static [Option<&'static Connector>] {
    CONNECTORS
        .get_or_init(|| {
            let mut instance: [Option<&'static Connector>; 3] = [None, None, None];
            instance[0] = unsafe { TPSC_GetConnectorAPI().as_ref() };
            instance
        })
        .as_slice()
}

/***************************************************************************************************
 * Services
 **************************************************************************************************/

/// `Service` encapsulates the mapping from [`UUID`] to [`Connector`]
struct Service {
    /// The service instance UUID that identifies this service (spec §3.2.3.2.1).
    pub uuid: UUID,
    /// The connector through which this service was discovered and is reachable.
    pub connector: &'static Connector,
}

/// `Services` holds [`Service`] instances.
///
/// It has a fixed size (initially of 10 - this can be increased by modifying the size of the
/// backing array).
///
/// Values are all initialized to `None` because this is the default for the [`Option`] type.
struct Database {
    /// The fixed-size backing array of discovered services; unused slots are `None`.
    pub inner: [Option<Service>; 10],
}

// This is the global store for our Services array. `Database` is trivially const-constructible,
// so unlike the old `state::Storage` (lazily `.set()` on first use) this can just be a `Mutex`
// initialized directly - no separate init step is needed.
static SERVICES: Mutex<Database> = Mutex::new(Database {
    inner: [None, None, None, None, None, None, None, None, None, None],
});

#[cfg_attr(feature = "trace", trace)]
fn add_service(uuid: &UUID, connector: &'static Connector) -> Result<(), TPSError> {
    // Discovery can run more than once (e.g. repeated `service_discovery` calls), and will
    // re-report services already known to this database - that is not an error.
    if find_service(uuid).is_some() {
        return Ok(());
    }

    SERVICES.lock(|services| {
        for slot in services.inner.as_mut() {
            if slot.is_none() {
                *slot = Some(Service {
                    uuid: uuid.clone(),
                    connector,
                });
                return Ok(());
            }
        }
        Err(TPSError::GenericError)
    })
}

/// Look up the [`Connector`] registered for `uuid` in `SERVICES`, populated by a prior
/// [`populate_services_array`] call. Returns `None` if no service with a matching UUID has been
/// discovered yet.
#[cfg_attr(feature = "trace", trace)]
pub(crate) fn find_service(uuid: &UUID) -> Option<&'static Connector> {
    SERVICES.lock(|services| {
        for svc in services.inner.as_mut().iter_mut().flatten() {
            if matches_uuid(&svc.uuid, uuid) {
                return Some(svc.connector);
            }
        }
        None
    })
}

/// Populate [`service_array`] with a list of all of the services supported by the connectors.
#[cfg_attr(feature = "trace", trace)]
pub fn populate_services_array(service_array: &mut [ServiceIdentifier]) -> Result<usize, TPSError> {
    let mut service_count: usize = 0;
    // Fetch the set of services from all connectors
    for maybe_connector in connectors() {
        if let Some(connector_instance) = *maybe_connector {
            // Connect to the connector. Public login should be sufficient
            let conn_id = connect(connector_instance, LOGIN_PUBLIC, None)?;
            // Perform service discovery
            let items_copied = discover_services(
                connector_instance,
                conn_id,
                &mut service_array[service_count..],
            )?;
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
    if services_copied <= selected_services.len() {
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
        ServiceBound::Inclusive(mv) => {
            version_segment_test(service_version, mv, BoundsType::Inclusive, CheckType::Above)
        }
        ServiceBound::Exclusive(mv) => {
            version_segment_test(service_version, mv, BoundsType::Exclusive, CheckType::Above)
        }
        ServiceBound::NoBounds => InBounds,
    };
    // `first_excluded_version`/`last_excluded_version` together bound a single excluded
    // sub-range (spec §3.2.3.4.4): a version is excluded only if it is on the excluded side of
    // *both* bounds simultaneously. `NoBounds` on either side leaves that side of the window
    // unbounded (vacuously satisfied), except that `NoBounds` on *both* sides is explicitly
    // defined by the spec to mean "no version is excluded" rather than "every version is
    // excluded" (which the unbounded-both-sides interpretation would otherwise imply).
    let excluded_by_first = match &match_range.first_excluded_version {
        ServiceBound::Inclusive(mv) => {
            version_segment_test(service_version, mv, BoundsType::Inclusive, CheckType::Above)
                == InBounds
        }
        ServiceBound::Exclusive(mv) => {
            version_segment_test(service_version, mv, BoundsType::Exclusive, CheckType::Above)
                == InBounds
        }
        ServiceBound::NoBounds => true,
    };
    let excluded_by_last = match &match_range.last_excluded_version {
        ServiceBound::Inclusive(mv) => {
            version_segment_test(service_version, mv, BoundsType::Inclusive, CheckType::Below)
                == InBounds
        }
        ServiceBound::Exclusive(mv) => {
            version_segment_test(service_version, mv, BoundsType::Exclusive, CheckType::Below)
                == InBounds
        }
        ServiceBound::NoBounds => true,
    };
    let excluded = if matches!(match_range.first_excluded_version, ServiceBound::NoBounds)
        && matches!(match_range.last_excluded_version, ServiceBound::NoBounds)
    {
        false
    } else {
        excluded_by_first && excluded_by_last
    };
    let highest_ok = match &match_range.highest_acceptable_version {
        ServiceBound::Inclusive(mv) => {
            version_segment_test(service_version, mv, BoundsType::Inclusive, CheckType::Below)
        }
        ServiceBound::Exclusive(mv) => {
            version_segment_test(service_version, mv, BoundsType::Exclusive, CheckType::Below)
        }
        ServiceBound::NoBounds => InBounds,
    };
    lowest_ok == InBounds && !excluded && highest_ok == InBounds
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version(major: u32, minor: u32, patch: u32) -> ServiceVersion {
        ServiceVersion {
            major_version: major,
            minor_version: minor,
            patch_version: patch,
        }
    }

    fn uuid(byte: u8) -> UUID {
        UUID { bytes: [byte; 16] }
    }

    /***********************************************************************************************
     * version_segment_test
     **********************************************************************************************/

    #[test]
    fn version_segment_test_exclusive_above() {
        let bound = version(1, 0, 0);
        assert!(matches!(
            version_segment_test(&version(2, 0, 0), &bound, BoundsType::Exclusive, CheckType::Above),
            VersionTest::InBounds
        ));
        assert!(matches!(
            version_segment_test(&version(1, 0, 0), &bound, BoundsType::Exclusive, CheckType::Above),
            VersionTest::OutOfBounds
        ));
        assert!(matches!(
            version_segment_test(&version(0, 9, 9), &bound, BoundsType::Exclusive, CheckType::Above),
            VersionTest::OutOfBounds
        ));
    }

    #[test]
    fn version_segment_test_exclusive_below() {
        let bound = version(1, 0, 0);
        assert!(matches!(
            version_segment_test(&version(0, 9, 9), &bound, BoundsType::Exclusive, CheckType::Below),
            VersionTest::InBounds
        ));
        assert!(matches!(
            version_segment_test(&version(1, 0, 0), &bound, BoundsType::Exclusive, CheckType::Below),
            VersionTest::OutOfBounds
        ));
        assert!(matches!(
            version_segment_test(&version(1, 0, 1), &bound, BoundsType::Exclusive, CheckType::Below),
            VersionTest::OutOfBounds
        ));
    }

    #[test]
    fn version_segment_test_inclusive_above() {
        let bound = version(1, 0, 0);
        assert!(matches!(
            version_segment_test(&version(1, 0, 0), &bound, BoundsType::Inclusive, CheckType::Above),
            VersionTest::InBounds
        ));
        assert!(matches!(
            version_segment_test(&version(2, 0, 0), &bound, BoundsType::Inclusive, CheckType::Above),
            VersionTest::InBounds
        ));
        assert!(matches!(
            version_segment_test(&version(0, 9, 9), &bound, BoundsType::Inclusive, CheckType::Above),
            VersionTest::OutOfBounds
        ));
    }

    #[test]
    fn version_segment_test_inclusive_below() {
        let bound = version(1, 0, 0);
        assert!(matches!(
            version_segment_test(&version(1, 0, 0), &bound, BoundsType::Inclusive, CheckType::Below),
            VersionTest::InBounds
        ));
        assert!(matches!(
            version_segment_test(&version(0, 9, 9), &bound, BoundsType::Inclusive, CheckType::Below),
            VersionTest::InBounds
        ));
        assert!(matches!(
            version_segment_test(&version(1, 0, 1), &bound, BoundsType::Inclusive, CheckType::Below),
            VersionTest::OutOfBounds
        ));
    }

    /***********************************************************************************************
     * matches_version
     **********************************************************************************************/

    fn no_bounds_range() -> ServiceRange {
        ServiceRange {
            lowest_acceptable_version: ServiceBound::NoBounds,
            first_excluded_version: ServiceBound::NoBounds,
            last_excluded_version: ServiceBound::NoBounds,
            highest_acceptable_version: ServiceBound::NoBounds,
        }
    }

    #[test]
    fn matches_version_no_bounds_matches_anything() {
        let range = no_bounds_range();
        assert!(matches_version(&version(0, 0, 0), &range));
        assert!(matches_version(&version(99, 99, 99), &range));
    }

    #[test]
    fn matches_version_lowest_acceptable_inclusive() {
        let mut range = no_bounds_range();
        range.lowest_acceptable_version = ServiceBound::Inclusive(version(1, 0, 0));
        assert!(matches_version(&version(1, 0, 0), &range));
        assert!(matches_version(&version(2, 0, 0), &range));
        assert!(!matches_version(&version(0, 9, 9), &range));
    }

    #[test]
    fn matches_version_lowest_acceptable_exclusive() {
        let mut range = no_bounds_range();
        range.lowest_acceptable_version = ServiceBound::Exclusive(version(1, 0, 0));
        assert!(!matches_version(&version(1, 0, 0), &range));
        assert!(matches_version(&version(1, 0, 1), &range));
    }

    #[test]
    fn matches_version_highest_acceptable_inclusive() {
        let mut range = no_bounds_range();
        range.highest_acceptable_version = ServiceBound::Inclusive(version(2, 0, 0));
        assert!(matches_version(&version(2, 0, 0), &range));
        assert!(matches_version(&version(1, 0, 0), &range));
        assert!(!matches_version(&version(2, 0, 1), &range));
    }

    #[test]
    fn matches_version_highest_acceptable_exclusive() {
        let mut range = no_bounds_range();
        range.highest_acceptable_version = ServiceBound::Exclusive(version(2, 0, 0));
        assert!(!matches_version(&version(2, 0, 0), &range));
        assert!(matches_version(&version(1, 9, 9), &range));
    }

    #[test]
    fn matches_version_first_excluded_inclusive_excludes_at_and_above() {
        // first_excluded_version, Inclusive, checks Below: in-bounds means strictly below the
        // excluded version, i.e. the excluded version itself and anything above it is rejected.
        let mut range = no_bounds_range();
        range.first_excluded_version = ServiceBound::Inclusive(version(2, 0, 0));
        assert!(matches_version(&version(1, 9, 9), &range));
        assert!(!matches_version(&version(2, 0, 0), &range));
        assert!(!matches_version(&version(2, 0, 1), &range));
    }

    #[test]
    fn matches_version_last_excluded_inclusive_excludes_at_and_below() {
        // last_excluded_version, Inclusive, checks Above: in-bounds means strictly above the
        // excluded version, i.e. the excluded version itself and anything below it is rejected.
        let mut range = no_bounds_range();
        range.last_excluded_version = ServiceBound::Inclusive(version(2, 0, 0));
        assert!(!matches_version(&version(1, 9, 9), &range));
        assert!(!matches_version(&version(2, 0, 0), &range));
        assert!(matches_version(&version(2, 0, 1), &range));
    }

    #[test]
    fn matches_version_realistic_in_range() {
        let mut range = no_bounds_range();
        range.lowest_acceptable_version = ServiceBound::Inclusive(version(1, 0, 0));
        range.highest_acceptable_version = ServiceBound::Exclusive(version(2, 0, 0));
        assert!(matches_version(&version(1, 5, 0), &range));
    }

    #[test]
    fn matches_version_realistic_out_of_range() {
        let mut range = no_bounds_range();
        range.lowest_acceptable_version = ServiceBound::Inclusive(version(1, 0, 0));
        range.highest_acceptable_version = ServiceBound::Exclusive(version(2, 0, 0));
        assert!(!matches_version(&version(2, 0, 0), &range));
        assert!(!matches_version(&version(0, 5, 0), &range));
    }

    /// Fix pin (Stage 1 Finding 1, tps_client_api). Spec §3.2.3.4.4 defines
    /// `first_excluded_version`/`last_excluded_version` together as bounding a *single excluded
    /// sub-range* `[first_excluded_version, last_excluded_version]` (subject to each bound's own
    /// inclusive/exclusive sense) — any version outside that sub-range matches. `matches_version`
    /// computes "is this version inside the excluded window" directly and negates it, rather than
    /// ANDing two independent-constraint checks (the previous, buggy formulation, which rejected
    /// every version once both exclusion fields were set). This reproduces the exact bound values
    /// from the spec's own Annex B (§A.6) worked example (`first_excluded_version =
    /// Inclusive(1,1,1)`, `last_excluded_version = Exclusive(1,2,0)`), which excludes only the
    /// half-open window `[1.1.1, 1.2.0)`.
    #[test]
    fn matches_version_combined_exclusion_window_excludes_only_the_window() {
        let mut range = no_bounds_range();
        range.first_excluded_version = ServiceBound::Inclusive(version(1, 1, 1));
        range.last_excluded_version = ServiceBound::Exclusive(version(1, 2, 0));

        // A version outside the excluded window `[1.1.1, 1.2.0)` matches, regardless of which
        // side of the window it falls on.
        assert!(matches_version(&version(0, 5, 0), &range));
        assert!(matches_version(&version(1, 5, 0), &range));
        assert!(matches_version(&version(2, 0, 0), &range));
        // A version genuinely inside the excluded window is rejected.
        assert!(!matches_version(&version(1, 1, 5), &range));
    }

    /// Exercised via the full four-bound `ServiceRange` shape from spec Annex B's worked example
    /// (§A.6): `lowest_acceptable_version = Inclusive(0,0,1)`,
    /// `first_excluded_version = Inclusive(1,1,1)`, `last_excluded_version = Exclusive(1,2,0)`,
    /// `highest_acceptable_version = Exclusive(2,0,0)`. Per spec this accepts the union
    /// `[0.0.1, 1.1.1) ∪ [1.2.0, 2.0.0)` and rejects everything else.
    #[test]
    fn matches_version_annex_b_example_range_matches_expected_union() {
        let range = ServiceRange {
            lowest_acceptable_version: ServiceBound::Inclusive(version(0, 0, 1)),
            first_excluded_version: ServiceBound::Inclusive(version(1, 1, 1)),
            last_excluded_version: ServiceBound::Exclusive(version(1, 2, 0)),
            highest_acceptable_version: ServiceBound::Exclusive(version(2, 0, 0)),
        };

        // 0.5.0 and 1.5.0 fall outside the excluded window and inside the acceptable bounds, so
        // both match; 1.1.5 is inside the excluded window and is rejected.
        assert!(matches_version(&version(0, 5, 0), &range));
        assert!(matches_version(&version(1, 5, 0), &range));
        assert!(!matches_version(&version(1, 1, 5), &range));
    }

    /***********************************************************************************************
     * matches_uuid
     **********************************************************************************************/

    #[test]
    fn matches_uuid_wildcard_matches_anything() {
        assert!(matches_uuid(&uuid(1), &UUID_NIL));
        assert!(matches_uuid(&UUID_NIL, &UUID_NIL));
    }

    #[test]
    fn matches_uuid_exact_match() {
        assert!(matches_uuid(&uuid(7), &uuid(7)));
    }

    #[test]
    fn matches_uuid_mismatch() {
        assert!(!matches_uuid(&uuid(7), &uuid(8)));
    }
}
