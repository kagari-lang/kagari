//! Scoped native iteration resources, including library-owned wrapper payloads.
use crate::{
    error::{RuntimeError, RuntimeErrorKind},
    gc::{CollectionIteration, GcHeap, GcObjectKind, iter::NativeIter, storage::HeapObject},
    value::Value,
};
use kagari_types::ty::Ty;
use std::collections::HashSet;

fn invalid() -> RuntimeError {
    RuntimeError::new(RuntimeErrorKind::ScriptTrap, "invalid iterator resource")
}

impl GcHeap {
    pub fn begin_collection_iteration(
        &self,
        value: &Value,
    ) -> Result<CollectionIteration, RuntimeError> {
        self.ensure_execution_allowed()?;
        if matches!(value, Value::GcHandle(_) | Value::Interface(_)) {
            let mut guard = CollectionIteration {
                _children: Vec::new(),
                loop_leases: Vec::new(),
                _lease: None,
                _root: self.root_value(value.clone()).ok_or_else(|| {
                    RuntimeError::new(RuntimeErrorKind::ScriptTrap, "invalid iterator")
                })?,
            };
            let mut pending = vec![value.clone()];
            let mut visited = HashSet::new();
            while let Some(value) = pending.pop() {
                self.ensure_execution_allowed()?;
                self.resources.poll_execution()?;
                match value {
                    Value::GcHandle(id) => {
                        if !visited.insert(id) {
                            continue;
                        }
                        let mut objects = self.objects_mut()?;
                        let (cursor, dependencies) = match self.object_mut(&mut objects, id) {
                            Some(HeapObject::Native(object))
                                if matches!(object.ty, Ty::Iter(_)) =>
                            {
                                let iter = object.payload_mut::<NativeIter>()?;
                                iter.guard = None;
                                (true, vec![iter.source.clone()])
                            }
                            Some(HeapObject::Native(object)) => {
                                let mut sources = Vec::new();
                                object
                                    .iteration_sources(&mut |source| sources.push(source.clone()));
                                (false, sources)
                            }
                            _ => return Err(invalid()),
                        };
                        if cursor {
                            guard
                                .loop_leases
                                .try_reserve(1)
                                .map_err(|_| self.resource_limit("iterator guards"))?;
                            guard.loop_leases.push(
                                self.iterator_loops
                                    .acquire(id, None)
                                    .map_err(|_| self.resource_limit("iterator guards"))?,
                            );
                        }
                        pending.extend(dependencies);
                    }
                    Value::Interface(id) => {
                        if !visited.insert(id.0) {
                            continue;
                        }
                        let snapshot = self.interface_snapshot(id).ok_or_else(invalid)?;
                        pending.push(snapshot.data.clone());
                    }
                    source => guard
                        ._children
                        .push(self.begin_collection_iteration(&source)?),
                }
            }
            return Ok(guard);
        }
        if matches!(
            value,
            Value::Str(_) | Value::Range(_) | Value::Struct(_) | Value::Enum(_) | Value::Tuple(_)
        ) {
            return Ok(CollectionIteration {
                _children: Vec::new(),
                loop_leases: Vec::new(),
                _lease: None,
                _root: self.root_value(value.clone()).ok_or_else(invalid)?,
            });
        }
        let id = match value {
            Value::Array(id) | Value::Map(id) | Value::Set(id) => *id,
            _ => {
                return Err(RuntimeError::new(
                    RuntimeErrorKind::ScriptTrap,
                    "expected collection",
                ));
            }
        };
        let expected = match value {
            Value::Array(_) => GcObjectKind::Array,
            Value::Map(_) => GcObjectKind::Map,
            _ => GcObjectKind::Set,
        };
        if self.object_kind(id) != Some(expected) {
            return Err(RuntimeError::new(
                RuntimeErrorKind::ScriptTrap,
                "invalid collection handle",
            ));
        }
        let root = self.root_value(value.clone()).ok_or_else(|| {
            RuntimeError::new(RuntimeErrorKind::ScriptTrap, "invalid collection handle")
        })?;
        let lease = self
            .iterations
            .acquire(id, None)
            .map_err(|_| self.resource_limit("iteration registry"))?;
        Ok(CollectionIteration {
            _children: Vec::new(),
            loop_leases: Vec::new(),
            _lease: Some(lease),
            _root: root,
        })
    }
}
