//! Lower semantic types and portable host schemas to physical slots.
use kagari_abi::representation::ValueType;
use kagari_types::{scalar::BuiltinType, ty::Ty};

/// Physical slots preserve the existing numeric representation policy.
pub fn builtin_representation(ty: BuiltinType) -> ValueType {
    match ty {
        BuiltinType::Never => ValueType::Never,
        BuiltinType::Unit => ValueType::Unit,
        BuiltinType::Bool => ValueType::Bool,
        BuiltinType::I8 | BuiltinType::I16 | BuiltinType::I32 => ValueType::I32,
        BuiltinType::I64 | BuiltinType::U8 | BuiltinType::U16 | BuiltinType::U32 => ValueType::I64,
        BuiltinType::U64 => ValueType::U64,
        BuiltinType::F32 => ValueType::F32,
        BuiltinType::F64 => ValueType::F64,
        BuiltinType::String => ValueType::Str,
        BuiltinType::Char => ValueType::Char,
    }
}

/// Lower a semantic type to the physical slot required by checked execution.
pub fn semantic_representation(ty: &Ty) -> ValueType {
    match ty {
        Ty::Host(_) | Ty::Reference { .. } => ValueType::HostHandle,
        Ty::Builtin(ty) => builtin_representation(*ty),
        Ty::Parameter { .. } | Ty::Projection { .. } | Ty::SelfType(_) => ValueType::Generic,
        _ => ValueType::HeapObject,
    }
}
