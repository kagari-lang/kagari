use kagari_hir::types::{BuiltinType, TypeId};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ValueType {
    #[default]
    Unit,
    Bool,
    I32,
    I64,
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
    pub fn from_host_type(ty: &kagari_common::host_interface::HostValueType) -> Self {
        use kagari_common::host_interface::HostValueType as Host;
        match ty {
            Host::Unit => Self::Unit,
            Host::Bool => Self::Bool,
            Host::I32 => Self::I32,
            Host::I64 => Self::I64,
            Host::F32 => Self::F32,
            Host::F64 => Self::F64,
            Host::String => Self::Str,
            Host::Opaque(_) => Self::HostHandle,
            Host::Tuple(_)
            | Host::Array(_)
            | Host::Map { .. }
            | Host::Set(_)
            | Host::Option(_)
            | Host::Result { .. } => Self::HeapObject,
        }
    }
    pub fn from_type_id(type_id: &TypeId) -> Self {
        match type_id {
            TypeId::Host(_) => Self::HostHandle,
            TypeId::Unknown | TypeId::Error | TypeId::Generic(_) | TypeId::SelfType(_) => {
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
                | BuiltinType::U32
                | BuiltinType::U64
                | BuiltinType::USize,
            ) => Self::I64,
            TypeId::Builtin(BuiltinType::F32) => Self::F32,
            TypeId::Builtin(BuiltinType::F64) => Self::F64,
            TypeId::Builtin(BuiltinType::String) => Self::Str,
            TypeId::Tuple(_)
            | TypeId::Array(_)
            | TypeId::Map { .. }
            | TypeId::Set(_)
            | TypeId::Struct(_)
            | TypeId::Enum(_)
            | TypeId::Trait(_)
            | TypeId::StandardEnum { .. } => Self::HeapObject,
        }
    }
}
