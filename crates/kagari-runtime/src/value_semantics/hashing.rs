//! Structural builtin value hashing, independent of collection key/table policy.
use crate::{
    error::RuntimeError,
    host::HostRegistryId,
    resource::ResourceState,
    value::{
        aggregates::{enumerations::EnumObject, tuples::TupleObject},
        carriers::ScopedValue,
        kind::ObjectKind,
        strings::StringObject,
    },
    value_semantics::storage_error,
};
use kagari_abi::representation::ValueType;
use kagari_common::identity::table::DefinitionId;
use kagari_gc::{access::Scope, error::GcError};
use kagari_types::ty::Ty;
use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    slice,
};

#[derive(Debug, Clone)]
struct StructuralKey {
    parts: KeyParts,
}

#[derive(Debug, Clone)]
enum KeyParts {
    Single(KeyPart),
    Aggregate(Box<[KeyPart]>),
}

impl KeyParts {
    fn as_slice(&self) -> &[KeyPart] {
        match self {
            Self::Single(part) => slice::from_ref(part),
            Self::Aggregate(parts) => parts,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum KeyPart {
    Unit,
    Bool(bool),
    Char(char),
    I32(i32),
    I64(i64),
    U64(u64),
    Str(String),
    Tuple(usize),
    DeclaredEnum(HostRegistryId, DefinitionId, Vec<Ty>, DefinitionId),
}

impl Hash for StructuralKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.parts.as_slice().hash(state);
    }
}

impl StructuralKey {
    fn from_value<'gc>(
        cx: &Scope<'gc>,
        resources: &ResourceState,
        value: ScopedValue<'gc>,
    ) -> Result<Option<Self>, RuntimeError> {
        if let Some(part) = scalar_part(value) {
            return Ok(Some(Self {
                parts: KeyParts::Single(part),
            }));
        }
        if value.kind() == Some(ObjectKind::String) {
            return Ok(Some(Self {
                parts: KeyParts::Single(string_part(cx, value)?),
            }));
        }
        let mut pending = Vec::new();
        pending
            .try_reserve(1)
            .map_err(|_| RuntimeError::resource_limit("value hashing scratch"))?;
        pending.push(value);
        let mut parts = Vec::new();
        while let Some(value) = pending.pop() {
            if parts.len() >= 65536 {
                return Ok(None);
            }
            if parts.len().is_multiple_of(1024) {
                resources.poll_closed()?;
            }
            parts
                .try_reserve(2)
                .map_err(|_| RuntimeError::resource_limit("value hashing scratch"))?;
            if let Some(part) = scalar_part(value) {
                parts.push(part);
                continue;
            }
            match value.kind() {
                Some(ObjectKind::String) => parts.push(string_part(cx, value)?),
                Some(ObjectKind::Tuple) => {
                    let object = cx
                        .downcast::<TupleObject>(
                            value
                                .object()
                                .ok_or_else(|| storage_error(GcError::Representation))?,
                        )
                        .map_err(storage_error)?;
                    let count = cx.read(object, TupleObject::len);
                    parts.push(KeyPart::Tuple(count));
                    if !reserve_children(&mut pending, count, parts.len())? {
                        return Ok(None);
                    }
                    for index in (0..count).rev() {
                        if index.is_multiple_of(1024) {
                            resources.poll_closed()?;
                        }
                        pending.push(TupleObject::get(cx, object, index).map_err(storage_error)?);
                    }
                }
                Some(ObjectKind::Enum) => {
                    let object = cx
                        .downcast::<EnumObject>(
                            value
                                .object()
                                .ok_or_else(|| storage_error(GcError::Representation))?,
                        )
                        .map_err(storage_error)?;
                    let identity = cx.read(object, |object| {
                        let layout = object.layout();
                        let (nominal, variant) = layout.value_identity()?;
                        Some(KeyPart::DeclaredEnum(
                            layout.registry_owner(),
                            nominal.declaration,
                            nominal.arguments,
                            variant,
                        ))
                    });
                    let Some(identity) = identity else {
                        return Ok(None);
                    };
                    parts.push(identity);
                    let count = cx.read(object, EnumObject::len);
                    parts.push(KeyPart::Tuple(count));
                    if !reserve_children(&mut pending, count, parts.len())? {
                        return Ok(None);
                    }
                    for index in (0..count).rev() {
                        if index.is_multiple_of(1024) {
                            resources.poll_closed()?;
                        }
                        pending.push(EnumObject::get(cx, object, index).map_err(storage_error)?);
                    }
                }
                _ => return Ok(None),
            }
        }
        Ok(Some(Self {
            parts: KeyParts::Aggregate(parts.into_boxed_slice()),
        }))
    }
}

fn reserve_children(
    pending: &mut Vec<ScopedValue<'_>>,
    count: usize,
    completed: usize,
) -> Result<bool, RuntimeError> {
    // Every pending child contributes at least one part. Reject before allocating
    // an unbounded worklist for an already impossible key-size contract.
    if count
        > 65536usize
            .saturating_sub(completed)
            .saturating_sub(pending.len())
    {
        return Ok(false);
    }
    pending
        .try_reserve(count)
        .map_err(|_| RuntimeError::resource_limit("value hashing scratch"))?;
    Ok(true)
}

fn scalar_part(value: ScopedValue<'_>) -> Option<KeyPart> {
    let (kind, bits) = value.scalar_payload()?;
    Some(match kind {
        ValueType::Unit => KeyPart::Unit,
        ValueType::Bool => KeyPart::Bool(bits != 0),
        ValueType::Char => KeyPart::Char(u32::try_from(bits).ok().and_then(char::from_u32)?),
        ValueType::I32 => KeyPart::I32(bits as i32),
        ValueType::I64 => KeyPart::I64(bits as i64),
        ValueType::U64 => KeyPart::U64(bits),
        _ => return None,
    })
}

fn string_part<'gc>(cx: &Scope<'gc>, value: ScopedValue<'gc>) -> Result<KeyPart, RuntimeError> {
    let object = cx
        .downcast::<StringObject>(
            value
                .object()
                .ok_or_else(|| storage_error(GcError::Representation))?,
        )
        .map_err(storage_error)?;
    cx.read(object, |text| {
        let mut owned = String::new();
        owned
            .try_reserve_exact(text.as_str().len())
            .map_err(|_| RuntimeError::resource_limit("value hashing string"))?;
        owned.push_str(text.as_str());
        Ok(KeyPart::Str(owned))
    })
}

pub(crate) fn builtin_hash<'gc>(
    cx: &Scope<'gc>,
    resources: &ResourceState,
    value: ScopedValue<'gc>,
) -> Result<Option<i64>, RuntimeError> {
    let Some(key) = StructuralKey::from_value(cx, resources, value)? else {
        return Ok(None);
    };
    let mut hasher = DefaultHasher::new();
    key.hash(&mut hasher);
    Ok(Some(hasher.finish() as i64))
}

#[cfg(test)]
mod tests {
    use super::builtin_hash;
    use crate::{
        resource::ResourceState,
        value::{aggregates::tuples::TupleObject, carriers::ScopedValue, strings::StringObject},
    };
    use kagari_gc::{error::GcError, heap::Heap};

    #[test]
    fn builtin_hash_uses_value_content_and_rejects_unsupported_domains() -> Result<(), GcError> {
        let mut heap = Heap::default();
        let resources = ResourceState::default();
        heap.scope(|cx| {
            let first = StringObject::alloc(cx, "é🙂".into())?;
            let second = StringObject::alloc(cx, "é🙂".into())?;
            assert!(!first.ptr_eq(second));
            let hash = |value| builtin_hash(cx, &resources, value).unwrap();
            assert_eq!(
                hash(ScopedValue::managed(first)),
                hash(ScopedValue::managed(second))
            );
            assert!(hash(ScopedValue::managed(first)).is_some());
            let left =
                TupleObject::alloc(cx, &[ScopedValue::managed(first), ScopedValue::from(7_i32)])?;
            let right = TupleObject::alloc(
                cx,
                &[ScopedValue::managed(second), ScopedValue::from(7_i32)],
            )?;
            assert_eq!(
                builtin_hash(cx, &resources, ScopedValue::managed(left)).unwrap(),
                builtin_hash(cx, &resources, ScopedValue::managed(right)).unwrap()
            );
            assert!(
                builtin_hash(cx, &resources, ScopedValue::managed(left))
                    .unwrap()
                    .is_some()
            );
            assert!(
                builtin_hash(cx, &resources, ScopedValue::from(1.0_f64))
                    .unwrap()
                    .is_none()
            );
            assert!(
                builtin_hash(cx, &resources, ScopedValue::unit())
                    .unwrap()
                    .is_some()
            );
            // One tuple marker plus 65536 leaves cannot fit the current part
            // bound. Rejection must precede an unbounded traversal allocation.
            let boundary = TupleObject::alloc(cx, &vec![ScopedValue::unit(); 65536])?;
            assert!(
                builtin_hash(cx, &resources, ScopedValue::managed(boundary))
                    .unwrap()
                    .is_none()
            );
            Ok(())
        })?;
        assert_eq!(heap.stats().root_registrations, 0);
        Ok(())
    }
}
