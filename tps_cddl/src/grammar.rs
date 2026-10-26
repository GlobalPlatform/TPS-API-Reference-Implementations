/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
use pest_derive::Parser;

#[derive(Parser)]
#[grammar = "grammar.pest"]
pub struct CddlParser;
