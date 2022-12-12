/***************************************************************************************************
 * Copyright (c) 2021 Jeremy O'Donoghue. All rights reserved.
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
 * Intermediate representation used to post-process from the AST
 **************************************************************************************************/
use std::collections::HashMap;
use tps_cddl::cddl::{Type, Value};
use crate::error::CddlError;

#[derive(Debug)]
pub struct IRStore {
    store: HashMap<String, IR>
}

impl IRStore {
    /// Create an instance of IRStore
    pub fn new() -> IRStore {
        IRStore {
            store: HashMap::new()
        }
    }

    /// Insert or update the value associated with a key. We append to existing values
    /// if required.
    pub fn update(&mut self, k: &String, v: &Box<Type>) {
        if let Some(old_ir) = self.store.get(k) {
            match &old_ir {
                IR::Values(vs) => {
                    match &**v {
                        Type::Value(v) => {
                            // TODO: This is horribly inefficient - find a way to avoid cloning vs
                            let mut new_vs = vs.clone();
                            new_vs.push(v.clone());
                            let _ = self.store.insert(k.clone(), IR::Values(new_vs));
                        },
                        _ => ()
                    }
                },
            }
        } else {
            // Simple case
            println!("Update k: {:?}, v:{:?}", k, *v);
            match &**v {
                Type::Value(val) => {
                    let mut vs = Vec::new();
                    vs.push(val.clone());
                    let _ = self.store.insert(k.clone(), IR::Values(vs));
                },
                Type::Types(ts) => {

                }
                _ => ()
            }
        }
        ()
    }

    pub fn try_insert(&mut self, k: &String, v: &Box<Type>) -> Result<(), CddlError> {
        if !self.contains(k) {
            Ok(self.update(k, v))
        } else {
            Err(CddlError::ReassignmentError(k.clone()))
        }

    }

    pub fn contains(&self, k: &String) -> bool {
        self.store.contains_key(k)
    }
}

#[derive(Debug)]
pub enum IR {
    Values(Vec<Value>)
}
