//! Managed captures are GC edges; active accesses own roots and scoped guards.
use crate::{
    error::{RuntimeError, RuntimeErrorKind},
    gc::{GcHeap, GcObjectKind, HeapObject, HeapObjectId, RootSet},
    module::{LoadedModule, RetainedRuntimeProgram},
    native_value::iterator::{
        NativeStateDependency, data::NativeStateData, invocation::ManagedStateFactory,
    },
    value::Value,
};
use kagari_abi::native_import::{NativeSignature, callables::NativeCallableApplication};
use kagari_abi::types::AbiType;
use std::{any::Any, cell::Cell, fmt, mem, rc::Rc};

#[derive(Debug, Clone)]
pub(crate) struct ManagedIterContract {
    pub(crate) owner: LoadedModule,
    pub(crate) signature: NativeSignature,
    pub(crate) item: AbiType,
    pub(crate) callables: Vec<NativeCallableApplication>,
}
pub(crate) struct ManagedIter {
    pub(crate) contract: ManagedIterContract,
    pub(crate) captures: Vec<Value>,
    factory: Rc<dyn ManagedStateFactory>,
    payload: Box<dyn Any>,
    payload_units: usize,
    dependencies: Vec<(usize, Option<u64>)>,
    active: Rc<Cell<Option<u64>>>,
    sequence: Rc<Cell<u64>>,
    pub(crate) loops: Rc<Cell<usize>>,
    _retention: RetainedRuntimeProgram,
}
impl fmt::Debug for ManagedIter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ManagedIter")
            .field("contract", &self.contract)
            .field("captures", &self.captures)
            .field("active", &self.active)
            .finish_non_exhaustive()
    }
}
impl ManagedIter {
    pub(crate) fn units(&self) -> usize {
        self.captures.len() + self.dependencies.len() + self.payload_units
    }
    pub(crate) fn dependencies(&self) -> impl Iterator<Item = &Value> {
        self.dependencies
            .iter()
            .map(|(slot, _)| &self.captures[*slot])
    }
}
pub(crate) struct ManagedIterLease {
    pub(crate) heap: Rc<GcHeap>,
    pub(crate) id: HeapObjectId,
    _root: RootSet,
    active: Rc<Cell<Option<u64>>>,
    epoch: u64,
}
impl ManagedIterLease {
    pub(crate) fn validate(&self) -> Result<(), RuntimeError> {
        if self.active.get() == Some(self.epoch) {
            Ok(())
        } else {
            Err(invalid())
        }
    }
    pub(crate) fn finish(&self) {
        if self.active.get() == Some(self.epoch) {
            self.active.set(None);
        }
    }
}
impl Drop for ManagedIterLease {
    fn drop(&mut self) {
        self.finish();
    }
}
pub(crate) struct ManagedIterRequest {
    pub(crate) contract: ManagedIterContract,
    pub(crate) captures: Vec<Value>,
    pub(crate) dependencies: Vec<Value>,
    pub(crate) factory: Rc<dyn ManagedStateFactory>,
    pub(crate) lease: Rc<ManagedIterLease>,
}
fn invalid() -> RuntimeError {
    RuntimeError::module_validation("managed iterator state contract mismatch")
}
impl GcHeap {
    pub(crate) fn alloc_managed_iter<P: NativeStateData>(
        &self,
        contract: ManagedIterContract,
        captures: Vec<Value>,
        dependencies: &[NativeStateDependency],
        payload: P,
        factory: Rc<dyn ManagedStateFactory>,
        retention: RetainedRuntimeProgram,
    ) -> Result<HeapObjectId, RuntimeError> {
        self.ensure_execution_allowed()?;
        if contract.signature.params.len() != captures.len()
            || captures
                .iter()
                .zip(&contract.signature.params)
                .any(|(value, ty)| !self.matches_abi(value, ty, &contract.owner))
        {
            return Err(invalid());
        }
        let mut revisions = Vec::new();
        for dependency in dependencies {
            let (slot, required) = match dependency {
                NativeStateDependency::Collection(slot) => (*slot, true),
                NativeStateDependency::OptionalCollection(slot) => (*slot, false),
            };
            let value = captures.get(slot).ok_or_else(invalid)?;
            let valid = match value {
                Value::Array(_)
                | Value::Map(_)
                | Value::Set(_)
                | Value::Str(_)
                | Value::Range(_) => true,
                Value::GcHandle(id) => self.object_kind(*id) == Some(GcObjectKind::Iter),
                _ => false,
            };
            if !valid && !required {
                continue;
            }
            if !valid || revisions.iter().any(|(existing, _)| *existing == slot) {
                return Err(invalid());
            }
            revisions.push((slot, self.collection_revision(value)));
        }
        let payload_units = mem::size_of::<P>().div_ceil(mem::size_of::<usize>());
        self.alloc_object(HeapObject::ManagedIter(Box::new(ManagedIter {
            contract,
            captures,
            factory,
            payload: Box::new(payload),
            payload_units,
            dependencies: revisions,
            active: Rc::new(Cell::new(None)),
            sequence: Rc::new(Cell::new(0)),
            loops: Rc::new(Cell::new(0)),
            _retention: retention,
        })))
    }
    pub(crate) fn managed_iter_request(
        self: &Rc<Self>,
        value: &Value,
        item: &AbiType,
    ) -> Result<Option<ManagedIterRequest>, RuntimeError> {
        self.ensure_execution_allowed()?;
        let Value::GcHandle(id) = value else {
            return Err(invalid());
        };
        let (contract, captures, dependencies, factory, active, sequence) = {
            let objects = self.objects.borrow();
            match self.readable_object(&objects, *id).ok_or_else(invalid)? {
                HeapObject::Iter(iter) if iter.item_type == *item => return Ok(None),
                HeapObject::ManagedIter(state) if state.contract.item == *item => {
                    if state.active.get().is_some() {
                        return Err(RuntimeError::new(
                            RuntimeErrorKind::ScriptTrap,
                            "iterator step is already active",
                        ));
                    }
                    for (slot, revision) in &state.dependencies {
                        if self.collection_revision(&state.captures[*slot]) != *revision {
                            return Err(RuntimeError::new(
                                RuntimeErrorKind::ScriptTrap,
                                "structurally modified iterator source",
                            ));
                        }
                    }
                    (
                        state.contract.clone(),
                        state.captures.clone(),
                        state.dependencies().cloned().collect(),
                        state.factory.clone(),
                        state.active.clone(),
                        state.sequence.clone(),
                    )
                }
                _ => return Err(invalid()),
            }
        };
        let root = self
            .root_execution_values(vec![value.clone()])
            .ok_or_else(invalid)?;
        let epoch = sequence.get().checked_add(1).ok_or_else(invalid)?;
        sequence.set(epoch);
        active.set(Some(epoch));
        Ok(Some(ManagedIterRequest {
            contract,
            captures,
            dependencies,
            factory,
            lease: Rc::new(ManagedIterLease {
                heap: self.clone(),
                id: *id,
                _root: root,
                active,
                epoch,
            }),
        }))
    }
    pub(crate) fn managed_iter_data<P: NativeStateData>(
        &self,
        id: HeapObjectId,
    ) -> Result<P, RuntimeError> {
        self.ensure_execution_allowed()?;
        let objects = self.objects.borrow();
        let Some(HeapObject::ManagedIter(state)) = self.readable_object(&objects, id) else {
            return Err(invalid());
        };
        if state.active.get().is_none() {
            return Err(invalid());
        }
        state
            .payload
            .downcast_ref::<P>()
            .copied()
            .ok_or_else(invalid)
    }
    pub(crate) fn set_managed_iter_data<P: NativeStateData>(
        &self,
        id: HeapObjectId,
        value: P,
    ) -> Result<(), RuntimeError> {
        self.ensure_execution_allowed()?;
        let mut objects = self.objects.borrow_mut();
        let Some(HeapObject::ManagedIter(state)) = self.object_mut(&mut objects, id) else {
            return Err(invalid());
        };
        if state.active.get().is_none() {
            return Err(invalid());
        }
        *state.payload.downcast_mut::<P>().ok_or_else(invalid)? = value;
        Ok(())
    }
    pub(crate) fn set_managed_iter_capture(
        &self,
        id: HeapObjectId,
        slot: usize,
        value: Value,
    ) -> Result<(), RuntimeError> {
        self.ensure_execution_allowed()?;
        let (owner, expected, dependency) = {
            let objects = self.objects.borrow();
            let Some(HeapObject::ManagedIter(state)) = self.readable_object(&objects, id) else {
                return Err(invalid());
            };
            if state.active.get().is_none() {
                return Err(invalid());
            }
            (
                state.contract.owner.clone(),
                state
                    .contract
                    .signature
                    .params
                    .get(slot)
                    .cloned()
                    .ok_or_else(invalid)?,
                state.dependencies.iter().any(|(index, _)| *index == slot),
            )
        };
        if dependency || !self.matches_abi(&value, &expected, &owner) {
            return Err(invalid());
        }
        let mut objects = self.objects.borrow_mut();
        let Some(HeapObject::ManagedIter(state)) = self.object_mut(&mut objects, id) else {
            return Err(invalid());
        };
        state.captures[slot] = value;
        Ok(())
    }
}
