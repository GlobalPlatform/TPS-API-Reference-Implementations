/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
#![allow(dead_code)]

use std::boxed::Box;
use std::string::String;
use std::string::ToString;
use std::vec::Vec;

use tps_minicbor::{Decode, DecodeError, Decoder, EncodeError, EncodeItem, Encoder, Sizer};

const MAJOR_UNSIGNED: u8 = 0;
const MAJOR_NEGATIVE: u8 = 1;
const MAJOR_BYTES: u8 = 2;
const MAJOR_TEXT: u8 = 3;
const MAJOR_ARRAY: u8 = 4;
const MAJOR_MAP: u8 = 5;

pub fn decode_all<'a, T: Decode<'a>>(bytes: &'a [u8]) -> Result<T, DecodeError> {
    let mut d = Decoder::new(bytes);
    let value = T::decode(&mut d)?;
    if d.pos() != bytes.len() {
        return Err(DecodeError::InvalidAdditionalInfo);
    }
    Ok(value)
}

#[derive(Debug, PartialEq)]
pub struct Sample<'a> {
    pub id: u64,
    pub name: &'a str,
    pub data: &'a [u8],
}

impl EncodeItem for Sample<'_> {
    fn size(&self, s: &mut Sizer) -> Result<(), EncodeError> {
        s.array_len(3)?;
        self.id.size(s)?;
        self.name.size(s)?;
        self.data.size(s)?;
        Ok(())
    }

    fn encode<const N: usize>(&self, e: &mut Encoder<'_, N>) -> Result<(), EncodeError> {
        let mut array = e.array_with_len(3)?;
        array.insert(self.id)?;
        array.insert(self.name)?;
        array.insert(self.data)?;
        array.end()
    }
}

impl<'a> Decode<'a> for Sample<'a> {
    fn decode(d: &mut Decoder<'a>) -> Result<Self, DecodeError> {
        let len = d.array_len()?;
        if len != 3 {
            return Err(DecodeError::InvalidAdditionalInfo);
        }
        let id = u64::decode(d)?;
        let name = <&str>::decode(d)?;
        let data = <&[u8]>::decode(d)?;
        Ok(Self { id, name, data })
    }
}

#[derive(Debug, PartialEq)]
pub enum TestValue {
    Unsigned(u64),
    Negative(i64),
    Bytes(Vec<u8>),
    Text(String),
    Array(Vec<TestValue>),
    Map(Vec<(TestValue, TestValue)>),
    Bool(bool),
    Null,
    Undefined,
    Simple(u8),
    Float(f64),
    Tag(u64, Box<TestValue>),
}

struct TestDecoder<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> TestDecoder<'a> {
    fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    fn len(&self) -> usize {
        self.pos
    }

    fn read(&mut self) -> Result<u8, DecodeError> {
        if self.pos >= self.buf.len() {
            return Err(DecodeError::UnexpectedEof);
        }
        let b = self.buf[self.pos];
        self.pos += 1;
        Ok(b)
    }

    fn read_be(&mut self, bytes: usize) -> Result<u64, DecodeError> {
        let mut out = 0u64;
        for _ in 0..bytes {
            out = (out << 8) | (self.read()? as u64);
        }
        Ok(out)
    }

    fn read_header(&mut self) -> Result<(u8, u8, u64), DecodeError> {
        let first = self.read()?;
        let major = first >> 5;
        let addl = first & 0x1f;
        let value = match addl {
            0..=23 => addl as u64,
            24 => self.read_be(1)?,
            25 => self.read_be(2)?,
            26 => self.read_be(4)?,
            27 => self.read_be(8)?,
            31 => return Err(DecodeError::IndefiniteLength),
            _ => return Err(DecodeError::InvalidAdditionalInfo),
        };
        Ok((major, addl, value))
    }
}

fn decode_value(d: &mut TestDecoder<'_>) -> Result<TestValue, DecodeError> {
    let (major, addl, value) = d.read_header()?;
    match major {
        MAJOR_UNSIGNED => Ok(TestValue::Unsigned(value)),
        MAJOR_NEGATIVE => {
            if value > i64::MAX as u64 {
                return Err(DecodeError::InvalidAdditionalInfo);
            }
            let n = (value as i64)
                .checked_add(1)
                .and_then(|v| v.checked_neg())
                .ok_or(DecodeError::InvalidAdditionalInfo)?;
            Ok(TestValue::Negative(n))
        }
        MAJOR_BYTES => {
            let len = value as usize;
            if d.pos + len > d.buf.len() {
                return Err(DecodeError::UnexpectedEof);
            }
            let out = d.buf[d.pos..d.pos + len].to_vec();
            d.pos += len;
            Ok(TestValue::Bytes(out))
        }
        MAJOR_TEXT => {
            let len = value as usize;
            if d.pos + len > d.buf.len() {
                return Err(DecodeError::UnexpectedEof);
            }
            let bytes = &d.buf[d.pos..d.pos + len];
            d.pos += len;
            let s = std::str::from_utf8(bytes).map_err(|_| DecodeError::InvalidUtf8)?;
            Ok(TestValue::Text(s.to_string()))
        }
        MAJOR_ARRAY => {
            let len = value as usize;
            let mut items = Vec::with_capacity(len);
            for _ in 0..len {
                items.push(decode_value(d)?);
            }
            Ok(TestValue::Array(items))
        }
        MAJOR_MAP => {
            let len = value as usize;
            let mut items = Vec::with_capacity(len);
            for _ in 0..len {
                let key = decode_value(d)?;
                let val = decode_value(d)?;
                items.push((key, val));
            }
            Ok(TestValue::Map(items))
        }
        6 => {
            let item = decode_value(d)?;
            Ok(TestValue::Tag(value, Box::new(item)))
        }
        7 => match addl {
            0..=19 => Ok(TestValue::Simple(addl)),
            20 => Ok(TestValue::Bool(false)),
            21 => Ok(TestValue::Bool(true)),
            22 => Ok(TestValue::Null),
            23 => Ok(TestValue::Undefined),
            24 => Ok(TestValue::Simple(value as u8)),
            25 => {
                let f = f16_to_f32(value as u16);
                Ok(TestValue::Float(f as f64))
            }
            26 => Ok(TestValue::Float(f32::from_bits(value as u32) as f64)),
            27 => Ok(TestValue::Float(f64::from_bits(value))),
            _ => Err(DecodeError::InvalidAdditionalInfo),
        },
        _ => Err(DecodeError::InvalidAdditionalInfo),
    }
}

pub fn decode_value_all(bytes: &[u8]) -> Result<TestValue, DecodeError> {
    let mut d = TestDecoder::new(bytes);
    let value = decode_value(&mut d)?;
    if d.len() != bytes.len() {
        return Err(DecodeError::InvalidAdditionalInfo);
    }
    Ok(value)
}

pub fn hex_to_bytes(hex: &str) -> Vec<u8> {
    let mut out = Vec::new();
    let mut digits = Vec::new();
    let mut input = hex.trim();
    if let Some(rest) = input
        .strip_prefix("0x")
        .or_else(|| input.strip_prefix("0X"))
    {
        input = rest;
    }
    for c in input.chars() {
        if c.is_ascii_hexdigit() {
            digits.push(c);
        }
    }
    if digits.len() % 2 != 0 {
        if digits.first() == Some(&'0') {
            digits.remove(0);
        } else {
            digits.insert(0, '0');
        }
    }
    let mut iter = digits.chunks_exact(2);
    for pair in &mut iter {
        let high = pair[0].to_digit(16).unwrap() as u8;
        let low = pair[1].to_digit(16).unwrap() as u8;
        out.push((high << 4) | low);
    }
    out
}

fn f16_to_f32(bits: u16) -> f32 {
    let sign = ((bits >> 15) & 0x1) as u32;
    let exp = ((bits >> 10) & 0x1f) as i32;
    let frac = (bits & 0x03ff) as u32;
    let f32_bits = if exp == 0 {
        if frac == 0 {
            sign << 31
        } else {
            let mut frac_norm = frac;
            let mut exp_norm = -14i32;
            while (frac_norm & 0x0400) == 0 {
                frac_norm <<= 1;
                exp_norm -= 1;
            }
            let frac32 = (frac_norm & 0x03ff) << 13;
            let exp32 = (exp_norm + 127) as u32;
            (sign << 31) | (exp32 << 23) | frac32
        }
    } else if exp == 31 {
        (sign << 31) | 0x7f800000 | (frac << 13)
    } else {
        let exp32 = (exp - 15 + 127) as u32;
        (sign << 31) | (exp32 << 23) | (frac << 13)
    };
    f32::from_bits(f32_bits)
}
