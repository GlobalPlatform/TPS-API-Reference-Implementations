/***************************************************************************************************
 * Copyright (c) 2019-2021 Jeremy O'Donoghue. All rights reserved.
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

pub mod ast;
pub mod hexfloat;
pub mod parse;

pub use ast::{
    Assignment, GenericParam, Group, GroupItem, MemberKey, Occurs, Operator, Value, Rule,
    Type, CDDL,
};
pub use parse::cddl;
use std::fs;
use thiserror::Error;
use std::rc::Rc;

pub fn read(with_prelude: bool, path: Rc<String>) -> Result<CDDL, CDDLParseError> {
    let prelude = "
    any = #
    
    uint = #0
    nint = #1
    int = uint / nint
    
    bstr = #2
    bytes = bstr
    tstr = #3
    text = tstr
    
    tdate = #6.0(tstr)
    
    time = #6.1(number)
    number = int / float
    biguint = #6.2(bstr)
    bignint = #6.3(bstr)
    bigint = biguint / bignint
    integer = int / bigint
    unsigned = uint / biguint
    decfrac = #6.4([e10: int, m: integer])
    bigfloat = #6.5([e2: int, m: integer])
    eb64url = #6.21(any)
    eb64legacy = #6.22(any)
    eb16 = #6.23(any)
    
    encoded-cbor = #6.24(bstr)
    uri = #6.32(tstr)
    b64url = #6.33(tstr)
    b64legacy = #6.34(tstr)
    regexp = #6.35(tstr)
    mime-message = #6.36(tstr)
    cbor-any = #6.55799(any)
    
    float16 = #7.25
    float32 = #7.26
    float64 = #7.27
    float16-32 = float16 / float32
    float32-64 = float32 / float64
    float = float16-32 / float64
    
    false = #7.20
    true = #7.21
    bool = false / true
    nil = #7.22
    null = nil
    undefined = #7.23"
        .to_string();
    let file_ast = read_cddl(Rc::clone(&path))?;
    if with_prelude {
        let prelude_ast = match cddl(&prelude) {
            Ok((_, rules)) => Ok(rules),
            Err(_) => Err(CDDLParseError::ParseError(
                0,
                0,
                "Error reading Prelude - unrecoverable".to_string(),
            )),
        };
        match prelude_ast {
            Ok(mut prelude_rules) => {
                for rule in file_ast {
                    prelude_rules.push(rule);
                }
                Ok(prelude_rules)
            }
            err => err,
        }
    } else {
        Ok(file_ast)
    }
}

fn read_cddl(path: Rc<String>) -> Result<CDDL, CDDLParseError> {
    let rc_path = path.clone();
    let text_or_err = fs::read_to_string(rc_path.as_str());
    match text_or_err {
        Ok(text) =>
            match cddl(&text) {
                Ok((_, rules)) => Ok(rules),
                Err(e) => {
                    match e {
                        nom::Err::Incomplete(needed) => {
                            println!("CDDL parse failed because buffer exhausted. Need {:?} bytes", needed);
                            Err(CDDLParseError::Incomplete)
                        },
                        nom::Err::Error(e) => {
                            println!("CDDL errors:  {}", nom::error::convert_error::<&str>(&text, e));
                            Err(CDDLParseError::ParseError(0, 0, "Oh shit!".to_string()))
                        },
                        nom::Err::Failure(e) => {
                            println!("CDDL errors:  {}", nom::error::convert_error::<&str>(&text, e));
                            Err(CDDLParseError::ParseError(0, 0, "Oh shit!".to_string()))
                        }
                    }
                }
            },
        Err(_) => Err(CDDLParseError::NoFile),
        //Err(_) => Err(CDDLParseError::ReadFail(path)),
    }
}

#[derive(Debug, Error, PartialEq)]
pub enum CDDLParseError {
    #[error("Error parsing CDDL")]
    ParseError(u32, u32, String),
    #[error("An unexplained fatal error occurred")]
    ShitHappened,
    #[error("No filename was provided")]
    NoFile,
    #[error("Unexpected end of file")]
    Incomplete,
}
