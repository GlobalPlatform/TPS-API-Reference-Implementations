/***************************************************************************************************
 * Copyright (c) 2019-2021 Jeremy O'Donoghue. All rights reserved.
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
/***************************************************************************************************
 * Parser for CDDL, based on the grammar in RFC8610, Appendix B.
 *
 * The implementation uses the "Nom" parser combinator library
 **************************************************************************************************/
extern crate alloc;
extern crate base64;
extern crate hex;
extern crate nom;

use nom::{
    branch::alt, bytes::complete::tag, combinator::opt, error::context, error::ErrorKind,
    error::ParseError, error::VerboseError, multi::many0, multi::many1, multi::many_till,
    sequence::delimited, sequence::preceded, sequence::terminated, sequence::tuple, AsChar, Err,
    IResult, InputIter, Slice,
};
use std::iter::FromIterator;
use std::str;

use crate::cddl::ast::{
    Assignment, BsQual, GenericParam, Group, GroupItem, MemberKey, Occurs, Operator, Rule, Type,
    Value, CDDL,
};
use crate::cddl::hexfloat;

/**************************************************************************************************
 * CDDL file handling
 *************************************************************************************************/

/*
/// Construct a parse error
fn parse_err(s: String, ln: usize, pos: usize) -> ParseError {
    ParseError {
        line: ln as u32,
        pos: pos as u32,
        msg: s.clone(),
    }
}

/// Implement the Display trait for ParseError to allow error messages to be printed
impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} at line {} character {}",
            self.msg, self.line, self.pos
        )
    }
}
*/

/**************************************************************************************************
 * Error Handling
 *************************************************************************************************/

/// Type alias for the parser buffer underlying implementation.
type Buf<'a> = &'a str;

/// Type alias for the error type used by CDDL Parsers
type CDDLError<'a> = VerboseError<Buf<'a>>;

/// Type Alias for the result of any CDDL parser operation
type ParseResult<'a, T> = IResult<Buf<'a>, T, CDDLError<'a>>;

/// Macro to shorten error handling boilerplate in parsers. Constructs a CDDLError<'a>
/// instance usable in a ParseResult<'a>.
macro_rules! parse_err {
    ($buf:expr, $msg:expr, $kind:expr) => {
        Err(Err::Error(nom::error::ContextError::add_context(
            $buf,
            $msg,
            ParseError::from_error_kind($buf, $kind),
        )))
    };
}

/**************************************************************************************************
 * CDDL Grammar - see draft-ietf-cbor-cddl-08 / March 2019
 *************************************************************************************************/

/// Parser for top level:
///
/// ```text
/// cddl = S 1*(rule S)
/// ```
pub fn cddl(b: Buf) -> ParseResult<CDDL> {
    preceded(s, many1(terminated(rule, s)))(b)
}

/// Parser for
///
/// ```text
/// rule = typename [genericparm] S assignt S type
///      / groupname [genericparm] S assigng S grpent
/// ```
fn rule(b: Buf) -> ParseResult<Rule> {
    // typename [genericparm] S assignt S type
    fn p_typedef(b: Buf) -> ParseResult<Rule> {
        let (i, tn) = typename(b)?;
        let (i, gp) = opt(genericparm)(i)?;
        let (i, asgn) = delimited(s, assignt, s)(i)?;
        let (i, typ) = type0(i)?;
        Ok((i, Rule::TypeDef(tn, gp, asgn, Box::new(typ))))
    }
    // groupname [genericparm] S assigng S grpent
    fn p_groupdef(b: Buf) -> ParseResult<Rule> {
        let (i, gn) = groupname(b)?;
        let (i, gp) = opt(genericparm)(i)?;
        let (i, asgn) = delimited(s, assigng, s)(i)?;
        let (i, grp) = grpent(i)?;
        Ok((i, Rule::GroupDef(gn, gp, asgn, Box::new(grp))))
    }
    alt((p_typedef, p_groupdef))(b)
}

/// Parser for
///
/// ```text
/// assignt = "=" / "/="
/// ```
fn assignt(b: Buf) -> ParseResult<Assignment> {
    let (i, s) = alt((tag("="), tag("/=")))(b)?;
    match s {
        "=" => Ok((i, Assignment::Assign)),
        _ => Ok((i, Assignment::AssignExtend)),
    }
}

/// Parser for
///
/// ```text
/// assigng = "=" / "//="
/// ```
fn assigng(b: Buf) -> ParseResult<Assignment> {
    let (i, s) = alt((tag("="), tag("//=")))(b)?;
    match s {
        "=" => Ok((i, Assignment::Assign)),
        _ => Ok((i, Assignment::AssignExtend)),
    }
}

/// Parser for
///
/// ```text
/// genericparm = "<" S id S *("," S id S ) ">"
/// ```
fn genericparm(b: Buf) -> ParseResult<GenericParam> {
    let (i, id1) = delimited(tuple((char_is('<'), s)), id, s)(b)?;
    let (i, ids) = many0(delimited(tuple((char_is(','), s)), id, s))(i)?;
    let (i, _) = char_is('>')(i)?;
    let mut gps = Vec::<String>::new();
    gps.push(id1);

    for i in ids {
        gps.push(i);
    }
    Ok((i, gps))
}

/// Parser for
///
/// ```text
/// genericarg = "<" S type1 S *("," S type1 S ) ">"
/// ```
fn genericarg(b: Buf) -> ParseResult<Vec<Type>> {
    let (i, t1) = delimited(tuple((char_is('<'), s)), type1, s)(b)?;
    let (i, ts) = many0(delimited(tuple((char_is(','), s)), type1, s))(i)?;
    let (i, _) = char_is('>')(i)?;
    let mut v = Vec::<Type>::new();
    v.push(t1);

    for item in ts {
        v.push(item);
    }
    Ok((i, v))
}

//*************************************************************************************************
// Type definitions
//*************************************************************************************************

/// Parser for
///
/// ```text
/// type = type1 *(S "/" S type1)
/// ```
///
/// **Note:** The type rule (`fn type0()` in the parser as type is a reserved word in Rust) states
/// that a type can be defined as a choice between one or more types.
fn type0(b: Buf) -> ParseResult<Type> {
    fn type1s(b: Buf) -> ParseResult<Vec<Type>> {
        let (i, ts) = many0(preceded(tuple((s, char_is('/'), s)), type1))(b)?;
        Ok((i, ts))
    }

    let (i, t) = type1(b)?;
    let (i, ts) = type1s(i)?;
    let mut v = vec![t];

    if ts.is_empty() {
        Ok((i, Type::Types(v)))
    } else {
        for item in ts {
            v.push(item);
        }
        Ok((i, Type::Types(v)))
    }
}

/// Parser for
///
/// ```text
/// type1 = type2 [S (rangeop / ctlop) S type2]
/// ```
///
/// The type1 rule allows types to be optionally combined using range or control operators.
/// Types used in conjunction with range operators need to resolve to values that have meaningful
/// range semantics (int or float, basically). Types used in conjunction with control operators need
/// to be compatible with those control operators.
fn type1(b: Buf) -> ParseResult<Type> {
    fn op_type2(b: Buf) -> ParseResult<(Type, Operator)> {
        let (i, op) = delimited(s, alt((rangeop, ctlop)), s)(b)?;
        let (i, ty) = type2(i)?;
        // NB: The returned Type::Combined has the same type in both boxes. The first box should be discarded
        // in later processing
        Ok((i, (ty, op)))
    }
    let (i, t1) = type2(b)?;
    let (i, t2) = opt(op_type2)(i)?;

    match t2 {
        None => Ok((i, t1)),
        Some((typ2, op)) => Ok((i, Type::Combined(Box::new(t1), Box::new(typ2), op))),
    }
}

/// Parser for
///
/// ```text
/// type2 = value
///       / typename [genericarg]
///       / "(" S type S ")"
///       / "{" S group S "}"
///       / "[" S group S "]"
///       / "~" S typename [genericarg]
///       / "&" S "(" S group S ")"
///       / "&" S groupname [genericarg]
///       / "#" "6" ["." uint] "(" S type S ")"
///       / "#" DIGIT ["." uint]
///       / "#"
/// ```
///
/// The type2 rule defines the primitive and constructed types in CDDL. Note that CDDL uses
/// ordered matching, with the first successful match succeeding.
fn type2(b: Buf) -> ParseResult<Type> {
    // value
    fn p_value(b: Buf) -> ParseResult<Type> {
        let (i, v) = value(b)?;
        Ok((i, Type::Value(v)))
    }
    // typename [genericarg]
    fn p_rule(b: Buf) -> ParseResult<Type> {
        let (i, tn) = typename(b)?;
        let (i, ga) = opt(genericarg)(i)?;
        Ok((i, Type::Rule(tn, ga)))
    }
    // "(" S type S ")"
    fn p_types(b: Buf) -> ParseResult<Type> {
        delimited(tuple((char_is('('), s)), type0, tuple((s, char_is(')'))))(b)
    }
    // "{" S group S "}"
    fn p_groupmap(b: Buf) -> ParseResult<Type> {
        let (i, g) = delimited(tuple((char_is('{'), s)), group, tuple((s, char_is('}'))))(b)?;
        Ok((i, Type::GroupMap(g)))
    }
    // "[" S group S "]"
    fn p_grouparray(b: Buf) -> ParseResult<Type> {
        let (i, g) = delimited(tuple((char_is('['), s)), group, tuple((s, char_is(']'))))(b)?;
        Ok((i, Type::GroupArray(g)))
    }
    // "~" S typename [genericarg]
    fn p_unwrap(b: Buf) -> ParseResult<Type> {
        let (i, _) = tuple((char_is('~'), s))(b)?;
        let (i, tn) = p_rule(i)?;
        match tn {
            Type::Rule(s, ga) => Ok((i, Type::Unwrap(s, ga))),
            _ => parse_err!(i, "illegal typename", ErrorKind::AlphaNumeric),
        }
    }
    // "&" S "(" S group S ")"
    fn p_groupenum(b: Buf) -> ParseResult<Type> {
        let (i, g) = delimited(
            tuple((char_is('&'), s, char_is('('), s)),
            group,
            tuple((s, char_is(')'))),
        )(b)?;
        Ok((i, Type::GroupEnum(g)))
    }
    // "&" S groupname [genericarg]
    fn p_groupname_enum(b: Buf) -> ParseResult<Type> {
        let (i, gn) = preceded(tuple((char_is('&'), s)), groupname)(b)?;
        let (i, ga) = opt(genericarg)(i)?;
        Ok((i, Type::GroupNameEnum(gn, ga)))
    }
    // "#" "6" ["." uint] "(" S type S ")"
    fn p_tagged(b: Buf) -> ParseResult<Type> {
        let (i, _) = tuple((char_is('#'), char_is('6')))(b)?;
        let (i, tag) = opt(preceded(char_is('.'), uint))(i)?;
        let (i, typ) = delimited(tuple((char_is('('), s)), type0, tuple((s, char_is(')'))))(i)?;
        Ok((i, Type::Tagged(tag, Box::new(typ))))
    }
    // "#" DIGIT ["." uint]
    fn p_major(b: Buf) -> ParseResult<Type> {
        let (i, mt) = preceded(char_is('#'), digit)(b)?;
        let (i, ai) = opt(preceded(char_is('.'), uint))(i)?;
        let major = mt as i64 - 0x30i64; // parser for mt ensures mt can only be '0'..='9'
        Ok((i, Type::Major(major, ai)))
    }
    // "#"
    fn p_any(b: Buf) -> ParseResult<Type> {
        let (i, _) = char_is('#')(b)?;
        Ok((i, Type::Any))
    }
    alt((
        p_value,
        p_rule,
        p_types,
        p_groupmap,
        p_grouparray,
        p_unwrap,
        p_groupenum,
        p_groupname_enum,
        p_tagged,
        p_major,
        p_any,
    ))(b)
}

/// Parser for
///
/// ```text
/// group = grpchoice *(S "//" S grpchoice)
/// ```
fn group(b: Buf) -> ParseResult<Group> {
    let (i, gc1) = grpchoice(b)?;
    let (i, gcs) = many0(preceded(tuple((s, tag("//"), s)), grpchoice))(i)?;
    let mut result = gc1;
    for gc in gcs {
        result.extend(gc);
    }
    Ok((i, result))
}

/// Parser for
///
/// ```text
/// grpchoice = *(grpent optcom)
/// ```
fn grpchoice(b: Buf) -> ParseResult<Group> {
    let (i, grp) = many0(terminated(grpent, optcom))(b)?;
    Ok((i, grp))
}

/// Parser for
///
/// ```text
/// grpent = [occur S] [memberkey S] type
///        / [occur S] groupname [genericarg]  ; preempted by above
///        / [occur S] "(" S group S ")"
/// ```
fn grpent(b: Buf) -> ParseResult<GroupItem> {
    // [occur S]
    fn p_occur(b: Buf) -> ParseResult<Occurs> {
        let (i, occ) = opt(terminated(occur, s))(b)?;
        match occ {
            None => Ok((i, Occurs::Once)),
            Some(occur) => Ok((i, occur)),
        }
    }
    // [occur S] [memberkey S] type
    fn p_memberkey(b: Buf) -> ParseResult<GroupItem> {
        let (i, occ) = p_occur(b)?;
        let (i, mk) = opt(terminated(memberkey, s))(i)?;
        let (i, typ) = type0(i)?;
        match mk {
            None => Ok((i, GroupItem::Key(None, typ, occ))),
            Some(mk) => Ok((i, GroupItem::Key(Some(Box::new(mk)), typ, occ))),
        }
    }
    // [occur S] groupname [genericarg]
    fn p_groupname(b: Buf) -> ParseResult<GroupItem> {
        let (i, occ) = p_occur(b)?;
        let (i, gn) = groupname(i)?;
        let (i, ga) = opt(genericarg)(i)?;
        Ok((i, GroupItem::Name(gn, occ, ga)))
    }
    // [occur S] "(" S group S ")"
    fn p_groupdef(b: Buf) -> ParseResult<GroupItem> {
        let (i, occ) = p_occur(b)?;
        let (i, grp) = delimited(tuple((char_is('('), s)), group, tuple((s, char_is(')'))))(i)?;
        Ok((i, GroupItem::Grp(grp, occ)))
    }
    alt((p_memberkey, p_groupname, p_groupdef))(b)
}

/// Parser for
///
/// ```text
/// memberkey = type1 S ["^" S] "=>" / bareword S ":" / value S ":"
/// ```
///
/// The memberkey rule defines values that can be used as keys in a CBOR map.
fn memberkey(b: Buf) -> ParseResult<MemberKey> {
    // type1 S ["^" S] "=>"
    fn p_type1(b: Buf) -> ParseResult<MemberKey> {
        let (i, t1) = terminated(type1, s)(b)?;
        let (i, cut) = opt(terminated(char_is('^'), s))(i)?;
        let (i, _) = tag("=>")(i)?;
        Ok((i, MemberKey::FromType(Box::new(t1), cut.is_some())))
    }
    // bareword S ":"
    fn p_bareword(b: Buf) -> ParseResult<MemberKey> {
        let (i, bw) = terminated(bareword, tuple((s, char_is(':'))))(b)?;
        Ok((i, MemberKey::FromValue(Box::new(Value::Tstr(bw)))))
    }
    // value S ":"
    fn p_value(b: Buf) -> ParseResult<MemberKey> {
        let (i, val) = terminated(value, tuple((s, char_is(':'))))(b)?;
        Ok((i, MemberKey::FromValue(Box::new(val))))
    }
    alt((p_type1, p_bareword, p_value))(b)
}

//*************************************************************************************************
// Qualifiers
//*************************************************************************************************

/// Parser for
///
/// ```text
/// rangeop = "..." / ".."
/// ```
fn rangeop(b: Buf) -> ParseResult<Operator> {
    let (i, s) = alt((tag("..."), tag("..")))(b)?;
    match s {
        "..." => Ok((i, Operator::RangeExcl)),
        ".." => Ok((i, Operator::RangeIncl)),
        _ => parse_err!(i, "illegal range operation", ErrorKind::Alt),
    }
}

/// Parser for
///
/// ```text
/// ctlop = "." id
/// ```
fn ctlop(b: Buf) -> ParseResult<Operator> {
    let (i, op) = preceded(char_is('.'), id)(b)?;
    Ok((i, Operator::Control(op)))
}

/// Parser for
///
/// ```text
/// occur = [uint] "*" [uint] / "+" / "?"
/// ```
fn occur(b: Buf) -> ParseResult<Occurs> {
    // Helper parser for occur = [uint] "*" [uint]
    fn from_to(b: Buf) -> ParseResult<Occurs> {
        let (i, from) = opt(uint)(b)?;
        let (i, _) = char_is('*')(i)?;
        let (i, upto) = opt(uint)(i)?;
        let from_value = match from {
            None => 0,
            Some(v) => v,
        };
        let upto_value = match upto {
            None => i64::MAX,
            Some(v) => v,
        };
        if from_value == 0 && upto.is_none() {
            Ok((i, Occurs::ZeroPlus))
        } else if from_value == 0 && upto_value == 1 {
            Ok((i, Occurs::Optional))
        } else {
            Ok((i, Occurs::Between(from_value, upto_value)))
        }
    }
    // Helper parser for "+"
    fn one_or_more(b: Buf) -> ParseResult<Occurs> {
        let (i, _) = char_is('+')(b)?;
        Ok((
            i,
            Occurs::OnePlus,
        ))
    }
    // Helper parser for "?"
    fn optional(b: Buf) -> ParseResult<Occurs> {
        let (i, _) = char_is('?')(b)?;
        Ok((i, Occurs::Optional))
    }
    alt((from_to, one_or_more, optional))(b)
}

//*************************************************************************************************
// Parsing basic values, identifiers and the like
//*************************************************************************************************

/// Parser for
///
/// ```text
/// uint = DIGIT1 *DIGIT / "0x" 1*HEXDIG / "0b" 1*BINDIG / "0"
/// ```
///
/// TODO: currently returns i64, which doesn't cover some legal values. Split to UInt and NInt.
fn uint(b: Buf) -> ParseResult<i64> {
    // Helper for parsing decimal integers. Called from `uint`.
    fn dec_int(b: Buf) -> ParseResult<i64> {
        let (i, first_dig) = digit1(b)?;
        let (i, rest_digs) = many0(digit)(i)?;
        let mut s = String::from_iter(rest_digs);
        s.insert(0, first_dig);
        match i64::from_str_radix(&s, 10) {
            Ok(val) => Ok((i, val)),
            Err(_) => parse_err!(i, "expected decimal digit", ErrorKind::Digit),
        }
    }
    // Helper for parsing hex values
    fn hex_int(b: Buf) -> ParseResult<i64> {
        let (i, _) = tag("0x")(b)?;
        let (i, digits) = many1(hexdig)(i)?;
        match i64::from_str_radix(&(String::from_iter(digits)), 16) {
            Ok(val) => Ok((i, val)),
            Err(_) => parse_err!(i, "expected hex digit", ErrorKind::HexDigit),
        }
    }
    // Helper for parsing bin values
    fn bin_int(b: Buf) -> ParseResult<i64> {
        let (i, _) = tag("0b")(b)?;
        let (i, digits) = many1(bindig)(i)?;
        match i64::from_str_radix(&(String::from_iter(digits)), 2) {
            Ok(val) => Ok((i, val)),
            Err(_) => parse_err!(i, "expected hex digit", ErrorKind::HexDigit),
        }
    }
    // Helper for parsing zero
    fn zero_int(b: Buf) -> ParseResult<i64> {
        let (i, _) = char_is('0')(b)?;
        Ok((i, 0i64))
    }

    alt((dec_int, hex_int, bin_int, zero_int))(b)
}

/// Parser for
///
/// ```text
/// value = number / text / bytes
/// ```
fn value(b: Buf) -> ParseResult<Value> {
    alt((number, text, bytes))(b)
}

/// Parser for
///
/// ```text
/// int = [-] uint
/// ```
fn int(b: Buf) -> ParseResult<i64> {
    let (i, sign) = opt(char_is('-'))(b)?;
    let (i, val) = uint(i)?;
    match sign {
        None => Ok((i, val)),
        Some(_) => Ok((i, -val)),
    }
}

/// Parser for
///
/// ```text
/// number = hexfloat / (int ["." fraction] ["e" exponent])
/// ```
fn number(b: Buf) -> ParseResult<Value> {
    // Helper for parsing numbers - (int ["." fraction] ["e" exponent])
    fn int_or_float(b: Buf) -> ParseResult<Value> {
        let (i, int) = int(b)?;
        let (i, frac_part) = opt(preceded(char_is('.'), fraction))(i)?;
        let (i, exp_part) = opt(preceded(char_is('e'), exponent))(i)?;

        match (frac_part, exp_part) {
            // No fractional part or exponent - so int
            (None, None) => Ok((i, Value::Int(int))),
            (Some(frac), None) => {
                let s: &str = &[int.to_string(), ".".to_string(), frac].concat();
                let f = s.parse::<f64>().unwrap();
                Ok((i, Value::Float(f)))
            }
            (None, Some(exp)) => {
                let s: &str = &[int.to_string(), "e".to_string(), exp].concat();
                let f = s.parse::<f64>().unwrap();
                Ok((i, Value::Float(f)))
            }
            (Some(frac), Some(exp)) => {
                let s: &str =
                    &[int.to_string(), ".".to_string(), frac, "e".to_string(), exp].concat();
                let f = s.parse::<f64>().unwrap();
                Ok((i, Value::Float(f)))
            }
        }
    }

    let (i, v) = alt((hexfloat, int_or_float))(b)?;
    Ok((i, v))
}

/// Parser for
///
/// ```text
/// hexfloat = ["-"] "0x" 1*HEXDIG ["." 1*HEXDIG] "p" exponent
/// ```
fn hexfloat(b: Buf) -> ParseResult<Value> {
    let (i, is_neg) = opt(char_is('-'))(b)?;
    let (i, int_v) = preceded(tag("0x"), many1(hexdig))(i)?;
    let (i, float_v) = opt(preceded(char_is('.'), many1(hexdig)))(i)?;
    let (i, exp) = preceded(char_is('p'), exponent)(i)?;
    let is_negative = match is_neg {
        Some(_) => true,
        _ => false,
    };
    let int_s = String::from_iter(int_v);
    let float_s = match float_v {
        None => "".to_string(),
        Some(seq) => String::from_iter(seq),
    };
    match hexfloat::parse_hexfloat(is_negative, int_s.as_str(), float_s.as_str(), exp.as_str()) {
        Ok(float_val) => Ok((i, Value::Float(float_val))),
        Err(s) => parse_err!(i, s, ErrorKind::HexDigit),
    }
}

/// Parser for
///
/// ```text
/// fraction = 1*DIGIT
/// ```
fn fraction(b: Buf) -> ParseResult<String> {
    let (i, digits) = many1(digit)(b)?;
    Ok((i, String::from_iter(digits)))
}

/// Parser for
///
/// ```text
/// exponent = ["+"/"-"] 1*DIGIT
/// ```
fn exponent(b: Buf) -> ParseResult<String> {
    let (i, sign) = opt(alt((char_is('+'), char_is('-'))))(b)?;
    let sign = match sign {
        None => '+',
        Some(c) => c,
    };
    let (i, digits) = many1(digit)(i)?;
    let mut s = String::from_iter(digits);
    s.insert(0, sign);
    Ok((i, s))
}

/// Parser for
///
/// ```text
/// text = %x22 *SCHAR %x22
/// ```
fn text(b: Buf) -> ParseResult<Value> {
    let (i, chars_v) = context(
        "text = %x22 *SCHAR %x22",
        delimited(char_is('\u{022}'), many0(schar), char_is('\u{022}')),
    )(b)?;
    let st = chars_v.into_iter().collect();
    Ok((i, Value::Tstr(st)))
}

/// Parser for
///
/// ```text
/// bytes = [bsqual] %x27 *BCHAR %x27
/// ```
///
/// From the I-D, note the following behaviour.
///
/// - An unprefixed string is interpreted as a text string with "'" characters escaped.
///   It is interpreted as a byte string.
/// - A prefix of 'h' is used to preceed a string processed as a sequence containing pairs of
///   [0-9a-fA-F] characters. Whitespace is ignored for clarity.
/// - A prefix of 'b64' is used to preceed a string processed as a base 64 URL string. Again
///   whitespace is ignored for clarity. Following RFC4648, it is assumed that the URL-safe
///   base64 encoding is used in the string, to comply with I-D section 3.1 which states this
///   us a base64(url) string. Since RFC4648 states that padding MUST be included unless
///   explicitly stated otherwise, we configure base64 parser to require this.
///
/// Any deviation from the above is an error.
fn bytes(b: Buf) -> ParseResult<Value> {
    let (i, may_qual) = opt(bsqual)(b)?;
    let (i, bytes_v) = context(
        "bytes = [bsqual] %x27 *BCHAR %x27",
        delimited(char_is('\u{027}'), many0(bchar), char_is('\u{027}')),
    )(i)?;

    // bytes_v is Vec<String> as many0 returns a vector of successful parses. Destructure it.
    let bytes: String = bytes_v.into_iter().collect();
    match may_qual {
        //
        // String interpreted as a sequence of bytes. Note that any UTF8 characters permitted
        //
        None => Ok((i, Value::Bytes(bytes.into_bytes()))), // No bsqual case
        //
        // String interpreted as a sequence of pairs of hex characters
        //
        Some(BsQual::ByteStr) => match hex::decode(bytes) {
            // Successful hex decode
            Ok(vec_u8) => Ok((i, Value::Bytes(vec_u8))),
            // Error - odd length string
            Err(hex::FromHexError::OddLength) => {
                parse_err!(i, "bytes (hex): Odd Length String", ErrorKind::Many0)
            }
            // Error - invalid hex character in string
            Err(hex::FromHexError::InvalidHexCharacter { c: _, index: _ }) => {
                parse_err!(i, "bytes (hex): Invalid hex character", ErrorKind::Many0)
            }
            // Other error - we should not really get here.
            _ => parse_err!(i, "bytes (hex): other error", ErrorKind::Many0),
        },
        //
        // String interpreted accoring to the URL-safe base64 encoding with padding
        //
        Some(BsQual::Base64) => match base64::decode_config(&bytes, base64::URL_SAFE) {
            // Successful base64 decode
            Ok(vec_u8) => Ok((i, Value::Bytes(vec_u8))),
            // Error - invalid byte
            Err(base64::DecodeError::InvalidByte(_, _)) => {
                parse_err!(i, "bytes (b64): Invalid byte", ErrorKind::Many0)
            }
            // Error - invalid length
            Err(base64::DecodeError::InvalidLength) => {
                parse_err!(i, "bytes (b64): Invalid length", ErrorKind::Many0)
            }
            // Error - invalid symbol
            Err(base64::DecodeError::InvalidLastSymbol(_, _)) => {
                parse_err!(i, "bytes (b64): Invalid symbol", ErrorKind::Many0)
            }
        },
    }
}

/// Parser for
///
/// ```text
/// bsqual = "h" / "b64"
/// ```
fn bsqual(b: Buf) -> ParseResult<BsQual> {
    let (i, slice) = context("bsqual = \"h\" / \"b64\"", alt((tag("h"), tag("b64"))))(b)?;
    match slice {
        "h" => Ok((i, BsQual::ByteStr)),
        "b64" => Ok((i, BsQual::Base64)),
        _ => parse_err!(i, "expecting byte string qualifier h / b64", ErrorKind::Alt),
    }
}

/// Parser for
///
/// ```text
/// id = EALPHA *(*("-" / ".") (EALPHA / DIGIT))
/// ```
fn id(b: Buf) -> ParseResult<String> {
    let seps = many0(alt((tag("-"), tag("."))));
    let alpha_or_digit = alt((ealpha, digit)); // ParseResult<char>
    let (i, first) = ealpha(b)?; // ParseResult<char>
    let (i, rest) = many0(tuple((seps, alpha_or_digit)))(i)?; // Vec<(Vec<&str>, char)>
                                                              // Destructure `rest`
    let mut rest_str: String = first.to_string();
    for it in rest {
        let (sxs, ch) = it;
        if !sxs.is_empty() {
            for i in sxs {
                rest_str.push_str(i);
            }
        }
        rest_str.push(ch);
    }
    Ok((i, rest_str))
}

/// Parser for
///
/// ```text
/// typename = id
/// ```
fn typename(b: Buf) -> ParseResult<String> {
    id(b)
}

/// Parser for
///
/// ```text
/// groupname = id
/// ```
fn groupname(b: Buf) -> ParseResult<String> {
    id(b)
}

/// Parser for
///
/// ```text
/// bareword = id
/// ```
fn bareword(b: Buf) -> ParseResult<String> {
    id(b)
}

//*************************************************************************************************
// Terminals - this part is basically the lexer
//*************************************************************************************************

/// Parser for
///
/// ```text
/// SCHAR = %x20-21 / %x23-5B / %x5D-7E / %x80-10FFFD / SESC
/// ```
fn schar(b: Buf) -> ParseResult<char> {
    let is_bc_range = char_pred(|c| match c {
        '\u{0020}'
        | '\u{0021}'
        | '\u{0023}'..='\u{005b}'
        | '\u{005d}'..='\u{007e}'
        | '\u{0080}'..='\u{10fffd}' => true,
        _ => false,
    });
    alt((is_bc_range, sesc))(b)
}

/// Parser for
///
/// ```text
/// BCHAR = %x20-26 / %x28-5B / %x5D-10FFFD / SESC / CRLF
/// ```
///
/// **Note:** RFC8610 Errata: 0x7f (delete) is disallowed
fn bchar(b: Buf) -> ParseResult<char> {
    let is_bc_range = char_pred(|c| match c {
        '\u{0020}'..='\u{0026}'
        | '\u{0028}'..='\u{005b}'
        | '\u{005d}'..='\u{007e}'
        | '\u{0080}'..='\u{10fffd}' => true,
        _ => false,
    });
    alt((is_bc_range, sesc, crlf))(b)
}

/// Parser for
///
/// ```text
/// SESC = "\" (%x20-7E / %x80-10FFFD)
/// ```
///
/// TODO: RFC8610 errata exist here, and are not covered in implementation.
fn sesc(b: Buf) -> ParseResult<char> {
    let (i, _) = char_is('\\')(b)?;
    char_pred(|c| match c {
        '\u{0020}'..='\u{007e}' | '\u{0080}'..='\u{10fffd}' => true,
        _ => false,
    })(i)
}

/// Parser for
///
/// ```text
/// EALPHA = ALPHA / "@" / "_" / "$"
/// ``
fn ealpha(b: Buf) -> ParseResult<char> {
    let is_at = char_is('@');
    let is_und = char_is('_');
    let is_dol = char_is('$');
    alt((alpha, is_at, is_und, is_dol))(b)
}

/// Parser for
///
/// ```text
/// ALPHA = %x41-5A / %x61-7A
/// ```
fn alpha(b: Buf) -> ParseResult<char> {
    let is_upper = char_pred(|c| ('A'..='Z').contains(&c));
    let is_lower = char_pred(|c| ('a'..='z').contains(&c));
    alt((is_upper, is_lower))(b)
}

/// Parser for
///
/// ```text
/// DIGIT = %x30-39
/// ```
fn digit(b: Buf) -> ParseResult<char> {
    char_pred(|c| ('0'..='9').contains(&c))(b)
}

/// Parser for
///
/// ```text
/// DIGIT1 = %x31-39
/// ```
fn digit1(b: Buf) -> ParseResult<char> {
    char_pred(|c| ('1'..='9').contains(&c))(b)
}

/// Parser for
///
/// ```text
/// HEXDIG = DIGIT / "A" / "B" / "C" / "D" / "E" / "F".
/// ```
///
/// Note that this parser is more permissive than the CDDL draft as it
/// allows lowercase 'a' to 'f' as Hex digits.
fn hexdig(b: Buf) -> ParseResult<char> {
    char_pred(|c| match c {
        '0'..='9' | 'a'..='f' | 'A'..='F' => true,
        _ => false,
    })(b)
}

/// Parser for
///
/// ```text
/// BINDIG = %x30-31
/// ```
fn bindig(b: Buf) -> ParseResult<char> {
    alt((char_is('0'), char_is('1')))(b)
}

/// Parser for
///
/// ```text
/// optcom = S ["," S]
/// ```
fn optcom(b: Buf) -> ParseResult<()> {
    let (i, _) = s(b)?;
    let (i, _) = opt(preceded(char_is(','), s))(i)?;
    return Ok((i, ()));
}

/// Parser for
///
/// ```text
/// S = *WS
/// ```
fn s(b: Buf) -> ParseResult<()> {
    let (i, _) = many0(ws)(b)?;
    Ok((i, ()))
}

/// Parser for
///
/// ```text
/// WS = SP / NL
/// ```
fn ws(b: Buf) -> ParseResult<char> {
    alt((sp, nl))(b)
}

/// Parser for
///
/// ```text
/// SP = %x20
/// ```
fn sp(b: Buf) -> ParseResult<char> {
    let (i, _) = context("SP = %x20", tag("\u{0020}"))(b)?;
    Ok((i, '\u{0020}'))
}

/// Parser for
///
/// ```text
/// NL = COMMENT / CRLF
/// ```
///
/// **Note:** This function always returns a CR, so that we have a char returned
fn nl(b: Buf) -> ParseResult<char> {
    let (i, _) = context("NL = COMMENT / CRLF", alt((comment, crlf)))(b)?;
    Ok((i, '\u{000A}'))
}

/// Parser for
///
/// ```text
/// COMMENT = ";" *PCHAR CRLF
/// ```
///
/// **Note:** This function always returns a CR, so that we have a char returned
fn comment(b: Buf) -> ParseResult<char> {
    let (i, _) = tag(";")(b)?;
    let (i, (_, _)) = many_till(pchar, crlf)(i)?;
    Ok((i, '\u{000A}'))
}

/// Parser for
///
/// ```text
/// PCHAR = %x20-7E / %x80-10FFFD
/// ```
fn pchar(b: Buf) -> ParseResult<char> {
    let (i, ch) = char_pred(|c| match c {
        '\u{0020}'..='\u{007E}' => true,
        '\u{0080}'..='\u{10FFFD}' => true,
        _ => false,
    })(b)?;
    Ok((i, ch))
}

/// Parser for
///
/// ```text
/// CRLF = %x0A / %x0D.0A
/// ```
///
/// **Note:** This function always returns a CR, so that we have a char returned
fn crlf(b: Buf) -> ParseResult<char> {
    let (i, _) = alt((tag("\u{000A}"), tag("\u{000D}\u{000A}")))(b)?;
    Ok((i, '\u{000A}'))
}

/**************************************************************************************************
 * Additional Parser Combinators
 *************************************************************************************************/

/// Matches a single character if the result of calling `_closure`
/// yields `true`, otherwise parse fails.
///
/// The Closure should be of type `Fn(char) -> bool`.
fn char_pred<C>(_closure: C) -> impl Fn(Buf) -> ParseResult<char>
where
    C: Fn(char) -> bool,
{
    move |b: Buf| match (b).iter_elements().next().map(|tst| {
        let fst_char = tst.as_char();
        let is_match = _closure(fst_char);
        (fst_char, is_match)
    }) {
        Some((c, true)) => Ok((b.slice(c.len()..), c)),
        _ => Err(Err::Error(ParseError::from_char(b, ' '))),
    }
}

/// For some reason, nom::char causes lots of errors (probably due to the collision
/// with the `char` reserved word). This is a specialised replacement.
fn char_is(c: char) -> impl Fn(Buf) -> ParseResult<char> {
    // Closure performing pattern match on input
    move |b: Buf| match (b).iter_elements().next().map(|tst| {
        let is_match = tst.as_char() == c;
        (&c, is_match)
    }) {
        Some((c, true)) => Ok((b.slice(c.len()..), c.as_char())),
        _ => Err(Err::Error(ParseError::from_char(b, c))),
    }
}

//*************************************************************************************************
// Unit Tests
//*************************************************************************************************
#[cfg(test)]
mod tests {
    use super::*;

    // rangeop = "..." / ".."
    #[test]
    fn rangeop_t() {
        assert_eq!(rangeop("..3"), Ok(("3", Operator::RangeIncl)));
        assert_eq!(rangeop("...3"), Ok(("3", Operator::RangeExcl)));
    }

    // ctlop = "." id
    #[test]
    fn ctlop_t() {
        assert_eq!(
            ctlop(".foobar baz"),
            Ok((" baz", Operator::Control("foobar".to_string())))
        );
        assert_ne!(
            ctlop(".&foobar baz"),
            Ok((" baz", Operator::Control("&foobar".to_string())))
        );
    }
    // occur = [uint] "*" [uint] / "+" / "?"
    #[test]
    fn occur_t() {
        assert_eq!(occur("*foobar"), Ok(("foobar", Occurs::ZeroPlus)));
        assert_eq!(occur("3*foobar"), Ok(("foobar", Occurs::Between(3, i64::MAX))));
        assert_eq!(occur("*3foobar"), Ok(("foobar", Occurs::Between(0, 3))));
        assert_eq!(occur("42*54foobar"), Ok(("foobar", Occurs::Between(42, 54 ))));
        assert_eq!(occur("?foobar"), Ok(("foobar", Occurs::Optional)));
        assert_eq!(occur("+foobar"), Ok(("foobar", Occurs::OnePlus)));
        assert_eq!(occur("0*1foobar"), Ok(("foobar", Occurs::Optional)));
    }

    // value = number / text / bytes
    #[test]
    fn value_t() {
        assert_eq!(value("123 abc"), Ok((" abc", Value::Int(123))));
        assert_eq!(
            value("\"abc123ABC\u{020}\u{05b}\u{07e}\"abcd"),
            Ok((
                "abcd",
                Value::Tstr("abc123ABC\u{020}\u{05b}\u{07e}".to_string())
            ))
        );
        assert_eq!(
            value("'\u{020}\u{026}\u{023}\u{05b}\u{05d}\u{07e}\u{080}\u{10ffd}\\\u{022}'abc"),
            Ok((
                "abc",
                Value::Bytes(
                    [
                        0x20_u8, 0x26, 0x23, 0x5b, 0x5d, 0x7e, 0xc2, 0x80, 0xf0, 0x90, 0xbf, 0xbd,
                        0x22
                    ]
                    .to_vec()
                )
            ))
        );
    }

    // number = hexfloat / (int ["." fraction] ["e" exponent])
    #[test]
    fn number_t() {
        // TODO: no test for hexfloat until it actually works...
        assert_eq!(number("123 abc"), Ok((" abc", Value::Int(123))));
        assert_eq!(number("-123 abc"), Ok((" abc", Value::Int(-123))));
        assert_eq!(number("123.45 abc"), Ok((" abc", Value::Float(123.45))));
        assert_eq!(number("-123.45 abc"), Ok((" abc", Value::Float(-123.45))));
        assert_eq!(number("-123e-2 abc"), Ok((" abc", Value::Float(-123e-2))));
        assert_eq!(number("-123e2 abc"), Ok((" abc", Value::Float(-123e2))));
        assert_eq!(number("-123e+2 abc"), Ok((" abc", Value::Float(-123e+2))));
        assert_eq!(
            number("-123.45e+2 abc"),
            Ok((" abc", Value::Float(-123.45e+2)))
        );
    }

    // fraction = 1*DIGIT
    #[test]
    fn fraction_t() {
        assert_eq!(fraction("0z"), Ok(("z", "0".to_string())));
        assert_eq!(fraction("9z"), Ok(("z", "9".to_string())));
        assert_eq!(fraction("1234567890z"), Ok(("z", "1234567890".to_string())));
        assert_ne!(fraction("z123"), Ok(("z", "123".to_string())));
    }
    /* Test fails
    #[test]
    fn hexfloat_t() {
        assert_eq!(
            hexfloat("0x1.00p+1zzz"),
            Ok(("zzz", ParseValue::Float(2f64))),
        );
    }
    */

    // exponent = ["+"/"-"] 1*DIGIT
    #[test]
    fn exponent_t() {
        assert_eq!(exponent("1234abc"), Ok(("abc", "+1234".to_string())));
        assert_eq!(exponent("+1234abc"), Ok(("abc", "+1234".to_string())));
        assert_eq!(exponent("-1234abc"), Ok(("abc", "-1234".to_string())));
        assert_ne!(exponent("-abc"), Ok(("abc", "-1234".to_string())));
    }

    // text = %x22 *SCHAR %x22
    #[test]
    fn text_t() {
        assert_eq!(
            text("\"abc123ABC\u{020}\u{05b}\u{07e}\"abcd"),
            Ok((
                "abcd",
                Value::Tstr("abc123ABC\u{020}\u{05b}\u{07e}".to_string())
            ))
        );
        assert_ne!(
            text("\"abc123ABC\u{020}\u{05b}\u{07f}\"abcd"),
            Ok((
                "abcd",
                Value::Tstr("abc123ABC\u{020}\u{05b}\u{07f}".to_string())
            ))
        );
    }

    // bytes = [bsqual] %x27 *BCHAR %x27
    #[test]
    fn bytes_t() {
        assert_eq!(
            // NB: UTF8 warning - particularly evil example case used below, as it checks the
            // edge cases for all of the legal values for bytes.`
            // U+80 is encoded 0xC2, 0x80
            // U+10FFD is encoded 0xF0, 0x90, 0xBF, 0xBD
            bytes("'\u{020}\u{026}\u{023}\u{05b}\u{05d}\u{07e}\u{080}\u{10ffd}\\\u{022}'abc"),
            Ok((
                "abc",
                Value::Bytes(
                    [
                        0x20_u8, 0x26, 0x23, 0x5b, 0x5d, 0x7e, 0xc2, 0x80, 0xf0, 0x90, 0xbf, 0xbd,
                        0x22
                    ]
                    .to_vec()
                )
            ))
        );
    }

    // bsqual = "h" / "b64"
    #[test]
    fn bsqual_t() {
        assert_eq!(bsqual("h'1234'"), Ok(("'1234'", BsQual::ByteStr)));
        assert_eq!(bsqual("b64'1234'"), Ok(("'1234'", BsQual::Base64)));
        assert_ne!(bsqual("b65'1234'"), Ok(("'1234'", BsQual::Base64)));
    }

    // id = EALPHA *(*("-" / ".") (EALPHA / DIGIT))
    #[test]
    fn id_t() {
        assert_eq!(id("abc-x.foo31 "), Ok((" ", "abc-x.foo31".to_string())));
    }

    // SCHAR = %x20-21 / %x23-5B / %x5D-7E / %x80-10FFFD / SESC
    #[test]
    fn schar_t() {
        assert_eq!(schar("\u{020}abc"), Ok(("abc", '\u{020}')));
        assert_eq!(schar("\u{023}abc"), Ok(("abc", '\u{023}')));
        assert_eq!(schar("\u{07e}abc"), Ok(("abc", '\u{07e}')));
        assert_eq!(schar("\u{080}abc"), Ok(("abc", '\u{080}')));
        assert_eq!(schar("\\\u{031f1}abc"), Ok(("abc", '\u{031f1}')));
        assert_eq!(schar("\u{10fffd}abc"), Ok(("abc", '\u{10fffd}')));
        assert_ne!(bchar("\u{027}abc"), Ok(("abc", '\u{027}')));
    }

    // BCHAR = %x20-26 / %x28-5B / %x5D-10FFFD / SESC / CRLF
    #[test]
    fn bchar_t() {
        assert_eq!(bchar("\u{020}abc"), Ok(("abc", '\u{020}')));
        assert_eq!(bchar("\u{026}abc"), Ok(("abc", '\u{026}')));
        assert_eq!(bchar("\u{028}abc"), Ok(("abc", '\u{028}')));
        assert_eq!(bchar("\u{05d}abc"), Ok(("abc", '\u{05d}')));
        assert_eq!(bchar("\u{10fffd}abc"), Ok(("abc", '\u{10fffd}')));
        assert_ne!(bchar("\u{05c}abc"), Ok(("abc", '\u{05c}')));
    }

    // SESC = "\" (%x20-7E / %x80-10FFFD)
    #[test]
    fn sesc_t() {
        assert_eq!(sesc("\\\u{003c}abc"), Ok(("abc", '\u{003c}')));
        assert_eq!(sesc("\\\u{00a5}abc"), Ok(("abc", '\u{00a5}')));
        assert_eq!(sesc("\\\u{031f1}abc"), Ok(("abc", '\u{031f1}')));
        assert_ne!(sesc("\\\u{0019}abc"), Ok(("abc", '\u{0019}')));
    }

    // EALPHA = ALPHA / "@" / "_" / "$"
    #[test]
    fn ealpha_t() {
        assert_eq!(ealpha("aabc"), Ok(("abc", 'a')));
        assert_eq!(ealpha("_abc"), Ok(("abc", '_')));
        assert_eq!(ealpha("$abc"), Ok(("abc", '$')));
        assert_eq!(ealpha("@abc"), Ok(("abc", '@')));
        assert_ne!(ealpha("0abc"), Ok(("abc", '0')));
    }

    // ALPHA = %x41-5A / %x61-7A
    #[test]
    fn alpha_t() {
        assert_eq!(alpha("aabc"), Ok(("abc", 'a')));
        assert_eq!(alpha("zabc"), Ok(("abc", 'z')));
        assert_eq!(alpha("Aabc"), Ok(("abc", 'A')));
        assert_eq!(alpha("Zabc"), Ok(("abc", 'Z')));
        assert_ne!(alpha("0abc"), Ok(("abc", '0')));
    }

    // DIGIT = %x30-39
    #[test]
    fn digit_t() {
        assert_eq!(digit("0abc"), Ok(("abc", '0')));
        assert_eq!(digit("4abc"), Ok(("abc", '4')));
        assert_eq!(digit("9abc"), Ok(("abc", '9')));
        assert_ne!(digit("aabc"), Ok(("abc", 'a')));
    }

    // DIGIT1 = %x31-39
    #[test]
    fn digit1_t() {
        assert_eq!(digit1("1abc"), Ok(("abc", '1')));
        assert_eq!(digit1("6abc"), Ok(("abc", '6')));
        assert_eq!(digit1("9abc"), Ok(("abc", '9')));
        assert_ne!(digit1("0abc"), Ok(("abc", '0')));
    }

    #[test]
    fn hexdig_t() {
        assert_eq!(hexdig("abcde"), Ok(("bcde", 'a')));
        assert_eq!(hexdig("CDEFG"), Ok(("DEFG", 'C')));
        assert_eq!(hexdig("3zzQa"), Ok(("zzQa", '3')));
        assert_eq!(hexdig("9zzQa"), Ok(("zzQa", '9')));
        assert_eq!(hexdig("0zzQa"), Ok(("zzQa", '0')));
        assert_ne!(hexdig("HzzQa"), Ok(("zzQa", 'H')));
    }

    #[test]
    fn bindig_t() {
        assert_eq!(bindig("1011"), Ok(("011", '1')));
        assert_eq!(bindig("0101"), Ok(("101", '0')));
        assert_ne!(bindig("a101"), Ok(("101", 'a')));
    }

    #[test]
    fn s_t() {
        assert_eq!(s(" \n \r\n;foo\nabc"), Ok(("abc", ())));
    }

    #[test]
    fn ws_t() {
        assert_eq!(sp(" abcd"), Ok(("abcd", ' ')));
        assert_eq!(nl("\nabcd"), Ok(("abcd", '\n')));
        assert_eq!(nl("\r\nabcd"), Ok(("abcd", '\n')));
        assert_ne!(sp("abcd"), Ok(("bcd", 'a')));
    }

    #[test]
    fn sp_t() {
        assert_eq!(sp(" abcd"), Ok(("abcd", ' ')));
    }

    #[test]
    fn nl_t() {
        let result = nl("\nabcd");
        assert_eq!(result, Ok(("abcd", '\n')));
        let result = nl("\r\nabcd");
        assert_eq!(result, Ok(("abcd", '\n')))
    }

    #[test]
    fn comment_t() {
        let result = comment("; foobar\nbaz");
        assert_eq!(result, Ok(("baz", '\n')))
    }

    #[test]
    fn crlf_t() {
        let result = crlf("\nabc");
        assert_eq!(result, Ok(("abc", '\n')))
    }
    #[test]
    fn pchar_t() {
        assert_eq!(pchar("abc"), Ok(("bc", 'a')));
        assert_eq!(pchar("😂🤣🤪😎"), Ok(("🤣🤪😎", '😂')));
        assert_ne!(pchar("\u{007f}abc"), Ok(("abc", '\u{007f}')));
        assert_ne!(pchar("\u{10FFFF}abc"), Ok(("abc", '\u{10FFFF}')));
    }

    #[test]
    fn char_pred_t() {
        let result = char_pred(|_| true)("abc");
        assert_eq!(result, Ok(("bc", 'a')));
    }

    // Longer test cases
}
