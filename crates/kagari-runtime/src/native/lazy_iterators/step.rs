//! One charged native operation per lazy step phase.
mod flatten;
mod simple;
mod windows;
use crate::{
    LoadedModule, Runtime, RuntimeError,
    gc::RootSet,
    native::NativeAction,
    value::{EnumTag, Value},
};
use flatten::FlattenStep;
use kagari_abi::{native_import::EngineNativeImport, standard::bindings::NativeDefaultMethod};
use simple::SimpleStep;
use windows::WindowStep;

pub(super) const SCRATCH: usize = 10;
pub(super) const NEXT: usize = 0;
pub(super) const ITEM: usize = 1;
pub(super) const MAPPED: usize = 2;
pub(super) const OUTPUT: usize = 3;
pub(super) const STATE: usize = 4;
pub(super) const OTHER: usize = 5;
pub(super) const TEMP: usize = 6;
pub(super) const STORAGE: usize = 7;
pub(super) const INDEX: usize = 8;
pub(super) const END: usize = 9;
#[derive(Clone, Copy)]
struct StepContext {
    scratch: usize,
}
enum Policy {
    Simple(SimpleStep),
    Flatten(FlattenStep),
    Windows(WindowStep),
}
pub(super) struct LazyStep {
    loads: usize,
    context: StepContext,
    policy: Policy,
}
pub(super) fn invalid() -> RuntimeError {
    RuntimeError::module_validation("native lazy step contract mismatch")
}
impl LazyStep {
    pub(super) fn start(operation: NativeDefaultMethod, captures: usize) -> Self {
        let policy = match operation {
            NativeDefaultMethod::FlatMap | NativeDefaultMethod::Flatten => {
                Policy::Flatten(FlattenStep::start(operation))
            }
            NativeDefaultMethod::ListWindows | NativeDefaultMethod::ListChunks => {
                Policy::Windows(WindowStep::start(operation))
            }
            _ => Policy::Simple(SimpleStep::start(operation)),
        };
        Self {
            loads: captures,
            context: StepContext {
                scratch: captures + 1,
            },
            policy,
        }
    }
    pub(super) fn advance(
        &mut self,
        runtime: &Runtime,
        owner: &LoadedModule,
        contract: &EngineNativeImport,
        roots: &RootSet,
    ) -> Result<NativeAction, RuntimeError> {
        if self.loads != 0 {
            self.loads -= 1;
            return Ok(NativeAction::Continue);
        }
        match &mut self.policy {
            Policy::Simple(step) => step.advance(runtime, owner, contract, roots, self.context),
            Policy::Flatten(step) => step.advance(runtime, owner, contract, roots, self.context),
            Policy::Windows(step) => step.advance(runtime, owner, contract, roots, self.context),
        }
    }
    pub(super) fn receive(
        &mut self,
        runtime: &Runtime,
        roots: &RootSet,
        value: Value,
    ) -> Result<NativeAction, RuntimeError> {
        match &mut self.policy {
            Policy::Simple(step) => step.receive(runtime, roots, self.context, value),
            Policy::Flatten(step) => step.receive(runtime, roots, self.context, value),
            Policy::Windows(step) => step.receive(runtime, roots, self.context, value),
        }
    }
}
impl StepContext {
    fn get(self, roots: &RootSet, slot: usize) -> Result<Value, RuntimeError> {
        roots.get(self.scratch + slot).ok_or_else(invalid)
    }
    fn set(
        self,
        runtime: &Runtime,
        roots: &RootSet,
        slot: usize,
        value: Value,
    ) -> Result<(), RuntimeError> {
        roots
            .set(runtime.gc(), self.scratch + slot, value)
            .ok_or_else(invalid)
    }
    fn capture(self, roots: &RootSet, slot: usize) -> Result<Value, RuntimeError> {
        roots
            .get(slot)
            .filter(|_| slot + 1 < self.scratch)
            .ok_or_else(invalid)
    }
    fn read_state(
        self,
        runtime: &Runtime,
        roots: &RootSet,
        capture: usize,
    ) -> Result<Value, RuntimeError> {
        let Value::Array(id) = self.capture(roots, capture)? else {
            return Err(invalid());
        };
        runtime.gc().array_get(id, 0).ok_or_else(invalid)
    }
    fn write_state(
        self,
        runtime: &Runtime,
        roots: &RootSet,
        capture: usize,
        value: Value,
    ) -> Result<(), RuntimeError> {
        let Value::Array(id) = self.capture(roots, capture)? else {
            return Err(invalid());
        };
        runtime.gc().array_set(id, 0, value)
    }
    fn present(self, runtime: &Runtime, value: &Value) -> Result<bool, RuntimeError> {
        let Value::Enum(id) = value else {
            return Err(invalid());
        };
        let snapshot = runtime.gc().enum_snapshot(*id).ok_or_else(invalid)?;
        match (snapshot.tag, snapshot.fields.len()) {
            (EnumTag::OptionSome, 1) => Ok(true),
            (EnumTag::OptionNone, 0) => Ok(false),
            _ => Err(invalid()),
        }
    }
    fn read(self, runtime: &Runtime, value: &Value) -> Result<Value, RuntimeError> {
        let Value::Enum(id) = value else {
            return Err(invalid());
        };
        let snapshot = runtime.gc().enum_snapshot(*id).ok_or_else(invalid)?;
        if snapshot.tag != EnumTag::OptionSome || snapshot.fields.len() != 1 {
            return Err(invalid());
        }
        snapshot.fields.into_iter().next().ok_or_else(invalid)
    }
    fn wrap(self, runtime: &Runtime, value: Option<Value>) -> Result<Value, RuntimeError> {
        let (tag, fields) = match value {
            Some(value) => (EnumTag::OptionSome, vec![value]),
            None => (EnumTag::OptionNone, vec![]),
        };
        runtime.alloc_enum(tag, fields).map(Value::Enum)
    }
}
