use super::string_iter::StringTraversal;
use crate::{
    error::{RuntimeError, RuntimeErrorKind},
    gc::{CollectionIteration, GcHeap, GcObjectKind, HeapObject},
    module::{LoadedModule, RetainedRuntimeProgram},
    session::SessionState,
    value::{EnumTag, MapKey, Value},
};
use kagari_abi::{
    operations::{IterOp, StringIterKind},
    scalar::BuiltinType,
    types::AbiType,
};
use std::{
    cell::Cell,
    rc::{Rc, Weak},
};

#[derive(Debug)]
pub(super) struct NativeIter {
    pub(super) source: Value,
    pub(super) item_type: AbiType,
    pub(super) position: u128,
    pub(super) string: Option<StringTraversal>,
    /// Unordered tables retain keys once; next performs a direct checked lookup.
    pub(super) keys: Vec<MapKey>,
    pub(super) revision: u64,
    pub(super) guard: Option<CollectionIteration>,
    pub(super) loops: Rc<Cell<usize>>,
    pub(super) session: Weak<SessionState>,
    pub(super) owner: LoadedModule,
    pub(super) _retention: RetainedRuntimeProgram,
}

fn invalid() -> RuntimeError {
    RuntimeError::new(
        RuntimeErrorKind::ScriptTrap,
        "invalid or structurally modified iterator",
    )
}

impl GcHeap {
    /// Reopening an indexed adapter must validate and protect its retained source.
    pub fn resume_iter(&self, value: &Value) -> Result<(), RuntimeError> {
        self.ensure_execution_allowed()?;
        let session = self.resources.active_session().ok_or_else(invalid)?;
        self.resources.consume_instruction_steps(1)?;
        let Value::GcHandle(id) = value else {
            return Err(invalid());
        };
        let (source, revision, needs_guard) = {
            let objects = self.objects.borrow();
            let Some(HeapObject::Iter(iter)) = self.readable_object(&objects, *id) else {
                return Err(invalid());
            };
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
            let guard = self.begin_collection_iteration(&source)?;
            let mut objects = self.objects.borrow_mut();
            let Some(HeapObject::Iter(iter)) = self.object_mut(&mut objects, *id) else {
                return Err(invalid());
            };
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
        let Some(HeapObject::Iter(iter)) = self.readable_object(&objects, *id) else {
            return Err(invalid());
        };
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
        let Some(HeapObject::Iter(iter)) = self.object_mut(&mut objects, *id) else {
            return Err(invalid());
        };
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
        retention: RetainedRuntimeProgram,
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
        if !valid {
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
        let revision = self.collection_revision(source).ok_or_else(invalid)?;
        let session = self.resources.active_session().ok_or_else(invalid)?;
        session
            .iter_guards
            .borrow_mut()
            .try_reserve(1)
            .map_err(|_| self.resource_limit("iterator registry"))?;
        let guard = Some(self.begin_collection_iteration(source)?);
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
        let id = self.alloc_object(HeapObject::Iter(Box::new(NativeIter {
            source: source.clone(),
            item_type,
            position: 0,
            string: None,
            keys,
            revision,
            guard,
            loops: Rc::new(Cell::new(0)),
            session: Rc::downgrade(&session),
            owner: owner.clone(),
            _retention: retention,
        })))?;
        session.iter_guards.borrow_mut().insert(id);
        Ok(Value::GcHandle(id))
    }
    pub(crate) fn new_string_iter(
        &self,
        source: &Value,
        ty: &AbiType,
        kind: StringIterKind,
        owner: &LoadedModule,
        retention: RetainedRuntimeProgram,
    ) -> Result<Value, RuntimeError> {
        if !kind.valid_source(ty) {
            return Err(invalid());
        }
        let Value::Tuple(fields) = source else {
            return Err(invalid());
        };
        let traversal = StringTraversal::new(kind, fields)?;
        let value = self.new_iter(
            &fields[0],
            &AbiType::Builtin(BuiltinType::String),
            owner,
            retention,
        )?;
        let Value::GcHandle(id) = value else {
            return Err(invalid());
        };
        let mut objects = self.objects.borrow_mut();
        let Some(HeapObject::Iter(iter)) = self.object_mut(&mut objects, id) else {
            return Err(invalid());
        };
        iter.string = Some(traversal);
        iter.item_type = kind.item_type();
        Ok(value)
    }

    pub(crate) fn advance_iter(
        &self,
        value: &Value,
        ty: &AbiType,
        op: IterOp,
    ) -> Result<Value, RuntimeError> {
        self.ensure_execution_allowed()?;
        let (Value::GcHandle(id), AbiType::Iter(item)) = (value, ty) else {
            return Err(invalid());
        };
        if op == IterOp::Close {
            self.validate_iter(value, ty)?;
            self.close_iter_tree(value)?;
            return Ok(Value::Unit);
        }
        if op != IterOp::Next {
            return Err(invalid());
        }
        let (needs_guard, payload, next_position, string_cursor, owner) = {
            let objects = self.objects.borrow();
            let Some(HeapObject::Iter(iter)) = self.readable_object(&objects, *id) else {
                return Err(invalid());
            };
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
                iter.owner.clone(),
            )
        };
        if payload
            .as_ref()
            .is_some_and(|value| !self.matches_abi(value, item, &owner))
        {
            return Err(invalid());
        }
        let session = self.resources.active_session().ok_or_else(invalid)?;
        let new_guard = if needs_guard && payload.is_some() {
            session
                .iter_guards
                .borrow_mut()
                .try_reserve(1)
                .map_err(|_| self.resource_limit("iterator registry"))?;
            let source = {
                let objects = self.objects.borrow();
                let Some(HeapObject::Iter(iter)) = self.readable_object(&objects, *id) else {
                    return Err(invalid());
                };
                iter.source.clone()
            };
            Some(self.begin_collection_iteration(&source)?)
        } else {
            None
        };
        // Allocate before committing the position so allocation failure does not skip an item.
        let tag = if payload.is_some() {
            EnumTag::OptionSome
        } else {
            EnumTag::OptionNone
        };
        let result = self.alloc_enum(tag, payload.clone().into_iter().collect())?;
        let mut objects = self.objects.borrow_mut();
        let Some(HeapObject::Iter(iter)) = self.object_mut(&mut objects, *id) else {
            return Err(invalid());
        };
        if let (Some(traversal), Some(cursor)) = (&mut iter.string, string_cursor) {
            traversal.cursor = cursor;
        }
        if payload.is_some() {
            iter.position = next_position;
            if needs_guard {
                session.iter_guards.borrow_mut().insert(*id);
                iter.guard = new_guard;
                iter.session = Rc::downgrade(&session);
            }
        } else {
            iter.guard = None;
        }
        Ok(Value::Enum(result))
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
            if let Some(HeapObject::Iter(iter)) = &mut slot.object
                && iter.session.ptr_eq(&Rc::downgrade(session))
            {
                iter.guard = None;
            }
        }
    }
}
