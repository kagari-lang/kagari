use crate::{RuntimeError, RuntimeErrorKind, gc::GcHeap, value::Value};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReflectionError {
    error: RuntimeError,
}

impl ReflectionError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            error: RuntimeError::new(RuntimeErrorKind::ScriptTrap, message),
        }
    }

    pub fn message(&self) -> &str {
        self.error.message()
    }
    pub fn kind(&self) -> RuntimeErrorKind {
        self.error.kind()
    }
    pub(crate) fn into_write_error(self) -> RuntimeError {
        if self.error.kind() == RuntimeErrorKind::ScriptTrap {
            RuntimeError::invalid_reflective_write(self.error.message())
        } else {
            self.error
        }
    }
}
impl From<RuntimeError> for ReflectionError {
    fn from(error: RuntimeError) -> Self {
        Self { error }
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
        Value::Enum(handle) => {
            return Value::Str(
                gc.enum_snapshot(*handle)
                    .map(|snapshot| snapshot.tag.type_name().to_owned())
                    .unwrap_or_else(|| "enum".to_owned()),
            );
        }
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
        Value::Struct(handle) => {
            let (layout, slot) = resolve_field(gc, *handle, field_name)?;
            gc.struct_get_slot(*handle, &layout, slot)
                .ok_or_else(|| ReflectionError::new("invalid struct field"))
        }
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
            let (layout, slot) = resolve_field(gc, *handle, field_name)?;
            gc.struct_set_slot(*handle, &layout, slot, next_value)
                .map_err(ReflectionError::from)?;
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
            gc.array_set(*handle, index, next_value)
                .map_err(ReflectionError::from)?;
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

fn resolve_field(
    gc: &GcHeap,
    handle: crate::gc::HeapObjectId,
    name: &str,
) -> Result<(crate::module::StructLayoutRef, usize), ReflectionError> {
    let layout = gc
        .struct_layout(handle)
        .ok_or_else(|| ReflectionError::new("invalid struct"))?;
    let slot = layout
        .layout()
        .fields
        .iter()
        .position(|field| field.name == name)
        .ok_or_else(|| ReflectionError::new(format!("missing field {name}")))?;
    Ok((layout, slot))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        gc::GcHeapConfig,
        host::{
            DynamicPathArguments, HostBorrowTable, HostObjectId, HostPathDescriptorRegistration,
            HostPathSegmentRegistration, HostRootHandle, HostSchemaEpoch, HostTypeOwnership,
        },
        metadata::{AbiFingerprint, PathAccess, TypeId},
        value::InterfaceObjectId,
    };

    fn host_root_value(object_id: u64) -> Value {
        Value::HostRoot(HostRootHandle::new(
            Default::default(),
            HostObjectId(object_id),
            TypeId::new(0),
            HostSchemaEpoch::new(0),
            AbiFingerprint(1),
        ))
    }

    fn path_view_value(object_id: u64) -> Value {
        let result_type = TypeId::new(1);
        let mut runtime = crate::Runtime::default();
        let mut declaration = kagari_common::host_interface::HostTypeDeclaration::new("Player");
        declaration.ownership = HostTypeOwnership::HostRoot;
        declaration.path_access = PathAccess::ReadWrite;
        let mut hp = kagari_common::host_interface::HostFieldDeclaration::new(
            &declaration.id,
            "hp",
            kagari_common::host_interface::HostValueType::I32,
        );
        hp.writable = true;
        hp.path_access = PathAccess::ReadWrite;
        declaration.fields.push(hp);
        let root_type = runtime
            .register_host_type(crate::HostTypeRegistration::new(declaration, "Player"))
            .unwrap();
        let root = runtime
            .register_host_root(HostObjectId(object_id), root_type, HostSchemaEpoch::new(0))
            .unwrap();
        let descriptor = runtime
            .register_host_path_descriptor(HostPathDescriptorRegistration {
                root_type,
                result_type,
                segments: vec![HostPathSegmentRegistration::Field {
                    declaration: runtime
                        .host()
                        .host_type(root_type)
                        .unwrap()
                        .declaration
                        .fields
                        .iter()
                        .find(|field| field.name == "hp")
                        .unwrap()
                        .id
                        .clone(),
                }],
                access: PathAccess::ReadWrite,
                schema_epoch: HostSchemaEpoch::new(0),
                capability_requirements: crate::security::CapabilitySet::default(),
            })
            .unwrap();
        Value::HostPathView(
            runtime
                .host()
                .make_path_view(root, descriptor, DynamicPathArguments::empty())
                .unwrap(),
        )
    }

    fn shared_borrow_value(object_id: u64) -> Value {
        let table = HostBorrowTable::default();
        let guard = table.enter_frame().unwrap();
        Value::host_ref(
            guard
                .borrow_shared(HostObjectId(object_id), TypeId::new(0))
                .unwrap(),
        )
    }

    #[test]
    fn reports_production_value_category_names() {
        let gc = GcHeap::new(
            GcHeapConfig::default(),
            std::rc::Rc::new(crate::resource::ResourceState::default()),
        );
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
