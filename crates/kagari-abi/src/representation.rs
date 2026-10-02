use crate::scalar::BuiltinType;
use kagari_common::host_interface::value_type::HostValueType;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ValueType {
    /// No runtime value can inhabit this representation.
    Never,
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
    // broader than concrete semantic types and covers tuples, arrays, structs, enums, and
    // future runtime-managed objects such as closures or reflected values.
    HeapObject,
    HostHandle,
    /// Tagged runtime Value in a checked shared generic body. Its semantic type
    /// is a scoped parameter or projection, supplied by the call environment.
    Generic,
}

impl ValueType {
    pub fn may_contain_gc_reference(self) -> bool {
        matches!(self, Self::HeapObject | Self::Generic)
    }
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
    /// Physical slots preserve the existing numeric representation policy.
    pub fn from_builtin_type(ty: BuiltinType) -> Self {
        match ty {
            BuiltinType::Never => Self::Never,
            BuiltinType::Unit => Self::Unit,
            BuiltinType::Bool => Self::Bool,
            BuiltinType::I8 | BuiltinType::I16 | BuiltinType::I32 => Self::I32,
            BuiltinType::I64
            | BuiltinType::ISize
            | BuiltinType::U8
            | BuiltinType::U16
            | BuiltinType::U32 => Self::I64,
            BuiltinType::U64 | BuiltinType::USize => Self::U64,
            BuiltinType::F32 => Self::F32,
            BuiltinType::F64 => Self::F64,
            BuiltinType::String => Self::Str,
        }
    }
}
