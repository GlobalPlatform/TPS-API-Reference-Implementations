# ROT13 Connector

A minimal example of TPS Client connector implementation in Rust.

The ROT13 Connector builds as a static (C-linkable) library exporting the
connector API

## License

`rot13_connector` is MIT licensed.

## Usage

The `rot13_connector` crate provides a minimal example of how to implement a
TPS connector. The implementation should work for connectors implemented both
as statically loadable libraries and as dynamically loaded libraries.

The connector API requires exports a single function:

```rust
pub unsafe extern "C" fn TPSC_GetConnectorAPI() -> *const Connector {}
```

Where the `Connector` definition is provided by the `tps_connector` crate.

### lib.rs

The underlying implementation of the top-level module is fairly simple: there
is a static structure, `CONNECTOR` which holds the required C callable APIs
for the connector. There  are sample implementations of each of these
functions in the `c_api` module.

### c_api.rs 

Each of the callable functions in the `c_api` module does some sanitization
of any pointers  provided - this is necessarily limited mainly to checking
for NULL  pointers - and calls the relevant function in the `service` module,
which  is pure safe Rust.

It is quite likely that the functions in the `c_api` module can be re-used
without change in many implementations.

### service.rs

The majority of the sample implementation is in the `service` module.

To adapt this code to some other component, you will need to do at least the
following:

#### Secure Component Instance

Set the Secure Component Instance on which your service is running.

In this sample code I have used a randomly generated UUID. If you are
implementing on a real Secure Component, you should use an appropriate
mechanism to generate a repeatable, but statistically unique UUID for the
Secure  Component Instance. Some Secure Component APIs may already provide
a  suitable mechanism for this.

The TPS Client API Specification suggests some recommended ways to do this,
but the thing that matters is that the value is stable over time and
statistically unique.

#### Determine the Service Instance ID

As with the Secure Component Instance, this is a UUID and is required to
be statistically unique.

#### SERVICES array

During Service Discovery, a connector reports the services available from
the attached Secure Component.

The implementation in this sample is simplified too much to be used in most
real-world deployment scenarios, and is intended purely to show how the API
is expected to work. 

The `SERVICES` array is a reference to an array of `ServiceIdentifier`
instances. Each `ServiceIdentifier` needs to define the following fields:

- `service_instance` a UUID which provides a unique and stable identifier
  for the instance of a service on a Secure Component. This value *must* be
  different for each service on a given secure component. Where there is 
  more than one Secure Component on a device, it *must* be unique across
  devices. This means that you should generate this identity when a service
  is installed or first created on a device.
- `service_id` a UUID which defines what the service does. This value *must*
  be the same for all instances of the same type of service. GlobalPlatform
  specifies UUIDs for every service that it defines, and here we use the
  standard UUID for the ROT13 service.
- `secure_component_type` a UUID defining the type of Secure Component, for
  example TEE, Secure Element, TPM etc., that the service runs on. Bodies
  defining standardized Secure Components such as GlobalPlatform and the
  Trusted Computing Group define these for the components they standardize.
  In this case we use a "test" value, `GPP_TEST_SC_TYPE` which indicates
  that we are using a test component for which no security guarantees are made.
- `service_version`: this is a structure indicating the version of the 
  service API that is supported. For specifications defined by GlobalPlatform,
  this will align with the version of the GlobalPlatform document that defines
  the specification.

## Platform requirements

The implementation does not require the standard library, except when compiled
for test.

This implementation is intended to run on constrained devices (bare-metal or
RTOS-based) which may not have rich synchronization and threading APIs. The
code is thread-safe if the underlying platform provides atomic operations, as
there is a dependency on `AtomicBool` and `AtomicU32`.

The provided implementation generates somewhat unpredictable values for
`session_id` using a random number generator. In general, the use of
unpredictable values of `session_id` has some security benefit, but it may
be acceptable to replace this with something simpler on some platforms. There
is in any case no expectation of cryptographic quality randomness here.

## Known Limitations

This implementation is unlikely to be well-suited to interactions with
complex Secure Components supporting dynamic installation and update of
services.

Currently the implementation of `execute_transaction` does not support
session handling, which prevents use with multi-service cases. 
