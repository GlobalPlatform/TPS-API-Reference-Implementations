/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
//! CBOR Extended Diagnostic Notation (EDN) parser — RFC 8949 §8.
//!
//! Parses a single top-level EDN value and returns the equivalent binary CBOR.
//!
//! # Supported constructs
//!
//! | EDN | Example |
//! |-----|---------|
//! | Unsigned integer | `42`, `0x1a`, `0b1010`, `0o17` |
//! | Negative integer | `-1`, `-0x1f` |
//! | Float | `1.5`, `1.5e10`, `Infinity`, `-Infinity`, `NaN` |
//! | Text string | `"hello\nworld"` (JSON escapes + `\uXXXX`) |
//! | Byte string (hex) | `h'dead beef'` (whitespace ignored) |
//! | Byte string (base64) | `b64'aGVsbG8='` |
//! | Byte string (raw) | `'hello'` (raw UTF-8 bytes) |
//! | Array | `[1, "a", true]` |
//! | Map | `{1: "a", 2: h'ff'}` |
//! | Tagged value | `1("2013-03-21T20:04:00Z")` |
//! | Simple values | `true`, `false`, `null`, `undefined`, `simple(16)` |
//! | Comments | `/comment, delimited by slashes/` |

use std::string::String;
use std::vec;
use std::vec::Vec;

/// Error returned by [`parse_edn`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EdnError {
    /// Input ended before the value was complete.
    UnexpectedEof,
    /// An unexpected character was found at the given byte offset.
    UnexpectedChar(char, usize),
    /// An invalid escape sequence was found in a text string.
    InvalidEscape,
    /// A hex digit sequence was malformed.
    InvalidHex,
    /// A base64 sequence was malformed.
    InvalidBase64,
    /// An integer literal overflowed CBOR's supported range.
    IntOverflow,
    /// A float literal was encountered but the `float` feature is not enabled.
    FloatDisabled,
    /// Extra input remains after the top-level value.
    TrailingInput,
    /// `simple(N)` named a reserved value (20..=31); RFC 8949 §3.3 forbids
    /// the two-byte encoding for these, so no well-formed CBOR can express them.
    ReservedSimpleValue(u8),
}

impl core::fmt::Display for EdnError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            EdnError::UnexpectedEof => write!(f, "unexpected end of input"),
            EdnError::UnexpectedChar(c, off) => {
                write!(f, "unexpected '{c}' at byte offset {off}")
            }
            EdnError::InvalidEscape => write!(f, "invalid escape sequence in text string"),
            EdnError::InvalidHex => write!(f, "invalid hex sequence in byte string"),
            EdnError::InvalidBase64 => write!(f, "invalid base64 sequence in byte string"),
            EdnError::IntOverflow => write!(f, "integer value out of CBOR range"),
            EdnError::FloatDisabled => {
                write!(f, "float values require the 'float' feature of tps_minicbor")
            }
            EdnError::TrailingInput => write!(f, "extra input after top-level value"),
            EdnError::ReservedSimpleValue(n) => {
                write!(f, "simple({n}) is reserved (RFC 8949 §3.3) and cannot be well-formed CBOR")
            }
        }
    }
}

impl std::error::Error for EdnError {}

// ─── Public entry point ────────────────────────────────────────────────────────

/// Parse a single EDN value and return its CBOR binary encoding.
///
/// Returns [`EdnError::TrailingInput`] if non-whitespace / non-comment input
/// remains after the top-level value.
///
/// # Examples
///
/// ```rust
/// use tps_minicbor::parse_edn;
///
/// // Unsigned integer
/// assert_eq!(parse_edn("42").unwrap(), vec![0x18, 0x2a]);
///
/// // Text string
/// assert_eq!(parse_edn(r#""hi""#).unwrap(), b"\x62hi");
///
/// // Array
/// assert_eq!(parse_edn("[1, 2, 3]").unwrap(), vec![0x83, 0x01, 0x02, 0x03]);
///
/// // Map
/// assert_eq!(parse_edn("{1: 2}").unwrap(), vec![0xa1, 0x01, 0x02]);
/// ```
pub fn parse_edn(source: &str) -> Result<Vec<u8>, EdnError> {
    let mut p = Parser::new(source);
    let out = p.parse_value()?;
    p.skip_ws_comments()?;
    if p.pos < p.src.len() {
        return Err(EdnError::TrailingInput);
    }
    Ok(out)
}

// ─── Parser ───────────────────────────────────────────────────────────────────

struct Parser<'s> {
    src: &'s [u8],
    pos: usize,
}

impl<'s> Parser<'s> {
    fn new(source: &'s str) -> Self {
        Self {
            src: source.as_bytes(),
            pos: 0,
        }
    }

    fn peek(&self) -> Option<u8> {
        self.src.get(self.pos).copied()
    }

    fn peek_at(&self, offset: usize) -> Option<u8> {
        self.src.get(self.pos + offset).copied()
    }

    fn advance(&mut self) {
        if self.pos < self.src.len() {
            self.pos += 1;
        }
    }

    fn expect(&mut self, c: u8) -> Result<(), EdnError> {
        match self.peek() {
            Some(b) if b == c => {
                self.advance();
                Ok(())
            }
            Some(b) => Err(EdnError::UnexpectedChar(b as char, self.pos)),
            None => Err(EdnError::UnexpectedEof),
        }
    }

    /// Skip whitespace and comments per RFC 8610 Appendix G.6: a comment is
    /// any text within and including a pair of slashes ("/").
    fn skip_ws_comments(&mut self) -> Result<(), EdnError> {
        loop {
            match self.peek() {
                Some(b' ' | b'\t' | b'\r' | b'\n') => {
                    self.advance();
                }
                Some(b'/') => {
                    self.advance();
                    loop {
                        match self.peek() {
                            Some(b'/') => {
                                self.advance();
                                break;
                            }
                            Some(_) => self.advance(),
                            None => return Err(EdnError::UnexpectedEof),
                        }
                    }
                }
                _ => break,
            }
        }
        Ok(())
    }

    fn parse_value(&mut self) -> Result<Vec<u8>, EdnError> {
        self.skip_ws_comments()?;
        match self.peek().ok_or(EdnError::UnexpectedEof)? {
            b'"' => self.parse_text_string(),
            b'\'' => self.parse_raw_bstr(),
            b'h' if self.peek_at(1) == Some(b'\'') => self.parse_hex_bstr(),
            b'b' if self.src.get(self.pos..self.pos + 4) == Some(b"b64'") => self.parse_b64_bstr(),
            b'[' => self.parse_array(),
            b'{' => self.parse_map(),
            b't' => self.parse_keyword("true", || Ok(cbor_simple(21))),
            b'f' => self.parse_keyword("false", || Ok(cbor_simple(20))),
            b'n' => self.parse_keyword("null", || Ok(cbor_simple(22))),
            b'u' => self.parse_keyword("undefined", || Ok(cbor_simple(23))),
            b'I' => self.parse_keyword("Infinity", || cbor_float_inf(false)),
            b's' => self.parse_simple_call(),
            b'N' => self.parse_keyword("NaN", cbor_float_nan),
            b'-' if self.peek_at(1) == Some(b'I') => {
                self.advance(); // consume '-'
                self.parse_keyword("Infinity", || cbor_float_inf(true))
            }
            b'-' | b'0'..=b'9' => self.parse_number_or_tag(),
            c => Err(EdnError::UnexpectedChar(c as char, self.pos)),
        }
    }

    // ── Keywords ──────────────────────────────────────────────────────────────

    /// Match the literal keyword `kw`; on success call `encode()` for the result.
    fn parse_keyword<F: FnOnce() -> Result<Vec<u8>, EdnError>>(
        &mut self,
        kw: &str,
        encode: F,
    ) -> Result<Vec<u8>, EdnError> {
        let end = self.pos + kw.len();
        if self.src.get(self.pos..end) == Some(kw.as_bytes()) {
            // Ensure the keyword is not part of a longer identifier
            let after = self.src.get(end).copied();
            if after.is_none_or(|b| !b.is_ascii_alphanumeric() && b != b'_' && b != b'-') {
                self.pos = end;
                return encode();
            }
        }
        let c = self.src[self.pos] as char;
        Err(EdnError::UnexpectedChar(c, self.pos))
    }

    /// Parse `simple(N)` where N is 0–255.
    fn parse_simple_call(&mut self) -> Result<Vec<u8>, EdnError> {
        let tag = b"simple(";
        let end = self.pos + tag.len();
        if self.src.get(self.pos..end) != Some(tag) {
            let c = self.src[self.pos] as char;
            return Err(EdnError::UnexpectedChar(c, self.pos));
        }
        self.pos = end;
        let n = self.parse_uint_decimal()?;
        if n > 255 {
            return Err(EdnError::IntOverflow);
        }
        if (20..=31).contains(&n) {
            return Err(EdnError::ReservedSimpleValue(n as u8));
        }
        self.skip_ws_comments()?;
        self.expect(b')')?;
        Ok(cbor_simple(n as u8))
    }

    // ── Numbers and tags ──────────────────────────────────────────────────────

    /// Parse an integer or float literal; if immediately followed by `(`, treat as a tag.
    fn parse_number_or_tag(&mut self) -> Result<Vec<u8>, EdnError> {
        let number_start = self.pos; // includes '-' if present
        let negative = if self.peek() == Some(b'-') {
            self.advance();
            true
        } else {
            false
        };

        // Determine base from optional prefix
        let base: u32 = match (self.peek(), self.peek_at(1)) {
            (Some(b'0'), Some(b'x' | b'X')) => {
                self.pos += 2;
                16
            }
            (Some(b'0'), Some(b'b' | b'B')) => {
                self.pos += 2;
                2
            }
            (Some(b'0'), Some(b'o' | b'O')) => {
                self.pos += 2;
                8
            }
            _ => 10,
        };

        let digit_start = self.pos;
        while self.peek().is_some_and(|b| is_digit_for_base(b, base)) {
            self.advance();
        }
        if self.pos == digit_start {
            return Err(match self.peek() {
                Some(b) => EdnError::UnexpectedChar(b as char, self.pos),
                None => EdnError::UnexpectedEof,
            });
        }

        // Float detection: only base-10 with '.' or 'e'/'E'
        let is_float = base == 10
            && self
                .peek()
                .is_some_and(|b| b == b'.' || b == b'e' || b == b'E');

        if is_float {
            if self.peek() == Some(b'.') {
                self.advance();
                while self.peek().is_some_and(|b| b.is_ascii_digit()) {
                    self.advance();
                }
            }
            if self.peek().is_some_and(|b| b == b'e' || b == b'E') {
                self.advance();
                if self.peek().is_some_and(|b| b == b'+' || b == b'-') {
                    self.advance();
                }
                while self.peek().is_some_and(|b| b.is_ascii_digit()) {
                    self.advance();
                }
            }
            // Include leading '-' in the token so cbor_float_from_str sees the sign.
            let token = core::str::from_utf8(&self.src[number_start..self.pos]).unwrap();
            return cbor_float_from_str(token);
        }

        let digits = core::str::from_utf8(&self.src[digit_start..self.pos]).unwrap();
        let magnitude = u128::from_str_radix(digits, base).map_err(|_| EdnError::IntOverflow)?;

        let encoded = if negative {
            if magnitude == 0 {
                cbor_uint(0) // -0 == 0
            } else if magnitude > (u64::MAX as u128 + 1) {
                return Err(EdnError::IntOverflow);
            } else {
                cbor_nint((magnitude - 1) as u64)
            }
        } else {
            if magnitude > u64::MAX as u128 {
                return Err(EdnError::IntOverflow);
            }
            cbor_uint(magnitude as u64)
        };

        // A tag has the form `N(value)` — the `(` follows the tag number.
        // Tags may not be negative.
        if !negative && self.peek() == Some(b'(') {
            self.advance(); // consume '('
            self.skip_ws_comments()?;
            let inner = self.parse_value()?;
            self.skip_ws_comments()?;
            self.expect(b')')?;
            let tag_n = magnitude as u64;
            let mut out = cbor_head(6, tag_n);
            out.extend_from_slice(&inner);
            return Ok(out);
        }

        Ok(encoded)
    }

    fn parse_uint_decimal(&mut self) -> Result<u64, EdnError> {
        let start = self.pos;
        while self.peek().is_some_and(|b| b.is_ascii_digit()) {
            self.advance();
        }
        if self.pos == start {
            return Err(EdnError::UnexpectedEof);
        }
        let s = core::str::from_utf8(&self.src[start..self.pos]).unwrap();
        s.parse::<u64>().map_err(|_| EdnError::IntOverflow)
    }

    // ── Text strings ──────────────────────────────────────────────────────────

    fn parse_text_string(&mut self) -> Result<Vec<u8>, EdnError> {
        self.advance(); // opening '"'
        let mut s = String::new();
        loop {
            match self.peek().ok_or(EdnError::UnexpectedEof)? {
                b'"' => {
                    self.advance();
                    break;
                }
                b'\\' => {
                    self.advance();
                    let esc = self.peek().ok_or(EdnError::UnexpectedEof)?;
                    self.advance();
                    match esc {
                        b'"' => s.push('"'),
                        b'\\' => s.push('\\'),
                        b'/' => s.push('/'),
                        b'b' => s.push('\x08'),
                        b'f' => s.push('\x0c'),
                        b'n' => s.push('\n'),
                        b'r' => s.push('\r'),
                        b't' => s.push('\t'),
                        b'u' => {
                            let cp = self.parse_hex4()?;
                            let c = char::from_u32(cp).ok_or(EdnError::InvalidEscape)?;
                            s.push(c);
                        }
                        _ => return Err(EdnError::InvalidEscape),
                    }
                }
                b => {
                    self.advance();
                    if b < 0x80 {
                        s.push(b as char);
                    } else {
                        // Non-ASCII: back up one byte and reparse as UTF-8
                        let start = self.pos - 1;
                        let tail = core::str::from_utf8(&self.src[start..])
                            .map_err(|_| EdnError::InvalidEscape)?;
                        let c = tail.chars().next().ok_or(EdnError::InvalidEscape)?;
                        self.pos = start + c.len_utf8();
                        s.push(c);
                    }
                }
            }
        }
        Ok(cbor_tstr(&s))
    }

    fn parse_hex4(&mut self) -> Result<u32, EdnError> {
        let mut v = 0u32;
        for _ in 0..4 {
            let b = self.peek().ok_or(EdnError::UnexpectedEof)?;
            self.advance();
            let nibble = hex_nibble(b).ok_or(EdnError::InvalidEscape)?;
            v = (v << 4) | nibble as u32;
        }
        Ok(v)
    }

    // ── Byte strings ──────────────────────────────────────────────────────────

    fn parse_hex_bstr(&mut self) -> Result<Vec<u8>, EdnError> {
        self.pos += 2; // 'h' '\''
        let mut bytes = Vec::new();
        loop {
            // Whitespace and comments are ignored inside h'...' (RFC 8610 §G.1, §G.6).
            self.skip_ws_comments()?;
            match self.peek().ok_or(EdnError::UnexpectedEof)? {
                b'\'' => {
                    self.advance();
                    break;
                }
                hi => {
                    self.advance();
                    self.skip_ws_comments()?;
                    let lo = self.peek().ok_or(EdnError::UnexpectedEof)?;
                    self.advance();
                    let h = hex_nibble(hi).ok_or(EdnError::InvalidHex)?;
                    let l = hex_nibble(lo).ok_or(EdnError::InvalidHex)?;
                    bytes.push((h << 4) | l);
                }
            }
        }
        Ok(cbor_bstr(&bytes))
    }

    fn parse_b64_bstr(&mut self) -> Result<Vec<u8>, EdnError> {
        self.pos += 4; // 'b', '6', '4', '\''
        let mut encoded = String::new();
        loop {
            match self.peek().ok_or(EdnError::UnexpectedEof)? {
                b'\'' => {
                    self.advance();
                    break;
                }
                b => {
                    self.advance();
                    if !b.is_ascii_whitespace() {
                        encoded.push(b as char);
                    }
                }
            }
        }
        let bytes = base64_decode(&encoded)?;
        Ok(cbor_bstr(&bytes))
    }

    fn parse_raw_bstr(&mut self) -> Result<Vec<u8>, EdnError> {
        self.advance(); // opening '\''
        let mut bytes = Vec::new();
        loop {
            match self.peek().ok_or(EdnError::UnexpectedEof)? {
                b'\'' => {
                    self.advance();
                    break;
                }
                b => {
                    bytes.push(b);
                    self.advance();
                }
            }
        }
        Ok(cbor_bstr(&bytes))
    }

    // ── Collections ───────────────────────────────────────────────────────────

    fn parse_array(&mut self) -> Result<Vec<u8>, EdnError> {
        self.advance(); // '['
        let mut items: Vec<Vec<u8>> = Vec::new();
        self.skip_ws_comments()?;
        if self.peek() == Some(b']') {
            self.advance();
            return Ok(cbor_head(4, 0)); // empty array
        }
        loop {
            items.push(self.parse_value()?);
            self.skip_ws_comments()?;
            match self.peek().ok_or(EdnError::UnexpectedEof)? {
                b',' => {
                    self.advance();
                    self.skip_ws_comments()?;
                }
                b']' => {
                    self.advance();
                    break;
                }
                c => return Err(EdnError::UnexpectedChar(c as char, self.pos)),
            }
        }
        let mut out = cbor_head(4, items.len() as u64);
        for item in items {
            out.extend_from_slice(&item);
        }
        Ok(out)
    }

    fn parse_map(&mut self) -> Result<Vec<u8>, EdnError> {
        self.advance(); // '{'
        let mut pairs: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
        self.skip_ws_comments()?;
        if self.peek() == Some(b'}') {
            self.advance();
            return Ok(cbor_head(5, 0)); // empty map
        }
        loop {
            let k = self.parse_value()?;
            self.skip_ws_comments()?;
            self.expect(b':')?;
            self.skip_ws_comments()?;
            let v = self.parse_value()?;
            pairs.push((k, v));
            self.skip_ws_comments()?;
            match self.peek().ok_or(EdnError::UnexpectedEof)? {
                b',' => {
                    self.advance();
                    self.skip_ws_comments()?;
                }
                b'}' => {
                    self.advance();
                    break;
                }
                c => return Err(EdnError::UnexpectedChar(c as char, self.pos)),
            }
        }
        let mut out = cbor_head(5, pairs.len() as u64);
        for (k, v) in pairs {
            out.extend_from_slice(&k);
            out.extend_from_slice(&v);
        }
        Ok(out)
    }
}

// ─── CBOR encoding helpers ────────────────────────────────────────────────────

fn cbor_head(major: u8, value: u64) -> Vec<u8> {
    let mt = major << 5;
    if value <= 23 {
        vec![mt | value as u8]
    } else if value <= 0xff {
        vec![mt | 24, value as u8]
    } else if value <= 0xffff {
        vec![mt | 25, (value >> 8) as u8, value as u8]
    } else if value <= 0xffff_ffff {
        vec![
            mt | 26,
            (value >> 24) as u8,
            (value >> 16) as u8,
            (value >> 8) as u8,
            value as u8,
        ]
    } else {
        vec![
            mt | 27,
            (value >> 56) as u8,
            (value >> 48) as u8,
            (value >> 40) as u8,
            (value >> 32) as u8,
            (value >> 24) as u8,
            (value >> 16) as u8,
            (value >> 8) as u8,
            value as u8,
        ]
    }
}

fn cbor_uint(n: u64) -> Vec<u8> {
    cbor_head(0, n)
}

fn cbor_nint(n: u64) -> Vec<u8> {
    cbor_head(1, n)
}

fn cbor_bstr(bytes: &[u8]) -> Vec<u8> {
    let mut out = cbor_head(2, bytes.len() as u64);
    out.extend_from_slice(bytes);
    out
}

fn cbor_tstr(s: &str) -> Vec<u8> {
    let b = s.as_bytes();
    let mut out = cbor_head(3, b.len() as u64);
    out.extend_from_slice(b);
    out
}

fn cbor_simple(n: u8) -> Vec<u8> {
    // major 7 (0xe0): values 0-23 fit in one byte; 24+ require the two-byte form.
    if n <= 23 {
        vec![0xe0 | n]
    } else {
        vec![0xf8, n]
    }
}

fn cbor_float_nan() -> Result<Vec<u8>, EdnError> {
    #[cfg(feature = "float")]
    {
        // Preferred NaN: half-precision 0xf97e00
        Ok(vec![0xf9, 0x7e, 0x00])
    }
    #[cfg(not(feature = "float"))]
    Err(EdnError::FloatDisabled)
}

fn cbor_float_inf(negative: bool) -> Result<Vec<u8>, EdnError> {
    #[cfg(feature = "float")]
    {
        // Half-precision +/-Infinity
        Ok(if negative {
            vec![0xf9, 0xfc, 0x00]
        } else {
            vec![0xf9, 0x7c, 0x00]
        })
    }
    #[cfg(not(feature = "float"))]
    {
        let _ = negative;
        Err(EdnError::FloatDisabled)
    }
}

fn cbor_float_from_str(s: &str) -> Result<Vec<u8>, EdnError> {
    #[cfg(feature = "float")]
    {
        use half::f16;
        let v: f64 = s.parse().map_err(|_| EdnError::UnexpectedChar('?', 0))?;
        // Preferred encoding: smallest width that round-trips exactly
        let v16 = f16::from_f64(v);
        if f64::from(v16) == v {
            let bits = v16.to_bits();
            return Ok(vec![0xf9, (bits >> 8) as u8, bits as u8]);
        }
        let v32 = v as f32;
        if f64::from(v32) == v {
            let bits = v32.to_bits();
            return Ok(vec![
                0xfa,
                (bits >> 24) as u8,
                (bits >> 16) as u8,
                (bits >> 8) as u8,
                bits as u8,
            ]);
        }
        let bits = v.to_bits();
        Ok(vec![
            0xfb,
            (bits >> 56) as u8,
            (bits >> 48) as u8,
            (bits >> 40) as u8,
            (bits >> 32) as u8,
            (bits >> 24) as u8,
            (bits >> 16) as u8,
            (bits >> 8) as u8,
            bits as u8,
        ])
    }
    #[cfg(not(feature = "float"))]
    {
        let _ = s;
        Err(EdnError::FloatDisabled)
    }
}

// ─── Utilities ────────────────────────────────────────────────────────────────

fn is_digit_for_base(b: u8, base: u32) -> bool {
    match base {
        2 => matches!(b, b'0' | b'1'),
        8 => matches!(b, b'0'..=b'7'),
        10 => b.is_ascii_digit(),
        16 => b.is_ascii_hexdigit(),
        _ => false,
    }
}

fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

// ─── Inline base64 decoder (RFC 4648 §4 standard alphabet + padding) ──────────

fn base64_decode(s: &str) -> Result<Vec<u8>, EdnError> {
    const TABLE: [u8; 128] = {
        let mut t = [0xffu8; 128];
        let mut i = 0u8;
        while i < 26 {
            t[(b'A' + i) as usize] = i;
            t[(b'a' + i) as usize] = i + 26;
            i += 1;
        }
        let mut i = 0u8;
        while i < 10 {
            t[(b'0' + i) as usize] = 52 + i;
            i += 1;
        }
        t[b'+' as usize] = 62;
        t[b'/' as usize] = 63;
        t
    };

    let bytes = s.as_bytes();
    // Strip trailing padding '='
    let mut end = bytes.len();
    while end > 0 && bytes[end - 1] == b'=' {
        end -= 1;
    }
    let data = &bytes[..end];

    let mut out = Vec::with_capacity(data.len() * 3 / 4 + 1);
    let mut acc: u32 = 0;
    let mut bits: u32 = 0;

    for &b in data {
        if b >= 128 {
            return Err(EdnError::InvalidBase64);
        }
        let v = TABLE[b as usize];
        if v == 0xff {
            return Err(EdnError::InvalidBase64);
        }
        acc = (acc << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Ok(out)
}
