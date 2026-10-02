use super::string_iter::StringTraversal;
use crate::{
    error::{RuntimeError, RuntimeErrorKind},
    gc::{GcHeap, GcObjectKind, HeapObject, HeapObjectId},
    module::LoadedModule,
    native::{storage::NativePayload, storage_type::StorageType},
    session::SessionState,
    value::{EnumTag, MapKey, Value},
};
use kagari_abi::{
    operations::{IterOp, StringIterKind},
    scalar::BuiltinType,
    types::AbiType,
};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::{Rc, Weak},
};

#[derive(Debug)]
pub(super) struct NativeIter {
    pub(super) source: Value,
    pub(super) item_type: AbiType,
    pub(super) item_contract: StorageType,
    pub(super) position: u128,
    pub(super) string: Option<StringTraversal>,
    /// Unordered tables retain keys once; next performs a direct checked lookup.
    pub(super) keys: Vec<MapKey>,
    pub(super) revision: u64,
    pub(super) guard: Option<IterationLease>,
    pub(super) loops: Rc<Cell<usize>>,
    pub(super) session: Weak<SessionState>,
}

#[derive(Debug)]
pub(super) struct IterationLease {
    active: Rc<RefCell<HashMap<HeapObjectId, usize>>>,
    id: Option<HeapObjectId>,
}
impl Drop for IterationLease {
    fn drop(&mut self) {
        if let Some(id) = self.id {
            let mut active = self.active.borrow_mut();
            let count = active.get_mut(&id).expect("registered cursor source");
            *count -= 1;
            if *count == 0 {
                active.remove(&id);
            }
        }
    }
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
    fn begin_iteration_lease(&self, source: &Value) -> Result<IterationLease, RuntimeError> {
        self.ensure_execution_allowed()?;
        let (id, expected) = match source {
            Value::Array(id) => (Some(*id), GcObjectKind::Array),
            Value::Map(id) => (Some(*id), GcObjectKind::Map),
            Value::Set(id) => (Some(*id), GcObjectKind::Set),
            Value::Str(_) | Value::Range(_) => (None, GcObjectKind::Iter),
            _ => return Err(invalid()),
        };
        if let Some(id) = id {
            if self.object_kind(id) != Some(expected) {
                return Err(invalid());
            }
            let mut active = self.iterations.borrow_mut();
            let count = active
                .get(&id)
                .copied()
                .unwrap_or(0)
                .checked_add(1)
                .ok_or_else(invalid)?;
            active
                .try_reserve(1)
                .map_err(|_| self.resource_limit("iterator registry"))?;
            active.insert(id, count);
        }
        Ok(IterationLease {
            active: self.iterations.clone(),
            id,
        })
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
                iter.guard.is_none() && iter.loops.get() == 0,
            )
        };
        if self.collection_revision(&source) != Some(revision) {
            return Err(invalid());
        }
        if needs_guard {
            session
                .iter_guards
                .borrow_mut()
                .try_reserve(1)
                .map_err(|_| self.resource_limit("iterator registry"))?;
            let guard = self.begin_iteration_lease(&source)?;
            let mut objects = self.objects.borrow_mut();
            let Some(HeapObject::Native(object)) = self.object_mut(&mut objects, *id) else {
                return Err(invalid());
            };
            let iter = object.payload_mut::<NativeIter>()?;
            iter.guard = Some(guard);
            iter.session = Rc::downgrade(&session);
            session.iter_guards.borrow_mut().insert(*id);
        }
        Ok(())
    }

    pub(super) fn validate_iter(&self, value: &Value, ty: &AbiType) -> Result<(), RuntimeError> {
        self.ensure_execution_allowed()?;
        let (Value::GcHandle(id), AbiType::Iter(item)) = (value, ty) else {
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
        let mut objects = self.objects.borrow_mut();
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
                Some(objects[id.slot].revision)
            }
            _ => None,
        }
    }
    pub(crate) fn new_iter(
        &self,
        source: &Value,
        ty: &AbiType,
        owner: &LoadedModule,
    ) -> Result<Value, RuntimeError> {
        self.new_iter_with(source, ty, None, owner)
    }
    fn new_iter_with(
        &self,
        source: &Value,
        ty: &AbiType,
        traversal: Option<(AbiType, StringTraversal)>,
        owner: &LoadedModule,
    ) -> Result<Value, RuntimeError> {
        self.ensure_execution_allowed()?;
        let valid = match (source, ty) {
            (Value::Array(id), AbiType::Array(_, _)) => {
                self.object_kind(*id) == Some(GcObjectKind::Array)
            }
            (Value::Set(id), AbiType::Set(_, _)) => {
                self.object_kind(*id) == Some(GcObjectKind::Set)
            }
            (Value::Map(id), AbiType::Map { .. }) => {
                self.object_kind(*id) == Some(GcObjectKind::Map)
            }
            (Value::Range(range), AbiType::Range(_, kind)) => kind.has_start() && range.matches(ty),
            (Value::Str(_), AbiType::Builtin(BuiltinType::String)) => true,
            _ => false,
        };
        if !valid || !self.matches_abi(source, ty, owner) {
            return Err(invalid());
        }
        let item_type = match ty {
            AbiType::Range(item, _) | AbiType::Array(item, _) | AbiType::Set(item, _) => {
                (**item).clone()
            }
            AbiType::Map { key, value, .. } => {
                AbiType::Tuple(vec![(**key).clone(), (**value).clone()])
            }
            AbiType::Builtin(BuiltinType::String) => ty.clone(),
            _ => return Err(invalid()),
        };
        let (item_type, string) = match traversal {
            Some((item, traversal)) => (item, Some(traversal)),
            None => (item_type, None),
        };
        let item_contract = StorageType::prepare(item_type.clone(), owner)?;
        let revision = self.collection_revision(source).ok_or_else(invalid)?;
        let session = self.resources.active_session().ok_or_else(invalid)?;
        session
            .iter_guards
            .borrow_mut()
            .try_reserve(1)
            .map_err(|_| self.resource_limit("iterator registry"))?;
        let guard = Some(self.begin_iteration_lease(source)?);
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
        let cursor_type = AbiType::Iter(Box::new(item_type.clone()));
        let payload = NativeIter {
            source: source.clone(),
            item_type,
            item_contract,
            position: 0,
            string,
            keys,
            revision,
            guard,
            loops: Rc::new(Cell::new(0)),
            session: Rc::downgrade(&session),
        };
        let object = self
            .cursor_storage
            .prepare_payload(self, &cursor_type, payload, owner)?;
        let id = self.alloc_native(object)?;
        session.iter_guards.borrow_mut().insert(id);
        Ok(Value::GcHandle(id))
    }
    pub(crate) fn new_string_iter(
        &self,
        source: &Value,
        ty: &AbiType,
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
            &AbiType::Builtin(BuiltinType::String),
            Some((kind.item_type(), traversal)),
            owner,
        )
    }

    pub(crate) fn advance_iter(
        &self,
        value: &Value,
        ty: &AbiType,
        op: IterOp,
    ) -> Result<Value, RuntimeError> {
        self.ensure_execution_allowed()?;
        if op == IterOp::Close {
            self.validate_iter(value, ty)?;
            self.close_iter_tree(value)?;
            return Ok(Value::Unit);
        }
        if op != IterOp::Next {
            return Err(invalid());
        }
        self.advance_iter_with(value, ty, |payload| {
            let tag = if payload.is_some() {
                EnumTag::OptionSome
            } else {
                EnumTag::OptionNone
            };
            self.alloc_enum(tag, payload.into_iter().collect())
                .map(Value::Enum)
        })
    }
    pub(crate) fn next_iter_item(
        &self,
        value: &Value,
        ty: &AbiType,
    ) -> Result<Option<Value>, RuntimeError> {
        self.advance_iter_with(value, ty, Ok)
    }
    fn advance_iter_with<R>(
        &self,
        value: &Value,
        ty: &AbiType,
        finish: impl FnOnce(Option<Value>) -> Result<R, RuntimeError>,
    ) -> Result<R, RuntimeError> {
        self.ensure_execution_allowed()?;
        let (Value::GcHandle(id), AbiType::Iter(item)) = (value, ty) else {
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
                iter.guard.is_none() && iter.loops.get() == 0,
                payload,
                iter.position.checked_add(advance).ok_or_else(invalid)?,
                string_cursor,
            )
        };
        let session = self.resources.active_session().ok_or_else(invalid)?;
        let new_guard = if needs_guard && payload.is_some() {
            session
                .iter_guards
                .borrow_mut()
                .try_reserve(1)
                .map_err(|_| self.resource_limit("iterator registry"))?;
            let source = {
                let objects = self.objects.borrow();
                let Some(HeapObject::Native(object)) = self.readable_object(&objects, *id) else {
                    return Err(invalid());
                };
                object.payload::<NativeIter>()?.source.clone()
            };
            Some(self.begin_iteration_lease(&source)?)
        } else {
            None
        };
        // Prepare the public result before committing. Native adapters can take
        // an item directly, without allocating an intermediate script Option.
        let has_item = payload.is_some();
        let result = finish(payload)?;
        let mut objects = self.objects.borrow_mut();
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
                session.iter_guards.borrow_mut().insert(*id);
                iter.guard = new_guard;
                iter.session = Rc::downgrade(&session);
            }
        } else {
            iter.guard = None;
        }
        Ok(result)
    }
    pub(crate) fn release_iter_guards(&self, session: &Rc<SessionState>) {
        let mut objects = self.objects.borrow_mut();
        for id in session.iter_guards.borrow_mut().drain() {
            if id.owner != self.owner {
                continue;
            }
            let Some(slot) = objects.get_mut(id.slot) else {
                continue;
            };
            if slot.generation != id.generation {
                continue;
            }
            if let Some(HeapObject::Native(object)) = &mut slot.object
                && let Ok(iter) = object.payload_mut::<NativeIter>()
                && iter.session.ptr_eq(&Rc::downgrade(session))
            {
                iter.guard = None;
            }
        }
    }
}
