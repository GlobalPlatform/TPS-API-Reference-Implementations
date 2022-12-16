/***************************************************************************************************
 * Copyright (c) 2021 Jeremy O'Donoghue. All rights reserved.
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
 * rs_cddl utility which will eventually support code generation from CDDL
 **************************************************************************************************/
mod ir;
mod error;

extern crate tps_cddl;
extern crate clap;
extern crate thiserror;

use tps_cddl::cddl::*;

use clap::{App, Arg};
use std::error::Error;
use std::rc::Rc;
use crate::error::CddlError;

use crate::ir::{IRStore};

fn main() -> Result<(), Box<dyn Error>> {
    let cmd_line = App::new("cddlgen")
        .version(env!("CARGO_PKG_VERSION"))
        .author("Jeremy O'Donoghue <quic_jodonogh@quicinc.com")
        .about("Generate Rust code capable of serializing and deserializing CBOR matching CDDL")
        .arg(
            Arg::with_name("cddl")
                .short("c")
                .long("cddl")
                .required(true)
                .takes_value(true)
                .value_name("CDDL_FILE")
                .help("Path to CDDL file")
        )
        .arg(
            Arg::with_name("prelude")
                .short("p")
                .long("with-prelude")
                .required(false)
                .takes_value(false)
                .help("Include the CDDL prelude definitions")
        )
        .get_matches();

    let with_prelude = cmd_line.is_present("prelude");
    let rc_filename = Rc::new(cmd_line.value_of("cddl").unwrap().to_string());
    let ast = read(with_prelude, Rc::clone(&rc_filename))?;
    let mut ir = IRStore::new();
    pass1(&mut ir, &ast)?;

    Ok(println!("Completed! {:?}", ir))
}

fn pass1<'a, 'b>(ir: &'a mut IRStore, ast: &'b CDDL) -> Result<(), CddlError> where 'b : 'a {
    for item in ast {
        match item {
            Rule::TypeDef(s, None, Assignment::Assign, typ) => {
                // In this case it is an error for the key to exist already
                ir.try_insert(s,  typ)?
            },
            Rule::TypeDef(s, None, Assignment::AssignExtend, typ) => {
                ir.update(s, typ)
            },
            _ => ()
        }
    }
    Ok(())
}
