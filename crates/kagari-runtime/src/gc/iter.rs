use super::string_iter::StringTraversal;
use crate::{
    error::{RuntimeError, RuntimeErrorKind},
    frame::types::{arguments::TypeArgument, compatibility::TypeView},
    gc::{GcHeap, GcObjectKind, HeapObjectId, storage::HeapObject},
    module::ExecutableModule,
    native::{
        binding::NativeResult,
        sequence::SequencePayload,
        storage::{NativePayload, NativeStorage},
        storage_type::StorageType,
    },
    resource::HeapCharge,
    value::{MapKey, Value},
    value_check::matches_type,
};

use kagari_contract::operations::StringIterKind;
use kagari_types::{declaration::native::NativeStorageLayout, scalar::BuiltinType, ty::Ty};
use std::slice;

struct IterTypeScope<'a> {
    source: &'a TypeArgument,
    item: StorageType,
}

/// Owned preview data, materialized only after releasing iterator storage views.
pub(super) enum PendingItem {
    Value(Value),
    Text(String),
    Pair(Value, Value),
}

impl PendingItem {
    fn materialize(self, heap: &GcHeap) -> Result<Value, RuntimeError> {
        match self {
            Self::Value(value) => Ok(value),
            Self::Text(text) => heap.alloc_string(text),
            Self::Pair(a, b) => heap.alloc_tuple(vec![a, b]),
        }
    }
}

#[derive(Debug)]
pub(super) struct NativeIter {
    pub(super) source: Value,
    pub(super) item_type: Ty,
    pub(super) item_contract: StorageType,
    pub(super) position: u128,
    pub(super) string: Option<StringTraversal>,
    /// Unordered tables retain keys once; next performs a direct checked lookup.
    pub(super) keys: Vec<MapKey>,
    pub(super) revision: u64,
}

impl NativePayload for NativeIter {
    fn trace<'payload>(&'payload self, visit: &mut dyn FnMut(&'payload Value)) {
        visit(&self.source);
        if let Some(string) = &self.string {
            string.trace(visit);
        }
        for key in &self.keys {
            visit(key.value());
        }
    }

    fn charge(&self) -> NativeResult<HeapCharge> {
        Ok(HeapCharge::new(
            1 + self.keys.len(),
            self.keys
                .capacity()
                .checked_mul(size_of::<MapKey>())
                .expect("allocated iterator keys"),
        ))
    }
}

fn invalid() -> RuntimeError {
    RuntimeError::new(
        RuntimeErrorKind::ScriptTrap,
        "invalid or structurally modified iterator",
    )
}

impl GcHeap {
    /// Structural revision used by native cursors and callback-driven traversals.
    /// Element replacement does not change this revision. Invalid handles and
    /// sequence storage temporarily detached for editing have no readable revision.
    pub fn collection_revision(&self, source: &Value) -> Option<u64> {
        match source {
            Value::Str(_) | Value::Range(_) => Some(0),
            Value::Array(id) | Value::GcHandle(id) | Value::Map(id) | Value::Set(id) => {
                let objects = self.objects.borrow();
                let HeapObject::Native(object) = self.readable_object(&objects, *id)? else {
                    return None;
                };
                if object.storage.accepts_payload::<SequencePayload>()
                    && object.payload::<SequencePayload>().ok()?.detached
                {
                    return None;
                }
                Some(object.revision)
            }
            _ => None,
        }
    }

    pub(crate) fn matches_iter_type(&self, id: HeapObjectId, element: TypeView<'_>) -> bool {
        let objects = self.objects.borrow();
        matches!(self.readable_object(&objects, id), Some(HeapObject::Native(object)) if matches!(object.ty, Ty::Iter(_)) && object.payload::<NativeIter>().is_ok_and(|iter| iter.item_contract.matches_view(element)))
    }

    /// Reject a stale traversal before reading another element. Writes already
    /// performed by callbacks remain committed; no collection borrow is retained.
    pub fn check_collection_revision(
        &self,
        source: &Value,
        revision: u64,
    ) -> Result<(), RuntimeError> {
        self.ensure_execution_allowed()?;
        if self.collection_revision(source) != Some(revision) {
            return Err(invalid());
        }
        Ok(())
    }

    pub(crate) fn new_iter(
        &self,
        source: &Value,
        ty: &TypeArgument,
        item: TypeArgument,
        owner: &ExecutableModule,
    ) -> Result<Value, RuntimeError> {
        let scope = IterTypeScope {
            source: ty,
            item: StorageType::prepare_scoped(item, owner)?,
        };
        self.new_iter_with(source, ty.ty(), None, owner, Some(scope))
    }

    fn new_iter_with(
        &self,
        source: &Value,
        ty: &Ty,
        traversal: Option<(Ty, StringTraversal)>,
        owner: &ExecutableModule,
        scope: Option<IterTypeScope<'_>>,
    ) -> Result<Value, RuntimeError> {
        self.ensure_execution_allowed()?;
        let valid = match (source, ty) {
            (Value::Array(id), Ty::Array(_)) => self.object_kind(*id) == Some(GcObjectKind::Array),
            (Value::GcHandle(id), Ty::NativeObject(_)) => self.ensure_sequence(*id).is_ok(),
            (Value::Set(id), Ty::Set(_, _)) => self.object_kind(*id) == Some(GcObjectKind::Set),
            (Value::Map(id), Ty::Map { .. }) => self.object_kind(*id) == Some(GcObjectKind::Map),
            (Value::Range(range), Ty::Range(_, kind)) => {
                kind.has_start() && self.range(*range).is_some_and(|range| range.matches(ty))
            }
            (Value::Str(_), Ty::Builtin(BuiltinType::String)) => true,
            _ => false,
        };
        if !valid
            || !match &scope {
                Some(scope) => scope.source.matches_heap(self, source, owner),
                None => matches_type(self, source, ty, owner),
            }
        {
            return Err(invalid());
        }
        let item_type = match ty {
            Ty::Range(item, _) | Ty::Array(item) | Ty::Set(item, _) => (**item).clone(),
            Ty::NativeObject(_) => {
                let Value::GcHandle(id) = source else {
                    return Err(invalid());
                };
                self.sequence_contract(*id).ok_or_else(invalid)?.ty.clone()
            }
            Ty::Map { key, value, .. } => Ty::Tuple(vec![(**key).clone(), (**value).clone()]),
            Ty::Builtin(BuiltinType::String) => ty.clone(),
            _ => return Err(invalid()),
        };
        let (item_type, string) = match traversal {
            Some((item, traversal)) => (item, Some(traversal)),
            None => (item_type, None),
        };
        let item_contract = match scope {
            Some(scope) if scope.item.ty == item_type => scope.item,
            Some(_) => return Err(invalid()),
            None => StorageType::prepare(item_type.clone(), owner)?,
        };
        let revision = self.collection_revision(source).ok_or_else(invalid)?;
        let count = match source {
            Value::Map(id) => self.map_len(*id),
            Value::Set(id) => self.set_len(*id),
            _ => Some(0),
        }
        .ok_or_else(invalid)?;
        let mut keys = Vec::new();
        keys.try_reserve_exact(count)
            .map_err(|_| self.resource_limit("iterator keys"))?;
        match source {
            Value::Map(id) => self
                .with_map(*id, |entries| keys.extend(entries.keys().cloned()))
                .ok_or_else(invalid)?,
            Value::Set(id) => self
                .with_set(*id, |entries| keys.extend(entries.iter().cloned()))
                .ok_or_else(invalid)?,
            _ => {}
        }
        let cursor_type = Ty::Iter(Box::new(item_type.clone()));
        let payload = NativeIter {
            source: *source,
            item_type,
            item_contract,
            position: 0,
            string,
            keys,
            revision,
        };
        let object = self
            .cursor_storage
            .get_or_init(|| {
                NativeStorage::provided_with_layout::<NativeIter>(NativeStorageLayout::Iterator {
                    item: 0,
                })
            })
            .prepare_payload(self, &cursor_type, payload, owner)?;
        let id = self.alloc_native(object)?;
        Ok(Value::GcHandle(id))
    }

    pub(crate) fn new_string_iter(
        &self,
        source: &Value,
        ty: &Ty,
        kind: StringIterKind,
        owner: &ExecutableModule,
    ) -> Result<Value, RuntimeError> {
        if !kind.valid_source(ty) {
            return Err(invalid());
        }
        let (source, traversal) = match (ty, source) {
            (Ty::Builtin(BuiltinType::String), Value::Str(_)) => (
                *source,
                StringTraversal::new(kind, slice::from_ref(source))?,
            ),
            (Ty::Tuple(_), Value::Tuple(fields)) => {
                let fields = self.tuple(*fields).ok_or_else(invalid)?;
                let traversal = StringTraversal::new(kind, &fields)?;
                (fields[0], traversal)
            }
            _ => return Err(invalid()),
        };
        self.new_iter_with(
            &source,
            &Ty::Builtin(BuiltinType::String),
            Some((kind.item_type(), traversal)),
            owner,
            None,
        )
    }

    pub(crate) fn advance_iter_with<R>(
        &self,
        value: &Value,
        ty: &Ty,
        finish: impl FnOnce(Option<Value>) -> Result<R, RuntimeError>,
    ) -> Result<R, RuntimeError> {
        self.ensure_execution_allowed()?;
        let (Value::GcHandle(id), Ty::Iter(item)) = (value, ty) else {
            return Err(invalid());
        };
        let (payload, next_position, string_cursor) = {
            let objects = self.objects.borrow();
            let Some(object) = self.native_ref(&objects, *id) else {
                return Err(invalid());
            };
            let iter = object.payload::<NativeIter>()?;
            if iter.item_type != **item
                || self.collection_revision(&iter.source) != Some(iter.revision)
            {
                return Err(invalid());
            }
            let mut string_cursor = None;
            let (payload, advance) = if let Some(traversal) = &iter.string {
                let Value::Str(text) = &iter.source else {
                    return Err(invalid());
                };
                let (value, cursor) =
                    traversal.preview(self, &self.string(*text).ok_or_else(invalid)?)?;
                string_cursor = Some(cursor);
                (value, 0)
            } else {
                match &iter.source {
                    Value::Range(range) => (
                        self.range(*range)
                            .ok_or_else(invalid)?
                            .at(iter.position)?
                            .map(PendingItem::Value),
                        1,
                    ),
                    Value::Array(id) | Value::GcHandle(id) => (
                        self.buffer_get(*id, iter.position).map(PendingItem::Value),
                        1,
                    ),
                    Value::Set(id) => (
                        self.with_set(*id, |values| {
                            usize::try_from(iter.position)
                                .ok()
                                .and_then(|position| iter.keys.get(position))
                                .and_then(|key| values.get(key))
                                .map(|key| PendingItem::Value(key.to_value()))
                        })
                        .ok_or_else(invalid)?,
                        1,
                    ),
                    Value::Map(id) => (
                        self.with_map(*id, |entries| {
                            usize::try_from(iter.position)
                                .ok()
                                .and_then(|position| iter.keys.get(position))
                                .and_then(|key| {
                                    entries
                                        .get(key)
                                        .map(|value| PendingItem::Pair(key.to_value(), *value))
                                })
                        })
                        .ok_or_else(invalid)?,
                        1,
                    ),
                    Value::Str(text) => {
                        let text = self.string(*text).ok_or_else(invalid)?;
                        let character = usize::try_from(iter.position)
                            .ok()
                            .and_then(|position| text.get(position..))
                            .and_then(|tail| tail.chars().next());
                        match character {
                            Some(character) => (
                                Some(PendingItem::Text(character.to_string())),
                                character.len_utf8() as u128,
                            ),
                            None => (None, 0),
                        }
                    }
                    _ => return Err(invalid()),
                }
            };
            (
                payload,
                iter.position.checked_add(advance).ok_or_else(invalid)?,
                string_cursor,
            )
        };
        let payload = payload.map(|item| item.materialize(self)).transpose()?;
        let _payload_root = payload
            .as_ref()
            .filter(|value| value.object_id().is_some())
            .map(|value| self.root_value(*value).ok_or_else(invalid))
            .transpose()?;
        let has_item = payload.is_some();
        let result = finish(payload)?;
        let mut objects = self.objects_mut()?;
        let Some(object) = self.native_mut(&mut objects, *id) else {
            return Err(invalid());
        };
        let iter = object.payload_mut::<NativeIter>()?;
        if let (Some(traversal), Some(cursor)) = (&mut iter.string, string_cursor) {
            traversal.cursor = cursor;
        }
        if has_item {
            iter.position = next_position;
        }
        Ok(result)
    }
}
