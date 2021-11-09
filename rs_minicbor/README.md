# RS-MINICBOR

An implementation of CBOR in Rust which is aimed at relatively constrained embedded
systems where the Serde implementation is not necessarily well suited.

## License

`rs_minicbor` is MIT licensed. See LICENSE.

## API requirements

- Needs to work in a `#[no_std]` environment.
- Needs to handle unaligned data for primitive types.
    - Decode Trait?
- Should support Iterator traits, consistent with small implementation.
  - `fn core::iter::Iterator::next(&mut self) -> Option<Self::Item>`
  - `fn core::iter::IntoIterator::into_iter(self) -> Self::IntoIter`
    - Shared reference to collection as input -> iterator producing shatred references to items
  - `fn iter()` and `fn iter_mut()`.
    - `iter()` also produces a shared reference to items
    - `iter_mut()` produces mutable references to items
    - See [Stackoverflow answer](https://stackoverflow.com/questions/34733811/what-is-the-difference-between-iter-and-into-iter/34745885#34745885).
- Should be able to turn a `tstr` into `&str`, retaining the lifetime of the
  underlying buffer.
- Should be able to nest into arrays and maps to at least a reasonable depth.
- Should support CBOR sequences encoded as `bstr`
- Nice to have: can be driven by CDDL, or by some form of CDDL state machine.
  Reasonable restrictions are allowed.
- Needs to support search in maps
- CBOR arrays will be treated as slices from an API perspective, but note that we
  do not always have same type for each array entry.
  - `fn len(&self) -> usize` returns array length.
  - `fn first(&self) -> Option<&T>` returns first element (None if empty).
  - `fn split_first(&self) -> Option<(&T, &[T])>` splits at first element and rest.
  - `fn last(&self) -> Option<&T>` returns last element.
  - `fn get<I>(&self, index: I) n-> Option<&<I as SliceIndex<[T]>>::Output>`
- CBOR maps will be treated as similar to HashMap from an API perspective. Again
  we do not always have the same type for each map entry.
  - `fn len(&self) -> usize` returns number of elements in map.
  - `fn get<Q: ?Sized>(&self, k: &Q) -> Option<&V>` where Q is the type of keys and
    V is the type of values (which can be any CBOR value). It is allowed to constrain
    keys to integers and `tstr`s.

