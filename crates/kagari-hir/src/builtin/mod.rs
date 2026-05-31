pub mod array;
pub mod iterable;
pub mod surface;

use crate::types::{BuiltinType, TypeId};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuiltinFunction {
    TypeOf,
    GetField,
    SetField,
    SetIndex,
    Print,
}

impl BuiltinFunction {
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "type_of" => Some(Self::TypeOf),
            "get_field" => Some(Self::GetField),
            "set_field" => Some(Self::SetField),
            "set_index" => Some(Self::SetIndex),
            "print" => Some(Self::Print),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BuiltinMethod {
    Array(array::Method),
    Iterable(iterable::Method),
    String(StringMethod),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuiltinMethodSpec {
    pub name: &'static str,
    pub arity: usize,
    pub result: BuiltinMethodResult,
    pub mutates_receiver: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuiltinMethodResult {
    Unit,
    Receiver,
    ArrayElement,
    IterableElement,
    Builtin(BuiltinType),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum StringMethod {
    Len,
}

const STRING_LEN_SPEC: BuiltinMethodSpec = BuiltinMethodSpec {
    name: "len",
    arity: 0,
    result: BuiltinMethodResult::Builtin(BuiltinType::USize),
    mutates_receiver: false,
};

fn lookup_string_method(name: &str) -> Option<BuiltinMethod> {
    match name {
        "len" => Some(BuiltinMethod::String(StringMethod::Len)),
        _ => None,
    }
}

impl BuiltinMethod {
    pub fn resolve(receiver: &TypeId, name: &str) -> Option<Self> {
        match receiver {
            TypeId::Array(_) => array::lookup_method(name),
            TypeId::Builtin(BuiltinType::String) => lookup_string_method(name),
            _ => None,
        }
    }

    pub fn owner_name(self) -> &'static str {
        match self {
            Self::Array(_) => "array",
            Self::Iterable(_) => "iterable",
            Self::String(_) => "String",
        }
    }

    pub fn spec(self) -> &'static BuiltinMethodSpec {
        match self {
            Self::Array(method) => array::method_spec(method),
            Self::Iterable(method) => iterable::method_spec(method),
            Self::String(StringMethod::Len) => &STRING_LEN_SPEC,
        }
    }
}
