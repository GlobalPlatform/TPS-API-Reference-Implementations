/***************************************************************************************************
 * Copyright (c) 2020-2022 Qualcomm Innovation Center, Inc. All rights reserved.
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
/***************************************************************************************************
 * rs_minicbor CBOR constants
 *
 * A fairly comprehensive, memory efficient, deserializer and serializer for CBOR (RFC7049).
 * This implementation is designed for use in constrained systems and requires neither the Rust
 * standard library nor an allocator.
 **************************************************************************************************/
// Major type bitmask
//pub const MT_MASK: u8 = 0b111_00000;
/// Additional Information bitmask
pub const AI_MASK: u8 = 0b000_11111;

/// Major Type 0 (Positive integers)
pub const MT_UINT: u8 = 0b000_00000;
/// Major Type 1 (Negative integers)
pub const MT_NINT: u8 = 0b001_00000;
/// Major Type 2 (Byte Strings)
pub const MT_BSTR: u8 = 0b010_00000;
/// Major Type 3 (Text Strings)
pub const MT_TSTR: u8 = 0b011_00000;
/// Major Type 4 (Array)
pub const MT_ARRAY: u8 = 0b100_00000;
/// Major Type 5 (Map)
pub const MT_MAP: u8 = 0b101_00000;
/// Major Type 6 (Tag)
pub const MT_TAG: u8 = 0b110_00000;
/// Major Type 7 (Floats, simple types etc.)
pub const MT_SIMPLE: u8 = 0b111_00000;
pub const MT_FLOAT: u8 = 0b111_00000;

/// Maximum value of a "simple" payload mapped on AI bits
pub const PAYLOAD_AI_BITS: u8 = 23;
/// Indicates one byte of length of value information follows MT/AI byte
pub const PAYLOAD_ONE_BYTE: u8 = 24;
/// Indicates two bytes of length of value information follows MT/AI byte
pub const PAYLOAD_TWO_BYTES: u8 = 25;
/// Indicates four bytes of length of value information follows MT/AI byte
pub const PAYLOAD_FOUR_BYTES: u8 = 26;
/// Indicates eight bytes of length of value information follows MT/AI byte
pub const PAYLOAD_EIGHT_BYTES: u8 = 27;
// Indicates an indefinite number of bytes follow. Note that this option is not supported
// in the current version of rs_minicbor.
//pub const PAYLOAD_INDEFINITE_BYTES: u8 = 31;

/// Module defining bitfield values for what types are allowed by the filter trait. See
/// `Allowable`.
#[cfg(feature = "combinators")]
pub mod allow {
    pub const NONE: u32 = 1;
    pub const UINT: u32 = 2;
    pub const NINT: u32 = 4;
    pub const BSTR: u32 = 8;
    pub const TSTR: u32 = 16;
    pub const ARRAY: u32 = 32;
    pub const MAP: u32 = 64;
    pub const TAG: u32 = 128;
    pub const FLOAT: u32 = 256;
    pub const SIMPLE: u32 = 512;
}
