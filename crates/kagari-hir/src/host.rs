//! Declaration queries do not depend on the runtime or invoke host callbacks.
use kagari_common::host_interface::HostValueType;

use crate::types::{BuiltinType, TypeId};

pub fn scalar_type(ty: &HostValueType) -> Option<TypeId> {
    Some(TypeId::Builtin(match ty {
        HostValueType::Unit => BuiltinType::Unit,
        HostValueType::Bool => BuiltinType::Bool,
        HostValueType::I32 => BuiltinType::I32,
        HostValueType::I64 => BuiltinType::I64,
        HostValueType::F32 => BuiltinType::F32,
        HostValueType::F64 => BuiltinType::F64,
        HostValueType::String => BuiltinType::String,
        HostValueType::Opaque(_) => return None,
    }))
}
