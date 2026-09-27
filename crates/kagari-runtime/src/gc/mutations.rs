//! Guards prevent callbacks from modifying prepared mutation targets through aliases.
use super::*;

impl GcHeap {
    pub(crate) fn begin_collection_mutation(
        &self,
        value: &Value,
    ) -> Result<CollectionIteration, RuntimeError> {
        self.ensure_execution_allowed()?;
        let id = match value {
            Value::Array(id) if self.object_kind(*id) == Some(GcObjectKind::Array) => *id,
            Value::Map(id) if self.object_kind(*id) == Some(GcObjectKind::Map) => *id,
            Value::Set(id) if self.object_kind(*id) == Some(GcObjectKind::Set) => *id,
            _ => {
                return Err(RuntimeError::new(
                    RuntimeErrorKind::ScriptTrap,
                    "invalid mutation target",
                ));
            }
        };
        self.ensure_callback_mutable(id)?;
        let root = self.root_value(value.clone()).ok_or_else(|| {
            RuntimeError::new(RuntimeErrorKind::ScriptTrap, "invalid mutation root")
        })?;
        let mut active = self.mutations.borrow_mut();
        active
            .try_reserve(1)
            .map_err(|_| self.resource_limit("mutation registry"))?;
        active.insert(id, 1);
        Ok(CollectionIteration {
            _children: Vec::new(),
            iter_loops: Vec::new(),
            active: self.mutations.clone(),
            id: Some(id),
            _root: root,
        })
    }

    pub(crate) fn ensure_callback_mutable(&self, id: HeapObjectId) -> Result<(), RuntimeError> {
        if self.mutations.borrow().contains_key(&id) {
            Err(RuntimeError::new(
                RuntimeErrorKind::ScriptTrap,
                "container mutation during a guarded callback",
            ))
        } else {
            Ok(())
        }
    }
}
