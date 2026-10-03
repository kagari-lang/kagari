//! Lower semantic types and portable host schemas to physical slots.
use crate::scalar::BuiltinType;
use kagari_abi::representation::ValueType;
use kagari_common::host_interface::value_type::HostValueType;

pub fn host_representation(ty: &HostValueType) -> ValueType {
    match ty {
        HostValueType::Unit => ValueType::Unit,
        HostValueType::Bool => ValueType::Bool,
        HostValueType::I32 => ValueType::I32,
        HostValueType::I64 => ValueType::I64,
        HostValueType::F32 => ValueType::F32,
        HostValueType::F64 => ValueType::F64,
        HostValueType::String => ValueType::Str,
        HostValueType::Opaque(_) => ValueType::HostHandle,
        HostValueType::Tuple(_)
        | HostValueType::Array(_, _)
        | HostValueType::Map { .. }
        | HostValueType::Set(_, _)
        | HostValueType::Option(_)
        | HostValueType::Result { .. } => ValueType::HeapObject,
    }
}

/// Physical slots preserve the existing numeric representation policy.
pub fn builtin_representation(ty: BuiltinType) -> ValueType {
    match ty {
        BuiltinType::Never => ValueType::Never,
        BuiltinType::Unit => ValueType::Unit,
        BuiltinType::Bool => ValueType::Bool,
        BuiltinType::I8 | BuiltinType::I16 | BuiltinType::I32 => ValueType::I32,
        BuiltinType::I64
        | BuiltinType::ISize
        | BuiltinType::U8
        | BuiltinType::U16
        | BuiltinType::U32 => ValueType::I64,
        BuiltinType::U64 | BuiltinType::USize => ValueType::U64,
        BuiltinType::F32 => ValueType::F32,
        BuiltinType::F64 => ValueType::F64,
        BuiltinType::String => ValueType::Str,
    }
}
