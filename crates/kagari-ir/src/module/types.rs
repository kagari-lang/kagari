use kagari_common::host_interface::HostValueType;
use kagari_hir::types::{BuiltinType, TypeId};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ValueType {
    #[default]
    Unit,
    Bool,
    I32,
    I64,
    U64,
    F32,
    F64,
    Str,
    // Execution-layer reference to a heap-backed runtime object. This is intentionally
    // broader than HIR's full TypeId and covers tuples, arrays, structs, enums, and
    // future runtime-managed objects such as closures or reflected values.
    HeapObject,
    HostHandle,
}

impl ValueType {
    pub fn from_host_type(ty: &HostValueType) -> Self {
        match ty {
            HostValueType::Unit => Self::Unit,
            HostValueType::Bool => Self::Bool,
            HostValueType::I32 => Self::I32,
            HostValueType::I64 => Self::I64,
            HostValueType::F32 => Self::F32,
            HostValueType::F64 => Self::F64,
            HostValueType::String => Self::Str,
            HostValueType::Opaque(_) => Self::HostHandle,
            HostValueType::Tuple(_)
            | HostValueType::Array(_, _)
            | HostValueType::Map { .. }
            | HostValueType::Set(_, _)
            | HostValueType::Option(_)
            | HostValueType::Result { .. } => Self::HeapObject,
        }
    }
    pub fn from_type_id(type_id: &TypeId) -> Self {
        match type_id {
            TypeId::Host(_) => Self::HostHandle,
            TypeId::Projection { .. }
            | TypeId::Inference(_)
            | TypeId::Unknown
            | TypeId::Error
            | TypeId::Generic(_)
            | TypeId::SelfType(_) => {
                unreachable!("unchecked type reached code generation")
            }
            TypeId::Builtin(BuiltinType::Unit) => Self::Unit,
            TypeId::Builtin(BuiltinType::Bool) => Self::Bool,
            TypeId::Builtin(BuiltinType::I8 | BuiltinType::I16 | BuiltinType::I32) => Self::I32,
            TypeId::Builtin(
                BuiltinType::I64
                | BuiltinType::ISize
                | BuiltinType::U8
                | BuiltinType::U16
                | BuiltinType::U32,
            ) => Self::I64,
            TypeId::Builtin(BuiltinType::U64 | BuiltinType::USize) => Self::U64,
            TypeId::Builtin(BuiltinType::F32) => Self::F32,
            TypeId::Builtin(BuiltinType::F64) => Self::F64,
            TypeId::Builtin(BuiltinType::String) => Self::Str,
            TypeId::Tuple(_)
            | TypeId::Function { .. }
            | TypeId::Iter(_)
            | TypeId::Range(_, _)
            | TypeId::Array(_, _)
            | TypeId::Map { .. }
            | TypeId::Set(_, _)
            | TypeId::Struct(_)
            | TypeId::Enum(_)
            | TypeId::Trait(_)
            | TypeId::StandardEnum { .. } => Self::HeapObject,
        }
    }
}
