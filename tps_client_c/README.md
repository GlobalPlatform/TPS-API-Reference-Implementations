# tps_client_c

A reference implementation of the C language API for the GlobalPlatform
Trusted Platform Services TPS Client API.

The crate generates a C-linkable static library and includes a (very minimal) 
example of calling the TPS Client API from C.

## License

The `tps_client_c` crate is MIT licensed.

## Example

There is an extremely minimal example of calling `tps_client_c` from a C
program in the c_example directory. This example does depend on the C
standard library, even though the Rust components have no requirement for
a standard library.

## Operation

The crate exports C language wrappers over exch of the exported functions
in the `tps_client_api` crate, with some basic sanitization of the inputs
(checking for NULL pointers and the like).

## Limitations

This code should be substantially complete and usable. It has not (yet)
been extensively statically analyzed, and as most of the code is necessarily 
unsafe, this is a priority for a future version.