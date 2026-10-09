use super::string_iter::StringTraversal;
use crate::{
    error::{RuntimeError, RuntimeErrorKind},
    frame::types::{arguments::TypeArgument, bindings::TypeBindings},
    gc::{
        GcHeap, GcObjectKind, HeapObjectId,
        leases::{LeaseScope, OwnedLease},
        storage::HeapObject,
    },
    module::LoadedModule,
    native::{storage::NativePayload, storage_type::StorageType},
    value::{MapKey, Value},
    value_check::matches_type,
};
use kagari_common::identity::table::DefinitionId;
use kagari_contract::operations::{IterOp, StringIterKind};
use kagari_types::{scalar::BuiltinType, ty::Ty};

struct IterTypeScope<'a> {
    source: &'a TypeArgument,
    item: StorageType,
}

#[derive(Debug)]
pub(super) struct NativeIter {
    pub(super) source: Value,
    pub(super) item_type: Ty<DefinitionId>,
    pub(super) item_contract: StorageType,
    pub(super) position: u128,
    pub(super) string: Option<StringTraversal>,
    /// Unordered tables retain keys once; next performs a direct checked lookup.
    pub(super) keys: Vec<MapKey>,
    pub(super) revision: u64,
    pub(super) guard: Option<OwnedLease>,
}

impl NativePayload for NativeIter {
    fn trace<'payload>(&'payload self, visit: &mut dyn FnMut(&'payload Value)) {
        visit(&self.source);
        for key in &self.keys {
            visit(key.value());
        }
    }

    fn units(&self) -> usize {
        1 + self.keys.len()
    }
}

fn invalid() -> RuntimeError {
    RuntimeError::new(
        RuntimeErrorKind::ScriptTrap,
        "invalid or structurally modified iterator",
    )
}

impl GcHeap {
    fn begin_iteration_lease(
        &self,
        source: &Value,
        scope: &LeaseScope,
    ) -> Result<OwnedLease, RuntimeError> {
        self.ensure_execution_allowed()?;
        let (id, expected) = match source {
            Value::Array(id) => (*id, GcObjectKind::Array),
            Value::Map(id) => (*id, GcObjectKind::Map),
            Value::Set(id) => (*id, GcObjectKind::Set),
            Value::Str(_) | Value::Range(_) => return Ok(OwnedLease::new(Some(scope))),
            _ => return Err(invalid()),
        };
        if self.object_kind(id) != Some(expected) {
            return Err(invalid());
        }
        self.iterations
            .acquire(id, Some(scope))
            .map_err(|_| self.resource_limit("iterator registry"))
    }

    /// Reopening an indexed adapter must validate and protect its retained source.
    pub fn resume_iter(&self, value: &Value) -> Result<(), RuntimeError> {
        self.ensure_execution_allowed()?;
        let session = self.resources.active_session().ok_or_else(invalid)?;
        self.resources.poll_execution()?;
        let Value::GcHandle(id) = value else {
            return Err(invalid());
        };
        let (source, revision, needs_guard) = {
            let objects = self.objects.borrow();
            let Some(HeapObject::Native(object)) = self.readable_object(&objects, *id) else {
                return Err(invalid());
            };
            let iter = object.payload::<NativeIter>()?;
            (
                iter.source.clone(),
                iter.revision,
                !iter.guard.as_ref().is_some_and(OwnedLease::is_active)
                    && !self.iterator_loops.is_active(*id),
            )
        };
        if self.collection_revision(&source) != Some(revision) {
            return Err(invalid());
        }
        if needs_guard {
            let guard = self.begin_iteration_lease(&source, &session.leases)?;
            let mut objects = self.objects_mut()?;
            let Some(HeapObject::Native(object)) = self.object_mut(&mut objects, *id) else {
                return Err(invalid());
            };
            let iter = object.payload_mut::<NativeIter>()?;
            iter.guard = Some(guard);
        }
        Ok(())
    }

    pub(super) fn validate_iter(
        &self,
        value: &Value,
        ty: &Ty<DefinitionId>,
    ) -> Result<(), RuntimeError> {
        self.ensure_execution_allowed()?;
        let (Value::GcHandle(id), Ty::Iter(item)) = (value, ty) else {
            return Err(invalid());
        };
        let objects = self.objects.borrow();
        let Some(HeapObject::Native(object)) = self.readable_object(&objects, *id) else {
            return Err(invalid());
        };
        let iter = object.payload::<NativeIter>()?;
        if iter.item_type != **item {
            return Err(invalid());
        }
        Ok(())
    }

    fn close_iter_tree(&self, value: &Value) -> Result<(), RuntimeError> {
        let Value::GcHandle(id) = value else {
            return Err(invalid());
        };
        let mut objects = self.objects_mut()?;
        let Some(HeapObject::Native(object)) = self.object_mut(&mut objects, *id) else {
            return Err(invalid());
        };
        let iter = object.payload_mut::<NativeIter>()?;
        iter.guard = None;
        Ok(())
    }

    pub(super) fn collection_revision(&self, source: &Value) -> Option<u64> {
        match source {
            Value::Str(_) | Value::Range(_) => Some(0),
            Value::Array(id) | Value::Map(id) | Value::Set(id) => {
                let objects = self.objects.borrow();
                self.readable_object(&objects, *id)?;
                Some(objects[id.index()].revision)
            }
            _ => None,
        }
    }

    pub(crate) fn matches_iter_type(
        &self,
        id: HeapObjectId,
        element: &Ty<DefinitionId>,
        owner: &LoadedModule,
        environment: Option<&TypeBindings>,
    ) -> bool {
        let objects = self.objects.borrow();
        matches!(self.readable_object(&objects, id), Some(HeapObject::Native(object)) if matches!(object.ty, Ty::Iter(_)) && object.payload::<NativeIter>().is_ok_and(|iter| iter.item_contract.matches_scoped(element, owner, environment)))
    }

    pub(crate) fn new_iter(
        &self,
        source: &Value,
        ty: &TypeArgument,
        item: TypeArgument,
        owner: &LoadedModule,
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
        ty: &Ty<DefinitionId>,
        traversal: Option<(Ty<DefinitionId>, StringTraversal)>,
        owner: &LoadedModule,
        scope: Option<IterTypeScope<'_>>,
    ) -> Result<Value, RuntimeError> {
        self.ensure_execution_allowed()?;
        let valid = match (source, ty) {
            (Value::Array(id), Ty::Array(_, _)) => {
                self.object_kind(*id) == Some(GcObjectKind::Array)
            }
            (Value::Set(id), Ty::Set(_, _)) => self.object_kind(*id) == Some(GcObjectKind::Set),
            (Value::Map(id), Ty::Map { .. }) => self.object_kind(*id) == Some(GcObjectKind::Map),
            (Value::Range(range), Ty::Range(_, kind)) => kind.has_start() && range.matches(ty),
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
            Ty::Range(item, _) | Ty::Array(item, _) | Ty::Set(item, _) => (**item).clone(),
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
        let session = self.resources.active_session().ok_or_else(invalid)?;
        let guard = Some(self.begin_iteration_lease(source, &session.leases)?);
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
            source: source.clone(),
            item_type,
            item_contract,
            position: 0,
            string,
            keys,
            revision,
            guard,
        };
        let object = self
            .cursor_storage
            .prepare_payload(self, &cursor_type, payload, owner)?;
        let id = self.alloc_native(object)?;
        Ok(Value::GcHandle(id))
    }

    pub(crate) fn new_string_iter(
        &self,
        source: &Value,
        ty: &Ty<DefinitionId>,
        kind: StringIterKind,
        owner: &LoadedModule,
    ) -> Result<Value, RuntimeError> {
        if !kind.valid_source(ty) {
            return Err(invalid());
        }
        let Value::Tuple(fields) = source else {
            return Err(invalid());
        };
        let traversal = StringTraversal::new(kind, fields)?;
        self.new_iter_with(
            &fields[0],
            &Ty::Builtin(BuiltinType::String),
            Some((kind.item_type(), traversal)),
            owner,
            None,
        )
    }

    pub(crate) fn advance_iter(
        &self,
        value: &Value,
        ty: &Ty<DefinitionId>,
        op: IterOp,
    ) -> Result<Value, RuntimeError> {
        self.ensure_execution_allowed()?;
        if op == IterOp::Close {
            self.validate_iter(value, ty)?;
            self.close_iter_tree(value)?;
            return Ok(Value::Unit);
        }
        Err(invalid())
    }

    pub(crate) fn next_iter_item(
        &self,
        value: &Value,
        ty: &Ty<DefinitionId>,
    ) -> Result<Option<Value>, RuntimeError> {
        self.advance_iter_with(value, ty, Ok)
    }

    pub(crate) fn advance_iter_with<R>(
        &self,
        value: &Value,
        ty: &Ty<DefinitionId>,
        finish: impl FnOnce(Option<Value>) -> Result<R, RuntimeError>,
    ) -> Result<R, RuntimeError> {
        self.ensure_execution_allowed()?;
        let (Value::GcHandle(id), Ty::Iter(item)) = (value, ty) else {
            return Err(invalid());
        };
        let (needs_guard, payload, next_position, string_cursor) = {
            let objects = self.objects.borrow();
            let Some(HeapObject::Native(object)) = self.readable_object(&objects, *id) else {
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
                let (value, cursor) = traversal.preview(text)?;
                string_cursor = Some(cursor);
                (value, 0)
            } else {
                match &iter.source {
                    Value::Range(range) => (range.at(iter.position)?, 1),
                    Value::Array(id) => (self.array_get(*id, iter.position as usize), 1),
                    Value::Set(id) => (
                        self.with_set(*id, |values| {
                            iter.keys
                                .get(iter.position as usize)
                                .and_then(|key| values.get(key))
                                .map(MapKey::to_value)
                        })
                        .ok_or_else(invalid)?,
                        1,
                    ),
                    Value::Map(id) => (
                        self.with_map(*id, |entries| {
                            iter.keys.get(iter.position as usize).and_then(|key| {
                                entries
                                    .get(key)
                                    .map(|value| Value::Tuple(vec![key.to_value(), value.clone()]))
                            })
                        })
                        .ok_or_else(invalid)?,
                        1,
                    ),
                    Value::Str(text) => match text
                        .get(iter.position as usize..)
                        .and_then(|tail| tail.chars().next())
                    {
                        Some(character) => (
                            Some(Value::Str(character.to_string())),
                            character.len_utf8() as u128,
                        ),
                        None => (None, 0),
                    },
                    _ => return Err(invalid()),
                }
            };
            (
                !iter.guard.as_ref().is_some_and(OwnedLease::is_active)
                    && !self.iterator_loops.is_active(*id),
                payload,
                iter.position.checked_add(advance).ok_or_else(invalid)?,
                string_cursor,
            )
        };
        let session = self.resources.active_session().ok_or_else(invalid)?;
        let new_guard = if needs_guard && payload.is_some() {
            let source = {
                let objects = self.objects.borrow();
                let Some(HeapObject::Native(object)) = self.readable_object(&objects, *id) else {
                    return Err(invalid());
                };
                object.payload::<NativeIter>()?.source.clone()
            };
            Some(self.begin_iteration_lease(&source, &session.leases)?)
        } else {
            None
        };
        // Prepare the public result before committing. Native adapters can take
        // an item directly, without allocating an intermediate script Option.
        drop(session);
        let has_item = payload.is_some();
        let result = finish(payload)?;
        let mut objects = self.objects_mut()?;
        let Some(HeapObject::Native(object)) = self.object_mut(&mut objects, *id) else {
            return Err(invalid());
        };
        let iter = object.payload_mut::<NativeIter>()?;
        if let (Some(traversal), Some(cursor)) = (&mut iter.string, string_cursor) {
            traversal.cursor = cursor;
        }
        if has_item {
            iter.position = next_position;
            if needs_guard {
                iter.guard = new_guard;
            }
        } else {
            iter.guard = None;
        }
        Ok(result)
    }
}
