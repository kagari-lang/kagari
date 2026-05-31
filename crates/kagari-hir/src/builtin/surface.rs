use crate::types::{BuiltinType, TypeId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuiltinTypeFamily {
    Unit,
    Boolean,
    SignedInteger,
    UnsignedInteger,
    Float,
    String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuiltinTypeSpec {
    pub ty: BuiltinType,
    pub name: &'static str,
    pub family: BuiltinTypeFamily,
    pub const_safe: bool,
    pub heap_backed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StandardEnum {
    Option,
    Result,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StandardVariantSpec {
    pub name: &'static str,
    pub payload_arity: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StandardEnumSpec {
    pub kind: StandardEnum,
    pub name: &'static str,
    pub arity: usize,
    pub variants: &'static [StandardVariantSpec],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IterableProtocol {
    Array { item: TypeId },
    String { item: BuiltinType },
}

const BUILTIN_TYPES: &[BuiltinTypeSpec] = &[
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
        ty: BuiltinType::ISize,
        name: "isize",
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
        ty: BuiltinType::USize,
        name: "usize",
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
];

const OPTION_VARIANTS: &[StandardVariantSpec] = &[
    StandardVariantSpec {
        name: "Some",
        payload_arity: 1,
    },
    StandardVariantSpec {
        name: "None",
        payload_arity: 0,
    },
];

const RESULT_VARIANTS: &[StandardVariantSpec] = &[
    StandardVariantSpec {
        name: "Ok",
        payload_arity: 1,
    },
    StandardVariantSpec {
        name: "Err",
        payload_arity: 1,
    },
];

const STANDARD_ENUMS: &[StandardEnumSpec] = &[
    StandardEnumSpec {
        kind: StandardEnum::Option,
        name: "Option",
        arity: 1,
        variants: OPTION_VARIANTS,
    },
    StandardEnumSpec {
        kind: StandardEnum::Result,
        name: "Result",
        arity: 2,
        variants: RESULT_VARIANTS,
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

pub fn standard_enums() -> &'static [StandardEnumSpec] {
    STANDARD_ENUMS
}

pub fn standard_enum(name: &str) -> Option<&'static StandardEnumSpec> {
    standard_enums().iter().find(|spec| spec.name == name)
}

pub fn standard_enum_type(name: &str, args: Vec<TypeId>) -> Option<TypeId> {
    let spec = standard_enum(name)?;
    (args.len() == spec.arity).then(|| TypeId::StandardEnum {
        name: spec.name.to_owned(),
        args,
    })
}

pub fn is_numeric(ty: &TypeId) -> bool {
    matches!(
        builtin_family(ty),
        Some(
            BuiltinTypeFamily::SignedInteger
                | BuiltinTypeFamily::UnsignedInteger
                | BuiltinTypeFamily::Float
        )
    )
}

pub fn supports_unary_negation(ty: &TypeId) -> bool {
    matches!(
        builtin_family(ty),
        Some(BuiltinTypeFamily::SignedInteger | BuiltinTypeFamily::Float)
    )
}

pub fn supports_arithmetic(lhs: &TypeId, rhs: &TypeId) -> bool {
    lhs == rhs && is_numeric(lhs)
}

pub fn supports_ordering(lhs: &TypeId, rhs: &TypeId) -> bool {
    supports_arithmetic(lhs, rhs)
}

pub fn supports_boolean_logic(lhs: &TypeId, rhs: &TypeId) -> bool {
    lhs == &TypeId::Builtin(BuiltinType::Bool) && rhs == &TypeId::Builtin(BuiltinType::Bool)
}

pub fn supports_const_type(ty: &TypeId) -> bool {
    match ty {
        TypeId::Builtin(builtin) => builtin_type_spec(*builtin).is_some_and(|spec| spec.const_safe),
        _ => false,
    }
}

pub fn iterable_protocol(ty: &TypeId) -> Option<IterableProtocol> {
    match ty {
        TypeId::Array(element) => Some(IterableProtocol::Array {
            item: (**element).clone(),
        }),
        TypeId::Builtin(BuiltinType::String) => Some(IterableProtocol::String {
            item: BuiltinType::String,
        }),
        _ => None,
    }
}

fn builtin_family(ty: &TypeId) -> Option<BuiltinTypeFamily> {
    let TypeId::Builtin(builtin) = ty else {
        return None;
    };
    builtin_type_spec(*builtin).map(|spec| spec.family)
}
