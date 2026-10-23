//! Closed scalar and native representation facts; no source declaration catalog.
use crate::scalar::BuiltinType;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuiltinTypeFamily {
    Never,
    Unit,
    Boolean,
    SignedInteger,
    UnsignedInteger,
    Float,
    String,
    Character,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuiltinTypeSpec {
    pub ty: BuiltinType,
    pub name: &'static str,
    pub family: BuiltinTypeFamily,
    pub const_safe: bool,
    pub heap_backed: bool,
}

const BUILTIN_TYPES: &[BuiltinTypeSpec] = &[
    BuiltinTypeSpec {
        ty: BuiltinType::Never,
        name: "!",
        family: BuiltinTypeFamily::Never,
        const_safe: false,
        heap_backed: false,
    },
    BuiltinTypeSpec {
        ty: BuiltinType::Unit,
        name: "()",
        family: BuiltinTypeFamily::Unit,
        const_safe: true,
        heap_backed: false,
    },
    BuiltinTypeSpec {
        ty: BuiltinType::Bool,
        name: "bool",
        family: BuiltinTypeFamily::Boolean,
        const_safe: true,
        heap_backed: false,
    },
    BuiltinTypeSpec {
        ty: BuiltinType::I8,
        name: "i8",
        family: BuiltinTypeFamily::SignedInteger,
        const_safe: true,
        heap_backed: false,
    },
    BuiltinTypeSpec {
        ty: BuiltinType::I16,
        name: "i16",
        family: BuiltinTypeFamily::SignedInteger,
        const_safe: true,
        heap_backed: false,
    },
    BuiltinTypeSpec {
        ty: BuiltinType::I32,
        name: "i32",
        family: BuiltinTypeFamily::SignedInteger,
        const_safe: true,
        heap_backed: false,
    },
    BuiltinTypeSpec {
        ty: BuiltinType::I64,
        name: "i64",
        family: BuiltinTypeFamily::SignedInteger,
        const_safe: true,
        heap_backed: false,
    },
    BuiltinTypeSpec {
        ty: BuiltinType::U8,
        name: "u8",
        family: BuiltinTypeFamily::UnsignedInteger,
        const_safe: true,
        heap_backed: false,
    },
    BuiltinTypeSpec {
        ty: BuiltinType::U16,
        name: "u16",
        family: BuiltinTypeFamily::UnsignedInteger,
        const_safe: true,
        heap_backed: false,
    },
    BuiltinTypeSpec {
        ty: BuiltinType::U32,
        name: "u32",
        family: BuiltinTypeFamily::UnsignedInteger,
        const_safe: true,
        heap_backed: false,
    },
    BuiltinTypeSpec {
        ty: BuiltinType::U64,
        name: "u64",
        family: BuiltinTypeFamily::UnsignedInteger,
        const_safe: true,
        heap_backed: false,
    },
    BuiltinTypeSpec {
        ty: BuiltinType::F32,
        name: "f32",
        family: BuiltinTypeFamily::Float,
        const_safe: true,
        heap_backed: false,
    },
    BuiltinTypeSpec {
        ty: BuiltinType::F64,
        name: "f64",
        family: BuiltinTypeFamily::Float,
        const_safe: true,
        heap_backed: false,
    },
    BuiltinTypeSpec {
        ty: BuiltinType::String,
        name: "String",
        family: BuiltinTypeFamily::String,
        const_safe: false,
        heap_backed: true,
    },
    BuiltinTypeSpec {
        ty: BuiltinType::Char,
        name: "char",
        family: BuiltinTypeFamily::Character,
        const_safe: true,
        heap_backed: false,
    },
];

pub fn builtin_types() -> &'static [BuiltinTypeSpec] {
    BUILTIN_TYPES
}

pub fn builtin_type(name: &str) -> Option<BuiltinType> {
    builtin_types()
        .iter()
        .find(|spec| spec.name == name)
        .map(|spec| spec.ty)
}

pub fn builtin_type_spec(ty: BuiltinType) -> Option<&'static BuiltinTypeSpec> {
    builtin_types().iter().find(|spec| spec.ty == ty)
}
