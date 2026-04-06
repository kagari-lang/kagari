#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BuiltinType {
    Unit,
    Bool,
    I32,
    I64,
    F32,
    F64,
    Str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TypeId {
    Builtin(BuiltinType),
}

impl TypeId {
    pub fn from_name(name: &str) -> Option<Self> {
        let builtin = match name {
            "unit" => BuiltinType::Unit,
            "bool" => BuiltinType::Bool,
            "i32" => BuiltinType::I32,
            "i64" => BuiltinType::I64,
            "f32" => BuiltinType::F32,
            "f64" => BuiltinType::F64,
            "str" => BuiltinType::Str,
            _ => return None,
        };
        Some(Self::Builtin(builtin))
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::Builtin(BuiltinType::Unit) => "unit",
            Self::Builtin(BuiltinType::Bool) => "bool",
            Self::Builtin(BuiltinType::I32) => "i32",
            Self::Builtin(BuiltinType::I64) => "i64",
            Self::Builtin(BuiltinType::F32) => "f32",
            Self::Builtin(BuiltinType::F64) => "f64",
            Self::Builtin(BuiltinType::Str) => "str",
        }
    }
}
