# tps_error

The shared Rust error type for the TPS Client API implementation, per the GPP TPS Client API
Specification v1.0. `TPSError` is what `tps_client_api`, `tps_client_c`, `tps_connector` and
`rot13_connector` return, and this crate owns the mapping between it and the numerical
`TPSC_ERROR_*` codes that cross the C ABI.

`tps_error` is `no_std` by default, with an opt-in `std` feature for host-side testing and
tooling. Its only dependencies are `thiserror` (in its `no_std` configuration) and
`tps_client_common`, which supplies the error constants.

## What is provided

- `TPSError` — one variant for each API return code in spec Table 4-2 ("API Return Code
  Constants"), each carrying a human-readable message via `thiserror`. All variants are unit
  variants except `ShortBuffer(usize)`, which carries the buffer size the caller needs to
  supply.
- `impl From<TPSError> for u32` — converts a `TPSError` to the numerical code used over the C
  API. Note that `ShortBuffer`'s payload is *not* represented in the result: the required size
  travels over the C API in a separate out-parameter (e.g. `recv_len`, `num_services`), not in
  the `TPSC_Result`, so a caller converting an error for return across the FFI boundary must
  propagate the size itself.
- `from_c_error_code(code, buf_size) -> Result<(), TPSError>` — converts a numerical code back
  into a `TPSError`.

## Why conversion from C is a function rather than `From`/`TryFrom`

Two requirements pushed against the standard conversion traits:

- `TPSC_SUCCESS` is not an error, so the natural return type is `Result<(), TPSError>` rather
  than a `TPSError`.
- `ERROR_SHORT_BUFFER` must carry a size, which is not available from the code alone, so the
  conversion needs a second argument.

The resulting signature takes `buf_size: Option<usize>`. Two cases are worth knowing about:

- `ERROR_SHORT_BUFFER` with `buf_size == None` yields `TPSError::RuntimeError`, not
  `ShortBuffer`. Spec §3.3.4 requires an implementation to supply a buffer size alongside that
  code, so a caller omitting it is non-compliant and this is treated as a fatal condition
  rather than a recoverable one.
- A code outside Table 4-2 — including the implementation-defined `0xF0000001`–`0xF000FFFE`
  band and unassigned values — maps to `TPSError::GenericError` rather than panicking.

## Tests

The crate's tests pin every numeric value against Table 4-2 and exhaustively check both
conversion directions. The exhaustive checks use `match` without a wildcard arm, so adding a
variant to `TPSError` without updating the conversions produces a compile error rather than a
silent coverage gap.

Where Table 4-2 and the informative Annex A.4.1 disagree — the Annex lists `0xF0000004` for
`TPSC_ERROR_NOT_IMPLEMENTED` where Table 4-2 has `0xF0090004` — the tests follow Table 4-2,
which is the normative source.

## License

`tps_error` is MIT licensed.
