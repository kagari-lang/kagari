use crate::{gc::GcHeap, value::Value};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReflectionError {
    message: String,
}

impl ReflectionError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

pub fn type_of(gc: &GcHeap, value: &Value) -> Value {
    let type_name = match value {
        Value::Unit => "()",
        Value::Bool(_) => "bool",
        Value::I32(_) => "i32",
        Value::I64(_) => "i64",
        Value::F32(_) => "f32",
        Value::F64(_) => "f64",
        Value::Str(_) => "String",
        Value::Tuple(_) => "tuple",
        Value::Array(_) => "array",
        Value::Map(_) => "map",
        Value::Set(_) => "set",
        Value::Struct(handle) => {
            return Value::Str(
                gc.struct_name(*handle)
                    .unwrap_or_else(|| "struct".to_owned()),
            );
        }
        Value::GcHandle(_) => "gc_handle",
        Value::Interface(_) => "interface",
        Value::HostRoot(_) => "host_root",
        Value::HostPathView(_) => "host_path_view",
        Value::Ephemeral(_) => "ephemeral",
    };

    Value::Str(type_name.to_owned())
}

pub fn get_field(gc: &GcHeap, value: &Value, field_name: &str) -> Result<Value, ReflectionError> {
    match value {
        Value::Struct(handle) => gc
            .struct_get_field(*handle, field_name)
            .ok_or_else(|| ReflectionError::new(format!("missing field `{field_name}`"))),
        _ => Err(ReflectionError::new(
            "reflect_get_field expects struct value",
        )),
    }
}

pub fn set_field(
    gc: &GcHeap,
    value: &Value,
    field_name: &str,
    next_value: Value,
) -> Result<Value, ReflectionError> {
    if !next_value.is_default_heap_payload() {
        return Err(ReflectionError::new(
            "reflect_set_field expects default-storable value",
        ));
    }

    match value {
        Value::Struct(handle) => {
            let Some(()) = gc.struct_set_field(*handle, field_name, next_value) else {
                return Err(ReflectionError::new(format!(
                    "missing field `{field_name}`"
                )));
            };
            Ok(Value::Struct(*handle))
        }
        _ => Err(ReflectionError::new(
            "reflect_set_field expects struct value",
        )),
    }
}

pub fn set_index(
    gc: &GcHeap,
    value: &Value,
    index: &Value,
    next_value: Value,
) -> Result<Value, ReflectionError> {
    if !next_value.is_default_heap_payload() {
        return Err(ReflectionError::new(
            "reflect_set_index expects default-storable value",
        ));
    }

    let index = match index {
        Value::I32(index) if *index >= 0 => *index as usize,
        Value::I64(index) if *index >= 0 => *index as usize,
        _ => {
            return Err(ReflectionError::new(
                "reflect_set_index expects non-negative integer index",
            ));
        }
    };

    match value {
        Value::Array(handle) => {
            let Some(()) = gc.array_set(*handle, index, next_value) else {
                return Err(ReflectionError::new(format!("invalid index `{index}`")));
            };
            Ok(Value::Array(*handle))
        }
        Value::Tuple(elements) => {
            let mut updated = elements.clone();
            let Some(slot) = updated.get_mut(index) else {
                return Err(ReflectionError::new(format!("invalid index `{index}`")));
            };
            *slot = next_value;
            Ok(Value::Tuple(updated))
        }
        _ => Err(ReflectionError::new(
            "reflect_set_index expects array or tuple value",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        gc::GcHeapConfig,
        host::{
            DynamicPathArguments, HostBorrowTable, HostObjectId, HostPathDescriptorRegistration,
            HostPathSegment, HostRegistry, HostRootHandle, HostSchemaEpoch, HostTypeInfo,
            HostTypeOwnership,
        },
        metadata::{AbiFingerprint, FieldMetadataId, PathAccess, TypeId},
        value::InterfaceObjectId,
    };

    fn host_root_value(object_id: u64) -> Value {
        Value::HostRoot(HostRootHandle::new(
            HostObjectId(object_id),
            TypeId::new(0),
            HostSchemaEpoch::new(0),
            AbiFingerprint(1),
        ))
    }

    fn path_view_value(object_id: u64) -> Value {
        let root_type = TypeId::new(0);
        let result_type = TypeId::new(1);
        let mut registry = HostRegistry::default();
        registry
            .register_type(HostTypeInfo {
                type_id: root_type,
                script_name: "Player".to_owned(),
                rust_type_name: "Player".to_owned(),
                ownership: HostTypeOwnership::HostRoot,
                fields: Vec::new(),
                methods: Vec::new(),
                traits: Vec::new(),
                path_access: PathAccess::ReadWrite,
                reflection: crate::host::HostReflectionPolicy::Hidden,
                abi_fingerprint: AbiFingerprint(1),
            })
            .unwrap();
        let root = registry
            .register_root(HostObjectId(object_id), root_type, HostSchemaEpoch::new(0))
            .unwrap();
        let descriptor = registry
            .register_path_descriptor(HostPathDescriptorRegistration {
                root_type,
                result_type,
                segments: vec![HostPathSegment::Field {
                    name: "hp".to_owned(),
                    field_id: FieldMetadataId::new(0),
                    owner_type: root_type,
                    result_type,
                    access: PathAccess::ReadWrite,
                    abi_fingerprint: AbiFingerprint(2),
                }],
                access: PathAccess::ReadWrite,
                schema_epoch: HostSchemaEpoch::new(0),
                abi_fingerprint: AbiFingerprint(3),
                capability_requirements: crate::security::CapabilitySet::default(),
            })
            .unwrap();
        Value::HostPathView(
            registry
                .make_path_view(root, descriptor, DynamicPathArguments::empty())
                .unwrap(),
        )
    }

    fn shared_borrow_value(object_id: u64) -> Value {
        let table = HostBorrowTable::default();
        let guard = table.enter_frame();
        Value::host_ref(
            guard
                .borrow_shared(HostObjectId(object_id), TypeId::new(0))
                .unwrap(),
        )
    }

    #[test]
    fn reports_production_value_category_names() {
        let gc = GcHeap::new(GcHeapConfig::default());
        let map = gc.alloc_map(vec![]).unwrap();
        let set = gc.alloc_set(vec![]).unwrap();

        assert_eq!(type_of(&gc, &Value::Map(map)), Value::Str("map".to_owned()));
        assert_eq!(type_of(&gc, &Value::Set(set)), Value::Str("set".to_owned()));
        assert_eq!(
            type_of(&gc, &Value::Interface(InterfaceObjectId(1))),
            Value::Str("interface".to_owned())
        );
        assert_eq!(
            type_of(&gc, &host_root_value(2)),
            Value::Str("host_root".to_owned())
        );
        assert_eq!(
            type_of(&gc, &path_view_value(3)),
            Value::Str("host_path_view".to_owned())
        );
        assert_eq!(
            type_of(&gc, &shared_borrow_value(4)),
            Value::Str("ephemeral".to_owned())
        );
    }
}
