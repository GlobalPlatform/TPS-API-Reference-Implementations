/***************************************************************************************************
 * Copyright (c) 2020-2021 Jeremy O'Donoghue. All rights reserved.
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
/// Abstract Syntax Tree for IETF CDDL [RFC8610](https://www.rfc-editor.org/info/rfc8610)
///
/// This file provides a suitable Abstract Syntax Tree for the ABNF grammar defined in RFC8610,
/// Appendix B.

/// `Rule` is the top-level
#[derive(PartialEq, Debug)]
pub enum Rule {
    TypeDef(String, Option<GenericParam>, Assignment, Box<Type>),
    GroupDef(String, Option<GenericParam>, Assignment, Box<GroupItem>),
}

/// AST entry representing a type definition.
#[derive(PartialEq, Debug, Clone)]
pub enum Type {
    Value(Value),
    Rule(String, Option<Vec<Type>>),
    Types(Vec<Type>),
    GroupMap(Group),
    GroupArray(Group),
    Unwrap(String, Option<Vec<Type>>),
    GroupEnum(Group),
    GroupNameEnum(String, Option<Vec<Type>>),
    Tagged(Option<i64>, Box<Type>),
    Major(i64, Option<i64>),
    Combined(Box<Type>, Box<Type>, Operator),
    Any,
}

/// CDDL is a vector of Rules
pub type CDDL = Vec<Rule>;

/// A group is a vector of GroupItem
pub type Group = Vec<GroupItem>;

/// A genericparm is a vector of identifier names
pub type GenericParam = Vec<String>;

/// Operators on types
#[derive(PartialEq, Debug, Clone)]
pub enum Operator {
    RangeIncl,
    RangeExcl,
    Control(String),
}

/// A single item in a group definition
#[derive(PartialEq, Debug, Clone)]
pub enum GroupItem {
    Key(Option<Box<MemberKey>>, Type, Occurs),
    Name(String, Occurs, Option<Vec<Type>>),
    Grp(Group, Occurs),
}

/// Group member key values
#[derive(PartialEq, Debug, Clone)]
pub enum MemberKey {
    FromType(Box<Type>, bool),
    FromValue(Box<Value>),
}

/// Tokens qualifying bytestring format used
#[derive(PartialEq, Debug)]
pub(crate) enum BsQual {
    ByteStr,
    Base64,
}

/// Assignment tokens
#[derive(PartialEq, Debug, Copy, Clone)]
pub enum Assignment {
    Assign,
    AssignExtend,
}

/// Occurence
#[derive(PartialEq, Eq, Debug, Copy, Clone)]
pub enum Occurs {
    Once,
    Optional,
    ZeroPlus,
    OnePlus,
    Between(i64, i64)
}

/// Values in CDDL
#[derive(PartialEq, Debug, Clone)]
pub enum Value {
    Bytes(Vec<u8>),
    Tstr(String),
    Int(i64),
    Float(f64),
}
