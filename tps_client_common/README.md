# tps_client_common

This crate contains definitions that are used in multiple crates that make up
the TPS Client API implementation (`tps_client_api`, `tps_client_c`,
`tps_connector`), per the GPP TPS Client API Specification v1.0.

Values are (and must be) defined such that they can be exported to users of
the C language API via `cbindgen`, so every public type is `#[repr(C)]` and
every module mirrors a `TPSC_*`-prefixed section of the spec:

- `c_errors` — the `TPSC_ERROR_*` numerical return codes (spec Table 4-2).
- `c_login` — Session Login Method and `TPSC_ConnectionData` tag constants
  (spec Table 4-3, §4.3.5).
- `c_uuid` — standardized `UUID` constants (spec §3.2.3.2.1, Annex A.4.3).
- `c_structs` — the shared `#[repr(C)]` structures themselves, including
  `ConnectionData`, `ServiceIdentifier`, `ServiceRange`, `ServiceVersion`,
  `Session<'a>`, `MessageBuffer` and `UUID`.

`Session<'a>`'s `service_id` field is a borrowed `&'a UUID` rather than an
owned value, so that `tps_client_c`'s C ABI layer can hand the struct across
the FFI boundary as a raw pointer (no ABI change), while `tps_client_api`'s
Rust layer gets the pointed-to `UUID`'s lifetime enforced by the borrow
checker instead of by caller discipline alone.

`c_priv` holds the private, implementation-opaque `*Priv` companion types
(`ServiceSpecPriv`, `SessionPriv`, `MessageBufferPriv`) referenced by the
public structures' `imp` fields; these are not exported by `cbindgen` and are
never read directly by callers.

## License

`tps_client_common` is MIT licensed.