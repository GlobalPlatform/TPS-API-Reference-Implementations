/***************************************************************************************************
 * Copyright (c) 2020-2021 Jeremy O'Donoghue. All rights reserved.
 *
 * Permission is hereby granted, free of charge, to any person obtaining a copy of this software
 * and associated documentation files (the “Software”), to deal in the Software without
 * restriction, including without limitation the rights to use, copy, modify, merge, publish,
 * distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the
 * Software is furnished to do so, subject to the following conditions:
 *
 * The above copyright notice and this permission notice (including the next
 * paragraph) shall be included in all copies or substantial portions of the
 * Software.
 *
 * THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING
 * BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
 * NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM,
 * DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
 * OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
 **************************************************************************************************/
/// Handling for ieee754 floating point numbers as formatted in CDDL
///
/// We assume that we are handling ieee754 for 64 bit representation.
///
/// The actual storage, over 64 bits, is -
///
/// - 1 sign bit (0 for +ve, 1 for -ve)
/// - 11 bit biased unsigned exponent between -1024 and 1023, where 1023 (01111111111) == 0
/// - 52 bit fractional part
/// Some examples
/// 0 01111111111 0000000000000000000000000000000000000000000000000000 = 3FF0 0000 0000 0000 = 2^0 x 1 = 1
/// 0 01111111111 0000000000000000000000000000000000000000000000000001 = 3FF0 0000 0000 0001 = 2^0 x (1+2^-52) = 1.00
///
/// Now, suppose we have a value of "0x1a3.4b6p1", let's see how it would convert:
///
/// 0x1a3.4b6p1
/// 0x1a3 = 0000_0000_0000__0000_0000_0000_0000_0000_0000_0000_0000_0000_0000_0001_1010_0011
/// 0x4b6 = 0000_0000_0000__0000_0000_0000_0000_0000_0000_0000_0000_0000_0000_0100_1011_0110
/// thus we have 1_1010_0011.0100_1011_0110p1
/// which simplifies to 1.1010_0011_0100_1011_0110p9
/// ...and we drop the integer part to leave the power (because the integral part is always 1)
/// ...so 1010_0011_0100_1011_0110p9
/// The biased exponent is 1023+9 = 1032 = 100_0000_1000
///
/// So our representation is:
/// 0 100_0000_1000 1010_0011_0100_1011_0110_0000_0000_0000_0000_0000_0000_0000_0000
///
/// Note that the error handling in this module is fairly rudimentary because we assume that there
/// is a parser sitting above it validating that input is correct. We detect errors because we can,
/// but it's all a bit basic.
///
extern crate hex;

/// Version of parse_hexfloat where all inputs are slices on strings
pub fn parse_hexfloat(
    is_negative: bool,
    int_part: &str,
    frac_part: &str,
    exp_part: &str,
) -> Result<f64, &'static str> {
    let sign = parse_sign(is_negative);
    let exp = parse_exp(exp_part)?;
    let (signifier, exp_normaliser) = parse_significand(int_part, frac_part)?;
    // Manage the value of the exponent within range
    if (exp == -1023) || (exp == 1024) || (exp_normaliser == 0) {
        // NaN, signed zero, subnormal or normaliser is zero, do nothing
        let exp_bits: u64 = (exp as u64) << 52;
        Ok(f64::from_bits(sign | exp_bits | signifier))
    } else if (exp + exp_normaliser > -1023) && (exp + exp_normaliser < 1024) {
        // Exponent stays in range after normalization
        let exp_bits: u64 = ((exp + exp_normaliser) as u64) << 52;
        Ok(f64::from_bits(sign | exp_bits | signifier))
    } else {
        Err("Hexfloat: Exponent out of range after normalization")
    }
}

/// Parse the integer component of a hex floating point value to determine the sign.
/// Returns 0x8000_0000_0000_0000 for -ve and 0x0000_0000_0000_0000 for +ve
fn parse_sign(is_negative: bool) -> u64 {
    if is_negative {
        0x8000_0000_0000_0000u64
    } else {
        0x0u64
    }
}

/// Parse a (decimal) exponent.
/// Returns a biased exponent in bits 0-10 of the returned i64. Following the maths,
/// this should always be a positive value.
fn parse_exp(s: &str) -> Result<i64, &'static str> {
    let exp = s.parse::<i64>().unwrap();
    if (exp > -1023) && (exp < 1024) {
        // Case 1: well-defined exponent. 1023 represents zero, so add 1023 to all
        Ok((exp + 1023) & 0x0000_07FF)
    } else if exp == -1023 || exp == 1024 {
        // Case 2: signed zero / subnormal / NaN
        Ok(exp)
    } else {
        Err("HexFloat: Exponent out of range")
    }
}

/// Parse a (hexadecimal) integer part of a hex float.
///
/// Returns the normalised significand in the first element of the tuple
/// and the number of bits that offset to add to the exponent in the second part.
fn parse_significand(int_part: &str, frac_part: &str) -> Result<(u64, i64), &'static str> {
    let int_p = parse_hex(int_part)?;
    let frac_p = parse_hex(frac_part)?;
    let zero_int_part = int_p == 0;
    // Work out the modification to the exponent. Two possibilities:
    // - Non-zero integral part will add to the exponent based on the position of the MSB
    // - Zero integral part will subtract from the exponent based on the number of leading zeros
    let exp_modifier: i64 = if !zero_int_part {
        (64 - int_p.leading_zeros()).into()
    } else {
        (0 - frac_p.leading_zeros()).into()
    };
    // If we have a zero integer part, need to determine the number of leading zeros in the fractional
    // part. Cannot rely on parse_hex for this as it does not take account of place value.
    // Each zero character represents fours bits to shift right.
    let frac_shift_lead_zeros = if zero_int_part {
        match frac_part.chars().position(|elem| elem != '0') {
            None => 0u64,
            Some(pos) => pos as u64 * 4u64,
        }
    } else {
        0u64
    };
    // Shift the most significant 4-bit value to the upper 4 bits of the word
    let mut frac_shift = frac_p;
    loop {
        if frac_shift & 0xF000_0000_0000_0000u64 != 0 {
            break;
        }
        frac_shift <<= 4;
    }
    // Now shift right by the number of bits needed to account for leading zeros if the integer part
    // is zero
    frac_shift >>= frac_shift_lead_zeros;

    // Now keep shifting until we get a '1' in bit 53 - we will then be normalised.
    let mut significand = int_p;
    loop {
        // Stop condition: bit 53 == '1'
        if significand & 0x0010_0000_0000_0000u64 != 0 {
            break;
        }
        // Otherwise shift left by one and add the MSB
        let bit = if frac_shift & 0x8000_0000_0000_0000u64 != 0 {
            1u64
        } else {
            0u64
        };
        significand <<= 1;
        significand |= bit;
        frac_shift <<= 1;
    }

    // Mask off the upper bits to ensure that we can "OR" with the exponent.
    significand &= 0x000F_FFFF_FFFF_FFFFu64;
    Ok((significand, exp_modifier))
}

/// Parse a hex value into a u64
fn parse_hex(s: &str) -> Result<u64, &'static str> {
    match u64::from_str_radix(s, 16) {
        Err(_) => Err("Hexfloat: Expected hex string"),
        Ok(val) => Ok(val),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_hex_t() {
        assert_eq!(parse_hex("1234"), Ok(0x1234u64));
        assert_eq!(parse_hex("abcdef"), Ok(0xab_cdefu64));
        assert_ne!(parse_hex("g123"), Ok(0x1234u64));
        assert_ne!(parse_hex("123456789abcde11223344556677"), Ok(0x1234u64));
    }

    fn parse_hexfloat_t() {
        assert_eq!(parse_hexfloat(false, "1", "", "+1"), Ok(2f64));
        assert_eq!(parse_hexfloat(false, "13", "99999999999", "-5"), Ok(0.1f64));
    }
}
