pub mod array;

use crate::types::TypeId;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BuiltinMethod {
    Array(array::Method),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuiltinMethodSpec {
    pub name: &'static str,
    pub arity: usize,
}

impl BuiltinMethod {
    pub fn resolve(receiver: &TypeId, name: &str) -> Option<Self> {
        match receiver {
            TypeId::Array(_) => array::lookup_method(name),
            _ => None,
        }
    }

    pub fn owner_name(self) -> &'static str {
        match self {
            Self::Array(_) => "array",
        }
    }

    pub fn spec(self) -> &'static BuiltinMethodSpec {
        match self {
            Self::Array(method) => array::method_spec(method),
        }
    }
}
