## TPS Client API

## Overview

This crate contains a reference implementation of the GlobalPlatform TPS Client API.
This implementation is intended to be suitable for relatively constrained embedded targets.

The Client API is implemented as a library which can be linked with anything that
exposes a C language FFI (which means virtually anything). It also provides a Rust
API to simplify integration with applications written in Rust.

Communication with Secure Components is provided by _Connectors_ which provide an
abstract service-oriented interface to TPS Services implemented on Secure Components.

The implementations are intended to support `no_std` environments and thus have no
dependency on the Rust standard library. 

The Trusted Platform Services APIs make heavy use of
[CBOR](https://www.rfc-editor.org/info/rfc8949) and related technologies such as 
[COSE](https://www.rfc-editor.org/info/rfc8152) and 
[CDDL](https://www.rfc-editor.org/info/rfc8610), which are standardized by the IETF.

This project is composed of multiple sub-crates:

- `tps_client_api` implements most of the Client API functionality in a Rust crate.
- `tps_client_api_c` creates a statically linkable C library with a C language API
  which allows the TPS Client API to be used anywhere that supports a C language FFI.
  This library includes `tps_client_api`, and is able to connect to libraries that
  export the `tps_connector` API.
- `tps_client_common` provides definitions that are used across multiple crates in
  a system. It has no dependencies.
- `tps_error` contains error definitions for any crate using the TPS API ecosystem.
- `tps_connector` defines an API which enables security component back-ends to be
  called from `tps_client_api`. Both Rust and C language APIs are provided, 
  allowing Connector instances to be written in either language.
- `rot13_connector` provides a minimal implementation of the TPS Connector API
- `rot13_service` provides a minimal implementation of a TPS Service (in this case, ROT13
  "encryption).
- `rs_minicbor` is an implementation of [IETF CBOR (RFC8949)](https://www.rfc-editor.org/rfc/rfc8949.html)
  encoding and decoding, with no requirement for an allocator or the standard
  library.

## License

All parts of the TPS Client API are MIT licensed, See LICENSE.

## Building

> The current version of the project is built using CMake. This is likely to change
> in a future version to a build system based entirely on Cargo.

The build system is in a state of flux, and doesn't manage generated header files as
it should. The following instructions should enable you to build and run the example
and tests.

### Build dependencies

You will need to have the following installed:

- Rust toolchain (tested for Rust v1.64.0)
- [cbindgen](https://github.com/eqrion/cbindgen)
  - `cargo install --force cbindgen` is easiest way to install.
- CMake (tested for version 3.20 - this is set as a lower bound in the scripts, so
  older versions will not work)
  - Install from [CMake downloads](https://cmake.org/download/) or your package manager.
- [CMakeRust](https://github.com/Devolutions/CMakeRust)
  - Instructions to use CMakeRust can be found on
    [Devolutions blog](https://blog.devolutions.net/2018/06/insider-series-using-rust-code-in-a-cc-project-with-cmake/)

#### Using CMakeRust in the build system

Create a directory in the top of the repo for CMakeRust:

`cd <path_to_tps_repository>`
`mkdir cmake_rust`

Copy the contents of the cmake directory in the CMakeRust repository into the `cmake_rust` directory. I have
the following files:

- `CargoLink.cmake`
- `CMakeCargo.cmake`
- `CMakeDetermineRustCOmpiler.cmake`
- `CMakeRustCompiler.cmake.in`
- `CMakeRustInformation.cmake`
- `CMakeTestRustCompiler.cmake`
- `FindRust.cmake`
- Various license files.

Credit to the team at [Devolutions](https://devolutions.net) for CMakeRust, which is dual MIT/Apache2 licensed. 

### Building the example

This assumes that you are in the top directory of the repository.

`cd <path_to_tps_repository`

Next, create a directory for CMake artifacts

`mkdir cmake-build-debug`

Now you need to build everything with Cartgo, as this generates headers.

`cargo build`

This takes 30 seconds or so on my moderately powerful laptop with a decent internet connection.

```
cd cmake-build-debug
cmake ..
cd ..
cmake --build cmake-build-debug
```

### Running the Example

