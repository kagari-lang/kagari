use crate::{
    error::{RuntimeError, RuntimeErrorKind},
    gc::{GcHeap, HeapObjectId},
    module::StructLayoutRef,
    value::Value,
};

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

    pub fn into_runtime_error(self) -> RuntimeError {
        self.error
    }

    pub(crate) fn into_write_error(self) -> RuntimeError {
        if matches!(
            self.error.kind(),
            RuntimeErrorKind::ScriptTrap | RuntimeErrorKind::IndexOutOfBounds
        ) {
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

/// Access protected backend values through the existing semantic operation.
///
/// # Safety
/// Keep source objects and all managed arguments traced through this operation.
/// Copied managed results and nested snapshot edges need independent protection
/// before callbacks, collection, source mutation or clearing diagnostic records.
/// Returned storage loans must end before reentry. Validation cannot revive an
/// expired direct reference; scoped host resources keep their original lifetime.
#[doc(hidden)]
pub unsafe fn type_of_transport(gc: &GcHeap, value: &Value) -> Result<Value, ReflectionError> {
    type_of(gc, value)
}

pub(crate) fn type_of(gc: &GcHeap, value: &Value) -> Result<Value, ReflectionError> {
    let type_name = match value {
        Value::Range(value) => gc
            .range(*value)
            .ok_or_else(|| ReflectionError::new("invalid range"))?
            .kind
            .name(),
        Value::Unit => "()",
        Value::Bool(_) => "bool",
        Value::Char(_) => "char",
        Value::I32(_) => "i32",
        Value::I64(_) => "i64",
        Value::U64(_) => "u64",
        Value::F32(_) => "f32",
        Value::F64(_) => "f64",
        Value::Str(_) => "String",
        Value::Tuple(_) => "tuple",
        Value::Array(_) => "array",

        Value::Enum(handle) => {
            // Copy only the name and release the heap view before allocating.
            let name = gc
                .enum_view(*handle)
                .map(|view| view.tag.type_name().to_owned())
                .unwrap_or_else(|| "enum".to_owned());
            return gc.alloc_string(name).map_err(Into::into);
        }
        Value::Struct(handle) => {
            return gc
                .alloc_string(
                    gc.struct_name(*handle)
                        .unwrap_or_else(|| "struct".to_owned()),
                )
                .map_err(Into::into);
        }
        Value::GcHandle(_) => "gc_handle",
        Value::Interface(_) => "interface",
        Value::Closure(_) => "closure",
        Value::Cell(_) => "cell",
        Value::HostRoot(_) => "host_root",
        Value::HostPathView(_) => "host_path_view",
        Value::Ephemeral(_) | Value::RuntimeEphemeral(_) => "ephemeral",
    };

    gc.alloc_string(type_name.to_owned()).map_err(Into::into)
}

/// Access protected backend values through the existing semantic operation.
///
/// # Safety
/// Keep source objects and all managed arguments traced through this operation.
/// Copied managed results and nested snapshot edges need independent protection
/// before callbacks, collection, source mutation or clearing diagnostic records.
/// Returned storage loans must end before reentry. Validation cannot revive an
/// expired direct reference; scoped host resources keep their original lifetime.
#[doc(hidden)]
pub unsafe fn get_field_transport(
    gc: &GcHeap,
    value: &Value,
    field_name: &str,
) -> Result<Value, ReflectionError> {
    get_field(gc, value, field_name)
}

pub(crate) fn get_field(
    gc: &GcHeap,
    value: &Value,
    field_name: &str,
) -> Result<Value, ReflectionError> {
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

/// Access protected backend values through the existing semantic operation.
///
/// # Safety
/// Keep source objects and all managed arguments traced through this operation.
/// Copied managed results and nested snapshot edges need independent protection
/// before callbacks, collection, source mutation or clearing diagnostic records.
/// Returned storage loans must end before reentry. Validation cannot revive an
/// expired direct reference; scoped host resources keep their original lifetime.
#[doc(hidden)]
pub unsafe fn set_field_transport(
    gc: &GcHeap,
    value: &Value,
    field_name: &str,
    next_value: Value,
) -> Result<Value, ReflectionError> {
    set_field(gc, value, field_name, next_value)
}

pub(crate) fn set_field(
    gc: &GcHeap,
    value: &Value,
    field_name: &str,
    next_value: Value,
) -> Result<Value, ReflectionError> {
    if !next_value.is_default_heap_payload(gc) {
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

/// Access protected backend values through the existing semantic operation.
///
/// # Safety
/// Keep source objects and all managed arguments traced through this operation.
/// Copied managed results and nested snapshot edges need independent protection
/// before callbacks, collection, source mutation or clearing diagnostic records.
/// Returned storage loans must end before reentry. Validation cannot revive an
/// expired direct reference; scoped host resources keep their original lifetime.
#[doc(hidden)]
pub unsafe fn set_index_transport(
    gc: &GcHeap,
    value: &Value,
    index: &Value,
    next_value: Value,
) -> Result<Value, ReflectionError> {
    set_index(gc, value, index, next_value)
}

pub(crate) fn set_index(
    gc: &GcHeap,
    value: &Value,
    index: &Value,
    next_value: Value,
) -> Result<Value, ReflectionError> {
    if !next_value.is_default_heap_payload(gc) {
        return Err(ReflectionError::new(
            "reflect_set_index expects default-storable value",
        ));
    }

    let index = match index {
        Value::I32(index) if *index >= 0 => *index as u64,
        Value::I64(index) if *index >= 0 => *index as u64,
        Value::U64(index) => *index,
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
            let mut updated = gc
                .tuple(*elements)
                .ok_or_else(|| ReflectionError::new("invalid tuple"))?
                .to_vec();
            let Some(slot) = usize::try_from(index)
                .ok()
                .and_then(|index| updated.get_mut(index))
            else {
                return Err(ReflectionError::new(format!("invalid index `{index}`")));
            };
            *slot = next_value;
            gc.alloc_tuple(updated).map_err(Into::into)
        }
        _ => Err(ReflectionError::new(
            "reflect_set_index expects array or tuple value",
        )),
    }
}

fn resolve_field(
    gc: &GcHeap,
    handle: HeapObjectId,
    name: &str,
) -> Result<(StructLayoutRef, usize), ReflectionError> {
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
        host::{HostBorrowTable, HostObjectId, HostRootHandle, HostSchemaEpoch},
        metadata::{AbiFingerprint, TypeId},
        test_support::layouts::{allocation_owner, path_view_value},
    };

    use kagari_types::{scalar::BuiltinType, ty::Ty};

    fn host_root_value(gc: &GcHeap, object_id: u64) -> Value {
        gc.alloc_host_root(HostRootHandle::new(
            Default::default(),
            HostObjectId(object_id),
            TypeId::new(0),
            HostSchemaEpoch::new(0),
            AbiFingerprint(1),
        ))
        .unwrap()
    }

    fn shared_borrow_value(gc: &GcHeap, object_id: u64) -> Value {
        let table = HostBorrowTable::default();
        let guard = table.enter_frame().unwrap();
        gc.alloc_host_ref(
            guard
                .borrow_shared(HostObjectId(object_id), TypeId::new(0))
                .unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn wide_indices_do_not_mutate_arrays_or_tuples() {
        let mut runtime = crate::Runtime::default();
        let owner = allocation_owner(&mut runtime);
        let gc = runtime.gc();
        let array = Value::Array(
            runtime
                .alloc_array(&owner, Ty::Builtin(BuiltinType::I32), vec![Value::I32(7)])
                .unwrap(),
        );
        let tuple = gc.alloc_tuple(vec![Value::I32(7)]).unwrap();
        let before = gc.stats();
        for index in [
            Value::U64(1_u64 << 32),
            Value::U64(u64::MAX),
            Value::I64(1_i64 << 32),
        ] {
            for value in [array, tuple] {
                let error = set_index(gc, &value, &index, Value::I32(9)).unwrap_err();
                assert_eq!(
                    error.into_write_error().kind(),
                    RuntimeErrorKind::InvalidReflectiveWrite
                );
            }
        }
        let Value::Array(id) = array else {
            unreachable!()
        };
        assert_eq!(gc.array_get(id, 0), Some(Value::I32(7)));
        let Value::Tuple(id) = tuple else {
            unreachable!()
        };
        assert_eq!(&*gc.tuple(id).unwrap(), &[Value::I32(7)]);
        assert_eq!(gc.stats(), before);
    }

    #[test]
    fn reports_production_value_category_names() {
        let mut runtime = crate::Runtime::default();
        let interface = crate::test_support::layouts::interface_value(&mut runtime);
        let gc = runtime.gc();
        for (value, expected) in [
            (interface, "interface"),
            (host_root_value(gc, 2), "host_root"),
            (path_view_value(gc, 3), "host_path_view"),
            (shared_borrow_value(gc, 4), "ephemeral"),
        ] {
            let Value::Str(id) = type_of(gc, &value).unwrap() else {
                panic!("type name");
            };
            assert_eq!(&*gc.string(id).unwrap(), expected);
        }
    }
}
