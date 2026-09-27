use super::*;
use kagari_ir::module::{
    abi::{AbiType, BuiltinType},
    instruction::IterOp,
};

#[derive(Debug)]
pub(super) struct NativeIter {
    pub(super) source: Value,
    pub(super) item_type: AbiType,
    position: u128,
    revision: u64,
    pub(super) guard: Option<CollectionIteration>,
    pub(super) loops: Rc<Cell<usize>>,
    session: Weak<crate::session::SessionState>,
    owner: crate::LoadedModule,
    _retention: crate::module::RetainedRuntimeProgram,
}

fn invalid() -> RuntimeError {
    RuntimeError::new(
        RuntimeErrorKind::ScriptTrap,
        "invalid or structurally modified iterator",
    )
}

impl GcHeap {
    pub(crate) fn new_script_iter(
        &self,
        source: &Value,
        ty: &AbiType,
        owner: &crate::LoadedModule,
        retention: crate::module::RetainedRuntimeProgram,
    ) -> Result<Value, RuntimeError> {
        self.ensure_execution_allowed()?;
        let item_type = IterOp::closure_item(ty).ok_or_else(invalid)?.clone();
        if !self.matches_abi(source, ty, owner) {
            return Err(invalid());
        }
        let session = self.resources.active_session().ok_or_else(invalid)?;
        self.alloc_object(HeapObject::Iter(Box::new(NativeIter {
            source: source.clone(),
            item_type,
            position: 0,
            revision: 0,
            guard: None,
            loops: Rc::new(Cell::new(0)),
            session: Rc::downgrade(&session),
            owner: owner.clone(),
            _retention: retention,
        })))
        .map(Value::GcHandle)
    }

    /// Script-backed steps execute on the VM frame stack, never under a heap borrow.
    pub fn iter_step(&self, value: &Value, ty: &AbiType) -> Result<Option<Value>, RuntimeError> {
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
        Ok(match &iter.source {
            Value::Tuple(fields) => fields.first().cloned(),
            _ => None,
        })
    }

    fn close_iter_tree(&self, value: &Value) -> Result<(), RuntimeError> {
        let mut pending = vec![value.clone()];
        let mut visited = std::collections::HashSet::new();
        while let Some(Value::GcHandle(id)) = pending.pop() {
            if !visited.insert(id) {
                continue;
            }
            let mut objects = self.objects.borrow_mut();
            let Some(HeapObject::Iter(iter)) = self.object_mut(&mut objects, id) else {
                return Err(invalid());
            };
            iter.guard = None;
            let source = iter.source.clone();
            drop(objects);
            if let Value::Tuple(fields) = source {
                for dependency in fields.into_iter().skip(1) {
                    match dependency {
                        Value::GcHandle(_) => pending.push(dependency),
                        Value::Array(slot) => {
                            // Dynamic inner iterators retain their own guards. Read the
                            // live slot on close rather than retaining an obsolete inner.
                            let Some(Value::Enum(value)) = self.array_get(slot, 0) else {
                                return Err(invalid());
                            };
                            let snapshot = self.enum_snapshot(value).ok_or_else(invalid)?;
                            match snapshot.tag {
                                crate::value::EnumTag::OptionSome => {
                                    pending.extend(snapshot.fields)
                                }
                                crate::value::EnumTag::OptionNone => {}
                                _ => return Err(invalid()),
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        Ok(())
    }
    fn collection_revision(&self, source: &Value) -> Option<u64> {
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
        owner: &crate::LoadedModule,
        retention: crate::module::RetainedRuntimeProgram,
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
        let id = self.alloc_object(HeapObject::Iter(Box::new(NativeIter {
            source: source.clone(),
            item_type,
            position: 0,
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
            self.iter_step(value, ty)?;
            self.close_iter_tree(value)?;
            return Ok(Value::Unit);
        }
        if op != IterOp::Next {
            return Err(invalid());
        }
        let (needs_guard, payload, next_position, owner) = {
            let objects = self.objects.borrow();
            let Some(HeapObject::Iter(iter)) = self.readable_object(&objects, *id) else {
                return Err(invalid());
            };
            if iter.item_type != **item
                || self.collection_revision(&iter.source) != Some(iter.revision)
            {
                return Err(invalid());
            }
            let (payload, advance) = match &iter.source {
                Value::Range(range) => (range.at(iter.position)?, 1),
                Value::Array(id) => (self.array_get(*id, iter.position as usize), 1),
                Value::Set(id) => (
                    self.with_set(*id, |values| {
                        values
                            .get_index(iter.position as usize)
                            .map(|(key, _)| key.to_value())
                    })
                    .ok_or_else(invalid)?,
                    1,
                ),
                Value::Map(id) => (
                    self.with_map(*id, |entries| {
                        entries
                            .get_index(iter.position as usize)
                            .map(|(key, value)| Value::Tuple(vec![key.to_value(), value.clone()]))
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
            };
            (
                iter.guard.is_none() && iter.loops.get() == 0,
                payload,
                iter.position.checked_add(advance).ok_or_else(invalid)?,
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
            crate::value::EnumTag::OptionSome
        } else {
            crate::value::EnumTag::OptionNone
        };
        let result = self.alloc_enum(tag, payload.clone().into_iter().collect())?;
        let mut objects = self.objects.borrow_mut();
        let Some(HeapObject::Iter(iter)) = self.object_mut(&mut objects, *id) else {
            return Err(invalid());
        };
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
    pub(crate) fn release_iter_guards(&self, session: &Rc<crate::session::SessionState>) {
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
