//! Hashing of intrinsic values, independent of collection storage.
use crate::{
    gc::GcHeap,
    value::{EnumTag, Value},
};
use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
};

/// Hash intrinsic scalar, tuple and enum values using language equality.
/// Unsupported values, invalid heap identities and excessive trees return `None`.
/// Access protected backend values through the existing semantic operation.
///
/// # Safety
/// Keep source objects and all managed arguments traced through this operation.
/// Copied managed results and nested snapshot edges need independent protection
/// before callbacks, collection, source mutation or clearing diagnostic records.
/// Returned storage loans must end before reentry. Validation cannot revive an
/// expired direct reference; scoped host resources keep their original lifetime.
#[doc(hidden)]
pub unsafe fn script_hash_transport(gc: &GcHeap, value: &Value) -> Option<i64> {
    script_hash(gc, value)
}

pub(crate) fn script_hash(gc: &GcHeap, value: &Value) -> Option<i64> {
    // The first value is already available. Allocate traversal backing only if
    // a compound value actually contributes children; scalar hashing needs none.
    let mut first = Some(*value);
    let mut pending = Vec::new();
    let mut hasher = DefaultHasher::new();
    let mut count = 0usize;
    while let Some(value) = first.take().or_else(|| pending.pop()) {
        count += 1;
        if count > 65536 || !gc.validate_value(&value) {
            return None;
        }
        match value {
            Value::Unit => 0u8.hash(&mut hasher),
            Value::Bool(v) => {
                1u8.hash(&mut hasher);
                v.hash(&mut hasher);
            }
            Value::Char(v) => {
                2u8.hash(&mut hasher);
                v.hash(&mut hasher);
            }
            Value::I32(v) => {
                3u8.hash(&mut hasher);
                v.hash(&mut hasher);
            }
            Value::I64(v) => {
                4u8.hash(&mut hasher);
                v.hash(&mut hasher);
            }
            Value::U64(v) => {
                5u8.hash(&mut hasher);
                v.hash(&mut hasher);
            }
            Value::Str(id) => {
                6u8.hash(&mut hasher);
                gc.string(id)?.hash(&mut hasher);
            }
            Value::Tuple(id) => {
                let fields = gc.tuple(id)?;
                7u8.hash(&mut hasher);
                fields.len().hash(&mut hasher);
                pending.extend(fields.iter().rev().copied());
            }
            Value::Enum(id) => {
                let view = gc.enum_view(id)?;
                let EnumTag::Declared(layout) = &view.tag;
                8u8.hash(&mut hasher);
                layout.registry_owner().hash(&mut hasher);
                layout.layout().declaration.hash(&mut hasher);
                layout.layout().arguments.hash(&mut hasher);
                layout.variant().declaration.hash(&mut hasher);
                view.fields.len().hash(&mut hasher);
                pending.extend(view.fields.iter().rev().copied());
            }
            _ => return None,
        }
    }
    Some(hasher.finish() as i64)
}
