# tps_client_api

This crate contains a reference implementation for the GlobalPlatform
Trusted Platform Services API.

> **Work In Progress**: This crate is a Work In Progress, and as such is far
> from supporting all of the features intended.

At the time of writing, this crate is suitable for compilation on desktop
class machines to allow an understanding of how the APIs work, and for
development and specification of services.

It is intended to develop into a production quality framework for constrained
devices running bare-metal or under RTOS as well as for more fully-featured
environments supporting allocators and richer libraries.

The present implementation should compile for `no_std` environments.

## License

`tps_client_api` is MIT licensed.

## Basics of operation

The TPS Client API provides a common mechanism for communicating with standard
services which would normally execute on a Secure Component such as a TEE,
Secure Element or TPM. Communication with these services takes the form of
[CBOR (RFC 8949)](https://www.rfc-editor.org/rfc/rfc8949.html) messages.

The TPS Client API exports two normative interfaces. The one defined in
the top module of this crate (the normative Rust API) and a C language API
generated from definitions in the `tps_client_c` crate.

At the top level there are a set of exported functions:

- `cancel_transaction`: cancel an ongoing transaction (currently not implemented)
- `clear_transaction`: not implemented, and likely to be removed.
- `close_session`: close a session
- `execute_transaction`: perform an operation on a session which is connected to
  a service.
- `finalize_transaction`: sanitize the contents of a transaction buffer.
- `initialize_transaction`: Initialize the buffer structures used in message 
  transactions.
- `open_session`: Open a session to a specific service running on a specified
  Secure Component.
- `service_discovery`: Determine which services are available on a given device,
  and what Secure Components they are running on.

## Current limitations

- Not yet working on embedded platforms.
- Current implementation is not optimized for multi-threaded environments.
- Only tested with a single connector instance so far.
- Almost certainly some error conditions not covered correctly. 
