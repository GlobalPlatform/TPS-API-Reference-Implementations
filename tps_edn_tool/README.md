# tps_edn_tool

Provides `edn-tool`, a command line converter between CBOR extended diagnostic notation (EDN,
[RFC 8949 §8](https://www.rfc-editor.org/rfc/rfc8949.html#section-8)) and binary CBOR.

EDN is the textual rendering of a CBOR data item — `{1: "a", 2: [1, 2, 3]}` rather than nine
opaque bytes — so this tool is the usual way to inspect a CBOR artifact by eye, or to write one
by hand without hand-assembling the encoding.

The conversion itself is entirely `tps_minicbor`'s (`parse_edn` and
`Decoder::write_diagnostic_all_fmt`, built with its `edn`, `diagnostic` and `float` features);
this crate is a thin CLI over them.

## What is provided

- Binary
  - `edn-tool`, with two subcommands:
    - `encode [<file>]` — EDN text in, binary CBOR out.
    - `decode [<file>]` — binary CBOR in, EDN text out.
  - For both, `<file>` defaults to stdin when omitted, and output goes to stdout unless
    `-o`/`--output <file>` is given. Errors are reported on stderr with a non-zero exit status.

This crate has no library target.

## Usage

```bash
# Hand-write a CBOR data item
printf '{1: "a", 2: [1, 2, 3]}' > item.edn
edn-tool encode item.edn -o item.cbor

# Inspect an existing CBOR file
edn-tool decode item.cbor
# {1:"a",2:[1,2,3]}

# Both ends default to stdin/stdout, so the tool composes in a pipeline
printf '[1, 2, 3]' | edn-tool encode | edn-tool decode
# [1,2,3]
```

Note that `encode` and `decode` do not round-trip the *text* exactly: `decode` emits a
canonical rendering, so insignificant whitespace in the input is not preserved. The CBOR bytes
do round-trip.

## License

`tps_edn_tool` is MIT licensed.
