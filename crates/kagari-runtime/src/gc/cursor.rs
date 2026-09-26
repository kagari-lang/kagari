use super::*;
use kagari_ir::module::{
    abi::{AbiType, BuiltinType},
    instruction::CursorOp,
};

#[derive(Debug)]
pub(super) struct NativeCursor {
    pub(super) source: Value,
    pub(super) items: Vec<Value>,
    pub(super) item_type: AbiType,
    position: usize,
    revision: u64,
    pub(super) guard: Option<CollectionIteration>,
    pub(super) loops: Rc<Cell<usize>>,
    session: Weak<crate::session::SessionState>,
    _retention: crate::module::RetainedRuntimeProgram,
}

fn invalid() -> RuntimeError {
    RuntimeError::new(
        RuntimeErrorKind::ScriptTrap,
        "invalid or structurally modified iterator",
    )
}

impl GcHeap {
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
        if !self.matches_abi(source, ty, owner) {
            return Err(invalid());
        }
        let item_type = match ty {
            AbiType::Array(item) | AbiType::Set(item) => (**item).clone(),
            AbiType::Map { key, value } => AbiType::Tuple(vec![(**key).clone(), (**value).clone()]),
            AbiType::Builtin(BuiltinType::String) => ty.clone(),
            _ => return Err(invalid()),
        };
        let items = match source {
            Value::Array(id) => self.array_snapshot(*id).ok_or_else(invalid)?,
            Value::Set(id) => self.set_snapshot(*id).ok_or_else(invalid)?,
            Value::Map(id) => self
                .map_snapshot(*id)
                .ok_or_else(invalid)?
                .into_iter()
                .map(|(k, v)| Value::Tuple(vec![k, v]))
                .collect(),
            Value::Str(text) => text.chars().map(|c| Value::Str(c.to_string())).collect(),
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
            items,
            item_type,
            position: 0,
            revision,
            guard,
            loops: Rc::new(Cell::new(0)),
            session: Rc::downgrade(&session),
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
        let (source, revision, needs_guard, payload) = {
            let objects = self.objects.borrow();
            let Some(HeapObject::Cursor(cursor)) = self.readable_object(&objects, *id) else {
                return Err(invalid());
            };
            if cursor.item_type != **item {
                return Err(invalid());
            }
            (
                cursor.source.clone(),
                cursor.revision,
                cursor.guard.is_none() && cursor.loops.get() == 0,
                cursor.items.get(cursor.position).cloned(),
            )
        };
        if op == CursorOp::Close {
            let mut objects = self.objects.borrow_mut();
            let Some(HeapObject::Cursor(cursor)) = self.object_mut(&mut objects, *id) else {
                return Err(invalid());
            };
            cursor.guard = None;
            return Ok(Value::Unit);
        }
        if op != CursorOp::Next || self.collection_revision(&source) != Some(revision) {
            return Err(invalid());
        }
        let session = self.resources.active_session().ok_or_else(invalid)?;
        let new_guard = if needs_guard && payload.is_some() {
            session
                .cursor_guards
                .borrow_mut()
                .try_reserve(1)
                .map_err(|_| self.resource_limit("iterator registry"))?;
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
            cursor.position += 1;
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
