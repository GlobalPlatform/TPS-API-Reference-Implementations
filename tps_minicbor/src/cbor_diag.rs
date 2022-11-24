/***************************************************************************************************
 * Copyright (c) 2021, 2022 Qualcomm Innovation Center, Inc. All rights reserved.
 *
 * Permission is hereby granted, free of charge, to any person obtaining a copy of this software
 * and associated documentation files (the “Software”), to deal in the Software without
 * restriction, including without limitation the rights to use, copy, modify, merge, publish,
 * distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the
 * Software is furnished to do so, subject to the following conditions:
 *
 * The above copyright notice and this permission notice shall be included in all copies or
 * substantial portions of the Software.
 *
 * THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING
 * BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
 * NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM,
 * DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
 * OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
 **************************************************************************************************/
/// # diag - CBOR diagnostic style output for structured binary data
///
/// CBOR diagnostic notation is defined in RFC7049 and extended slightly in RFC8610. While it was
/// expressly designed for CBOR, it is sufficiently expressive that it is appropriate for many
/// TLV-style binary representations.
///
/// ## Format recap
///
/// - <tag> ( <data> ) is used to denote tags.
/// - uint, int, float types are formatted in their natural programming language formats. 0x, 0o
///   and 0b can be used to prefix values shown in hex, octal and binary, respectively.
/// - -Infinity, +Infinity and NaN are written as you might expect.
/// - true, false and null mean what your would expect
/// - Strings are written 'as a string' in single quotes.
/// - Byte strings are written as h'0123456789abcdef'. Optionally b64 can be used to show a Base64
///   or Base64url coded value. It is permissible to separate byte strings with spaces for ease of
///   visibility, so h'0123456789abcdef' is the same as h'01 23 45 67 89 ab cd ef'
/// - Comments are permitted, e.g. h'123456abcd' / Configuration Data /
///
/// ## Requirements
///
/// The design of the diagnostic formatter is intended to be suitable for use within tools as well
/// as simple "dump to stdout" applications. This means that output should be generated in a manner
/// that is closely related to the structure of the AST from which is was generated. This means
/// that:
///
/// - Output needs to provide some level of nesting information
/// - Output should provide comments that decode the data where possible/appropriate. By default
///   this could be simply a dump of the debug format (i.e. implementation of Debug trait, although
///   Display would be even better as it is intended for formatted output.
///
#[cfg(any(feature = "float", test))]
extern crate half;

#[cfg(any(feature = "full", test))]
extern crate chrono;

#[cfg(any(feature = "full", test))]
use std::boxed::Box;

#[cfg(any(feature = "full", test))]
use std::error::Error;

#[cfg(any(feature = "full", test))]
use std::fmt::Debug;

#[cfg(any(feature = "full", test))]
use std::fmt::Display;

#[cfg(any(feature = "full", test))]
use std::io::{Read, Write};

#[cfg(any(feature = "full", test))]
use std::string::String;

#[cfg(any(feature = "float", test))]
use half::f16;

#[cfg(any(feature = "full", test))]
use chrono::{DateTime, FixedOffset};

#[cfg(any(feature = "full", test))]
use crate::decoder::{ArrayBuf, CBORDecoder, DecodeBufIterator, MapBuf, SequenceBuffer, TagBuf};

#[cfg(any(feature = "full", test))]
use crate::types::CBOR;

/// Trait defining helper functions for conveniently displaying information in CBOR
/// diagnostic format.
#[cfg(any(feature = "full", test))]
pub trait Diag {
    fn cbor_diag(&self, outfp: &mut dyn Write) -> Result<(), Box<dyn Error>>;
}

#[cfg(any(feature = "full", test))]
impl<'a> Diag for SequenceBuffer<'a> {
    fn cbor_diag(&self, outfp: &mut dyn Write) -> Result<(), Box<dyn Error>> {
        for item in self.into_iter() {
            item.diag(outfp, 0)?;
        }
        Ok(())
    }
}

#[cfg(any(feature = "full", test))]
impl<'a> Diag for CBOR<'a> {
    fn cbor_diag(&self, outfp: &mut dyn Write) -> Result<(), Box<dyn Error>> {
        self.diag(outfp, 0)?;
        Ok(())
    }
}

#[cfg(any(feature = "full", test))]
impl<'a> Diag for CBORDecoder<'a> {
    fn cbor_diag(&self, outfp: &mut dyn Write) -> Result<(), Box<dyn Error>> {
        let it = self.into_inner().into_iter();
        for item in it {
            item.diag(outfp, 0)?;
        }
        Ok(())
    }
}

/// The DiagFormatter trait should be implemented for any data structure that is intended to be
/// displayed using the CBOR diagnostic format.
#[cfg(any(feature = "full", test))]
pub trait DiagFormatter {
    fn diag(
        &self,
        buf: &mut dyn std::io::Write,
        idt: u32,
    ) -> Result<(), std::io::Error>;
}

// This variant used when std_tags or test features are enabled
#[cfg(any(feature = "full", test))]
impl<'buf> DiagFormatter for CBOR<'buf> {
    fn diag(
        &self,
        buf: &mut dyn std::io::Write,
        idt: u32,
    ) -> Result<(), std::io::Error> {
        match self {
            CBOR::UInt(v) => diag_uint(buf, v, idt),
            CBOR::NInt(v) => diag_nint(buf, v, idt),
            CBOR::Float64(v) => diag_f64(buf, v, idt),
            CBOR::Float32(v) => diag_f32(buf, v, idt),
            CBOR::Float16(v) => diag_f16(buf, v, idt),
            CBOR::Bstr(bs) => diag_bstr(buf, *bs, idt),
            CBOR::Tstr(s) => diag_tstr(buf, *s, idt),
            CBOR::Array(ab) => ab.diag(buf, idt),
            CBOR::Map(mb) => mb.diag(buf, idt),
            CBOR::Tag(tb) => tb.diag(buf, idt),
            CBOR::Simple(v) => diag_uint(buf, &(*v as u64), idt),
            CBOR::False => diag_false(buf, idt),
            CBOR::True => diag_true(buf, idt),
            CBOR::Null => diag_null(buf, idt),
            CBOR::Undefined => diag_undefined(buf, idt),
            CBOR::Eof => Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "EOF reached",
            )),
            CBOR::DateTime(dt) => diag_date_time(buf, dt, idt),
            CBOR::Epoch(ep) => diag_epoch(buf, ep, idt),
        }
    }
}

#[cfg(any(feature = "full", test))]
#[inline]
fn diag_uint(
    buf: &mut dyn std::io::Write,
    v: &u64,
    idt: u32,
) -> Result<(), std::io::Error> {
    write!(buf, "{} {} ", indent(idt), v)
}

#[cfg(any(feature = "full", test))]
#[inline]
fn diag_nint(
    buf: &mut dyn std::io::Write,
    v: &u64,
    idt: u32,
) -> Result<(), std::io::Error> {
    write!(buf, "{} {} ", indent(idt), (-1i128 - (*v as i128)))
}

#[cfg(any(feature = "full", test))]
#[inline]
fn diag_f64(
    buf: &mut dyn std::io::Write,
    v: &f64,
    idt: u32,
) -> Result<(), std::io::Error> {
    write!(buf, "{} {} ", indent(idt), v)
}

#[cfg(any(feature = "full", test))]
#[inline]
fn diag_f32(
    buf: &mut dyn std::io::Write,
    v: &f32,
    idt: u32,
) -> Result<(), std::io::Error> {
    write!(buf, "{} {} ", indent(idt), v)
}

#[cfg(any(feature = "full", test))]
#[inline]
fn diag_f16(
    buf: &mut dyn std::io::Write,
    v: &f16,
    idt: u32,
) -> Result<(), std::io::Error> {
    write!(buf, "{} {} ", indent(idt), v)
}

#[cfg(any(feature = "full", test))]
#[inline]
fn diag_bstr(
    buf: &mut dyn std::io::Write,
    v: &[u8],
    idt: u32,
) -> Result<(), std::io::Error> {
    write!(buf, "{} h\'", indent(idt))?;
    for result in v.bytes() {
        if let Ok(byte) = result {
            write!(buf, "{}", print_hex(byte))?;
        } else {
            return Err(result.unwrap_err());
        }
    }
    write!(buf, "\' ")
}

#[cfg(any(feature = "full", test))]
#[inline]
fn diag_tstr(
    buf: &mut dyn std::io::Write,
    v: &str,
    idt: u32,
) -> Result<(), std::io::Error> {
    write!(buf, "{} \"{}\" ", indent(idt), v)
}

#[cfg(any(feature = "full", test))]
#[inline]
fn diag_false(buf: &mut dyn std::io::Write, idt: u32) -> Result<(), std::io::Error> {
    write!(buf, "{} false ", indent(idt))
}

#[cfg(any(feature = "full", test))]
#[inline]
fn diag_true(buf: &mut dyn std::io::Write, idt: u32) -> Result<(), std::io::Error> {
    write!(buf, "{} true ", indent(idt))
}

#[cfg(any(feature = "full", test))]
#[inline]
fn diag_null(buf: &mut dyn std::io::Write, idt: u32) -> Result<(), std::io::Error> {
    write!(buf, "{} null ", indent(idt))
}

#[cfg(any(feature = "full", test))]
#[inline]
fn diag_undefined(
    buf: &mut dyn std::io::Write,
    idt: u32,
) -> Result<(), std::io::Error> {
    write!(buf, "{} undefined ", indent(idt))
}

#[cfg(any(feature = "full", test))]
#[inline]
fn diag_date_time(
    buf: &mut dyn std::io::Write,
    v: &DateTime<FixedOffset>,
    idt: u32,
) -> Result<(), std::io::Error> {
    write!(
        buf,
        "{} 0 ( \"{}\" ) ",
        indent(idt),
        v.format("%Y-%m-%dT%H:%M:%S%z")
    )
}

#[cfg(any(feature = "full", test))]
#[inline]
fn diag_epoch(
    buf: &mut dyn std::io::Write,
    v: &i64,
    idt: u32,
) -> Result<(), std::io::Error> {
    write!(buf, "{} 1 ( {} ) ", indent(idt), v)
}

/// Implementation of DiagFormatter for ArrayBuf
#[cfg(any(feature = "full", test))]
impl<'buf> DiagFormatter for ArrayBuf<'buf> {
    fn diag(
        &self,
        buf: &mut dyn std::io::Write,
        idt: u32,
    ) -> Result<(), std::io::Error> {
        write!(buf, "{} [\n", indent(idt))?;
        for item in self.into_iter() {
            item.diag(buf, idt + 1)?;
            write!(buf, ", \n")?;
        }
        write!(buf, "{} ],\n", indent(idt))
    }
}

/// Implementation of DiagFormatter for MapBuf
#[cfg(any(feature = "full", test))]
impl<'buf> DiagFormatter for MapBuf<'buf> {
    fn diag(
        &self,
        buf: &mut dyn std::io::Write,
        idt: u32,
    ) -> Result<(), std::io::Error> {
        write!(buf, "{} {{\n", indent(idt))?;
        let mut it: DecodeBufIterator<'buf> = self.into_iter();
        let mut item_key = it.next();
        while item_key.is_some() {
            if let Some(key) = item_key {
                let item_value = it.next();
                if let Some(value) = item_value {
                    write!(buf, "{} ", indent(idt + 1))?;
                    key.diag(buf, 0)?;
                    write!(buf, ": ")?;
                    value.diag(buf, 0)?;
                }
            }
            write!(buf, ",\n")?;
            item_key = it.next();
        }
        write!(buf, "{} }}\n", indent(idt))
    }
}

/// Implementation of DiagFormatter for TagBuf
#[cfg(any(feature = "full", test))]
impl<'buf> DiagFormatter for TagBuf<'buf> {
    fn diag(
        &self,
        buf: &mut dyn std::io::Write,
        idt: u32,
    ) -> Result<(), std::io::Error> {
        write!(buf, "{} {}( ", indent(idt), self.get_tag())?;
        let mut it: DecodeBufIterator<'buf> = self.into_iter();
        let item = it.next();
        if let Some(cbor) = item {
            write!(buf, "{} ", indent(idt + 1))?;
            cbor.diag(buf, 0)?;
        }
        write!(buf, "{} )\n", indent(idt))
    }
}

/// Implementation of DiagFormatter for any data structure implementing Display.
#[cfg(any(feature = "full", test))]
impl DiagFormatter for dyn Display {
    fn diag(
        &self,
        buf: &mut dyn std::io::Write,
        idt: u32,
    ) -> Result<(), std::io::Error> {
        write!(buf, "{} {}\n", indent(idt), self)
    }
}

/// Implementation of DiagFormatter for any data structure implementing Debug
/// This is a bit of a "last resort" as debug output is a bit ugly in many cases.
#[cfg(any(feature = "full", test))]
impl DiagFormatter for dyn Debug {
    fn diag(
        &self,
        buf: &mut dyn std::io::Write,
        idt: u32,
    ) -> Result<(), std::io::Error> {
        write!(buf, "{} {:?}\n", indent(idt), self)
    }
}

/// Construct an indentation string to indent to indent level idt.
/// This could be used, for example, to pad a format string, e.g. write!(f, "{}foo", indent(3));
#[cfg(any(feature = "full", test))]
fn indent(idt: u32) -> String {
    let mut s: String = String::new();
    for _i in 0..(2 * idt) {
        s.push(' ');
    }
    s
}

/// Print a byte as two hex characters.
///
/// Unfortunately, the #x formatter always puts "0x" in front of a value and we do not want this in
/// diagnostic format, so we implement this manually
#[cfg(any(feature = "full", test))]
pub fn print_hex(b: u8) -> &'static str {
    let table = [
        "00", "01", "02", "03", "04", "05", "06", "07", "08", "09", "0a", "0b", "0c", "0d", "0e",
        "0f", "10", "11", "12", "13", "14", "15", "16", "17", "18", "19", "1a", "1b", "1c", "1d",
        "1e", "1f", "20", "21", "22", "23", "24", "25", "26", "27", "28", "29", "2a", "2b", "2c",
        "2d", "2e", "2f", "30", "31", "32", "33", "34", "35", "36", "37", "38", "39", "3a", "3b",
        "3c", "3d", "3e", "3f", "40", "41", "42", "43", "44", "45", "46", "47", "48", "49", "4a",
        "4b", "4c", "4d", "4e", "4f", "50", "51", "52", "53", "54", "55", "56", "57", "58", "59",
        "5a", "5b", "5c", "5d", "5e", "5f", "60", "61", "62", "63", "64", "65", "66", "67", "68",
        "69", "6a", "6b", "6c", "6d", "6e", "6f", "70", "71", "72", "73", "74", "75", "76", "77",
        "78", "79", "7a", "7b", "7c", "7d", "7e", "7f", "80", "81", "82", "83", "84", "85", "86",
        "87", "88", "89", "8a", "8b", "8c", "8d", "8e", "8f", "90", "91", "92", "93", "94", "95",
        "96", "97", "98", "99", "9a", "9b", "9c", "9d", "9e", "9f", "a0", "a1", "a2", "a3", "a4",
        "a5", "a6", "a7", "a8", "a9", "aa", "ab", "ac", "ad", "ae", "af", "b0", "b1", "b2", "b3",
        "b4", "b5", "b6", "b7", "b8", "b9", "ba", "bb", "bc", "bd", "be", "bf", "c0", "c1", "c2",
        "c3", "c4", "c5", "c6", "c7", "c8", "c9", "ca", "cb", "cc", "cd", "ce", "cf", "d0", "d1",
        "d2", "d3", "d4", "d5", "d6", "d7", "d8", "d9", "da", "db", "dc", "dd", "de", "df", "e0",
        "e1", "e2", "e3", "e4", "e5", "e6", "e7", "e8", "e9", "ea", "eb", "ec", "ed", "ee", "ef",
        "f0", "f1", "f2", "f3", "f4", "f5", "f6", "f7", "f8", "f9", "fa", "fb", "fc", "fd", "fe",
        "ff",
    ];
    table[b as usize]
}
