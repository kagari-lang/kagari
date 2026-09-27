use super::*;
use kagari_ir::module::{
    abi::{AbiType, BuiltinType},
    instruction::CursorOp,
};

#[derive(Debug)]
pub(super) struct NativeCursor {
    pub(super) source: Value,
    pub(super) item_type: AbiType,
    position: usize,
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
    pub(crate) fn new_script_cursor(
        &self,
        source: &Value,
        ty: &AbiType,
        owner: &crate::LoadedModule,
        retention: crate::module::RetainedRuntimeProgram,
    ) -> Result<Value, RuntimeError> {
        self.ensure_execution_allowed()?;
        let item_type = CursorOp::closure_item(ty).ok_or_else(invalid)?.clone();
        if !self.matches_abi(source, ty, owner) {
            return Err(invalid());
        }
        let session = self.resources.active_session().ok_or_else(invalid)?;
        self.alloc_object(HeapObject::Cursor(Box::new(NativeCursor {
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
    pub fn cursor_step(&self, value: &Value, ty: &AbiType) -> Result<Option<Value>, RuntimeError> {
        self.ensure_execution_allowed()?;
        let (Value::GcHandle(id), AbiType::Cursor(item)) = (value, ty) else {
            return Err(invalid());
        };
        let objects = self.objects.borrow();
        let Some(HeapObject::Cursor(cursor)) = self.readable_object(&objects, *id) else {
            return Err(invalid());
        };
        if cursor.item_type != **item {
            return Err(invalid());
        }
        Ok(match &cursor.source {
            Value::Tuple(fields) => fields.first().cloned(),
            _ => None,
        })
    }

    fn close_cursor_tree(&self, value: &Value) -> Result<(), RuntimeError> {
        let mut pending = vec![value.clone()];
        let mut visited = std::collections::HashSet::new();
        while let Some(Value::GcHandle(id)) = pending.pop() {
            if !visited.insert(id) {
                continue;
            }
            let mut objects = self.objects.borrow_mut();
            let Some(HeapObject::Cursor(cursor)) = self.object_mut(&mut objects, id) else {
                return Err(invalid());
            };
            cursor.guard = None;
            if let Value::Tuple(fields) = &cursor.source {
                pending.extend(
                    fields
                        .iter()
                        .skip(1)
                        .filter(|v| matches!(v, Value::GcHandle(_)))
                        .cloned(),
                );
            }
        }
        Ok(())
    }
    fn collection_revision(&self, source: &Value) -> Option<u64> {
        match source {
            Value::Str(_) => Some(0),
            Value::Array(id) | Value::Map(id) | Value::Set(id) => {
                let objects = self.objects.borrow();
                self.readable_object(&objects, *id)?;
                Some(objects[id.slot].revision)
            }
            _ => None,
        }
    }
    pub(crate) fn new_cursor(
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
            (Value::Str(_), AbiType::Builtin(BuiltinType::String)) => true,
            _ => false,
        };
        if !valid {
            return Err(invalid());
        }
        let item_type = match ty {
            AbiType::Array(item, _) | AbiType::Set(item, _) => (**item).clone(),
            AbiType::Map { key, value, .. } => {
                AbiType::Tuple(vec![(**key).clone(), (**value).clone()])
            }
            AbiType::Builtin(BuiltinType::String) => ty.clone(),
            _ => return Err(invalid()),
        };
        let revision = self.collection_revision(source).ok_or_else(invalid)?;
        let session = self.resources.active_session().ok_or_else(invalid)?;
        session
            .cursor_guards
            .borrow_mut()
            .try_reserve(1)
            .map_err(|_| self.resource_limit("iterator registry"))?;
        let guard = Some(self.begin_collection_iteration(source)?);
        let id = self.alloc_object(HeapObject::Cursor(Box::new(NativeCursor {
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
        session.cursor_guards.borrow_mut().insert(id);
        Ok(Value::GcHandle(id))
    }
    pub(crate) fn advance_cursor(
        &self,
        value: &Value,
        ty: &AbiType,
        op: CursorOp,
    ) -> Result<Value, RuntimeError> {
        self.ensure_execution_allowed()?;
        let (Value::GcHandle(id), AbiType::Cursor(item)) = (value, ty) else {
            return Err(invalid());
        };
        if op == CursorOp::Close {
            self.cursor_step(value, ty)?;
            self.close_cursor_tree(value)?;
            return Ok(Value::Unit);
        }
        if op != CursorOp::Next {
            return Err(invalid());
        }
        let (needs_guard, payload, next_position, owner) = {
            let objects = self.objects.borrow();
            let Some(HeapObject::Cursor(cursor)) = self.readable_object(&objects, *id) else {
                return Err(invalid());
            };
            if cursor.item_type != **item
                || self.collection_revision(&cursor.source) != Some(cursor.revision)
            {
                return Err(invalid());
            }
            let (payload, advance) = match &cursor.source {
                Value::Array(id) => (self.array_get(*id, cursor.position), 1),
                Value::Set(id) => (
                    self.with_set(*id, |values| {
                        values
                            .get_index(cursor.position)
                            .map(|(key, _)| key.to_value())
                    })
                    .ok_or_else(invalid)?,
                    1,
                ),
                Value::Map(id) => (
                    self.with_map(*id, |entries| {
                        entries
                            .get_index(cursor.position)
                            .map(|(key, value)| Value::Tuple(vec![key.to_value(), value.clone()]))
                    })
                    .ok_or_else(invalid)?,
                    1,
                ),
                Value::Str(text) => match text
                    .get(cursor.position..)
                    .and_then(|tail| tail.chars().next())
                {
                    Some(character) => (
                        Some(Value::Str(character.to_string())),
                        character.len_utf8(),
                    ),
                    None => (None, 0),
                },
                _ => return Err(invalid()),
            };
            (
                cursor.guard.is_none() && cursor.loops.get() == 0,
                payload,
                cursor.position.checked_add(advance).ok_or_else(invalid)?,
                cursor.owner.clone(),
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
                .cursor_guards
                .borrow_mut()
                .try_reserve(1)
                .map_err(|_| self.resource_limit("iterator registry"))?;
            let source = {
                let objects = self.objects.borrow();
                let Some(HeapObject::Cursor(cursor)) = self.readable_object(&objects, *id) else {
                    return Err(invalid());
                };
                cursor.source.clone()
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
        let Some(HeapObject::Cursor(cursor)) = self.object_mut(&mut objects, *id) else {
            return Err(invalid());
        };
        if payload.is_some() {
            cursor.position = next_position;
            if needs_guard {
                session.cursor_guards.borrow_mut().insert(*id);
                cursor.guard = new_guard;
                cursor.session = Rc::downgrade(&session);
            }
        } else {
            cursor.guard = None;
        }
        Ok(Value::Enum(result))
    }
    pub(crate) fn release_cursor_guards(&self, session: &Rc<crate::session::SessionState>) {
        let mut objects = self.objects.borrow_mut();
        for id in session.cursor_guards.borrow_mut().drain() {
            if id.owner != self.owner {
                continue;
            }
            let Some(slot) = objects.get_mut(id.slot) else {
                continue;
            };
            if slot.generation != id.generation {
                continue;
            }
            if let Some(HeapObject::Cursor(cursor)) = &mut slot.object
                && cursor.session.ptr_eq(&Rc::downgrade(session))
            {
                cursor.guard = None;
            }
        }
    }
}
