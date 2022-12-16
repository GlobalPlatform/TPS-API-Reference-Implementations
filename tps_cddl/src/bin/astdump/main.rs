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
 * rs_cddl utility binary for dumping, in debug format, the AST generated from parsing a CDDL
 * file.
 *
 * This can be useful for:
 * - Determining whether a CDDL file is syntactically correct
 * - Investigation and analysis of how code generation will work
 **************************************************************************************************/
extern crate tps_cddl;
extern crate clap;
extern crate thiserror;

use tps_cddl::cddl::*;

use clap::{App, Arg};
use std::error::Error;
use std::rc::Rc;

fn main() -> Result<(), Box<dyn Error>> {
    let cmd_line = App::new("astdump")
        .version(env!("CARGO_PKG_VERSION"))
        .author("Jeremy O'Donoghue <quic_jodonogh@quicinc.com")
        .about("Dump a CDDL file Abstract Syntax Tree")
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
                .help("Include the CDDL prelude in parsed AST")
        )
        .get_matches();

    let with_prelude = cmd_line.is_present("prelude");
    let rc_filename = Rc::new(cmd_line.value_of("cddl").unwrap().to_string());
    let ast = read(with_prelude, Rc::clone(&rc_filename))?;
    Ok(println!("CDDL = {:?}", ast))
}
