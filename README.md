# TPS API Reference Implementations

This project provides a set of reference implementations for specifications created
by the [GlobalPlatform](https://globalplatform.org) Trusted Platform Services
Committee and its Working Groups.

The Trusted Platform Services Committee aims to create APIs which allow application
developers to use standardized security services hosted on Secure Components such as
a Trusted Execution Environment, a Secure Element or a Trusted Platform Module (TPM).

The Trusted Platform Services APIs make heavy use of
[CBOR](https://www.rfc-editor.org/info/rfc8949) and related technologies such as 
[COSE](https://www.rfc-editor.org/info/rfc8152) and 
[CDDL](https://www.rfc-editor.org/info/rfc8610), which are standardized by the IETF.

The reference implementations on this site are, where appropriate, intended to be
usable on relatively constrained embedded microcontroller platforms (circa 200-500kB RAM/ROM).

As this is a new project, we have preference for contributions in the
[Rust](https://rust-lang.org) programming language.

## License

Contributions *and their dependencies* must be MIT licensed or provided under a
compatible license. Dual-licensed (Apache 2 OR MIT) dependencies, which are
common in the Rust language ecosystem, are fine.

The easiest way to verify license conditions is to install `cargo license` and
run it inside each crate.

## Components

- [**`rs_minicbor`**](rs_minicbor/README.md): An implementation of
  [RFC 8949 Concise Binary Object Representation (CBOR)](https://www.rfc-editor.org/rfc/rfc8949) 
  intended for use on embedded platforms, or other places where the developer requires more
  control over serialization and deserialization than something like SERDE CBOR.
- [**`rs_cddl`**](rs_cddl/README.md): Tooling for working with
  [RFC8610 Concise Data Definition Language](https://www.rfc-editor.org/rfc/rfc8610.txt). This
  tooling is at an early development phase, but does include a complete CDDL parser and AST.