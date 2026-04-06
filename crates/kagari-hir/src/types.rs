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

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TypeId {
    Builtin(BuiltinType),
    Tuple(Vec<TypeId>),
    Array(Box<TypeId>),
    Struct(String),
    Enum(String),
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

    pub fn display_name(&self) -> String {
        match self {
            Self::Builtin(BuiltinType::Unit) => "unit".to_owned(),
            Self::Builtin(BuiltinType::Bool) => "bool".to_owned(),
            Self::Builtin(BuiltinType::I32) => "i32".to_owned(),
            Self::Builtin(BuiltinType::I64) => "i64".to_owned(),
            Self::Builtin(BuiltinType::F32) => "f32".to_owned(),
            Self::Builtin(BuiltinType::F64) => "f64".to_owned(),
            Self::Builtin(BuiltinType::Str) => "str".to_owned(),
            Self::Tuple(elements) => {
                let inner = elements
                    .iter()
                    .map(TypeId::display_name)
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("({inner})")
            }
            Self::Array(element) => format!("[{}]", element.display_name()),
            Self::Struct(name) | Self::Enum(name) => name.clone(),
        }
    }

    pub fn is_heap_backed(&self) -> bool {
        match self {
            Self::Builtin(_) => false,
            Self::Tuple(_) | Self::Array(_) | Self::Struct(_) | Self::Enum(_) => true,
        }
    }
}
