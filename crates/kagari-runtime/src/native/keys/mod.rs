//! Selected key lookups and atomic mutations on existing Map/Set storage.
mod lookup;
mod updates;
use crate::{
    LoadedModule, Runtime, RuntimeError,
    gc::RootSet,
    native::{
        NativeAction,
        keys::{
            lookup::{KeyStep, Lookup},
            updates::MapUpdate,
        },
    },
    value::Value,
};
use kagari_abi::{
    native_import::{EngineNativeImport, NativeWitness, NativeWitnessImplementation},
    scalar::BuiltinType,
    standard::{StandardIntrinsic, traits::StandardTrait},
    types::AbiType,
};
const CANDIDATES: usize = 0;
const CANDIDATE: usize = 1;
const RESULT: usize = 2;
const KEY: usize = 3;
pub(super) const SCRATCH_ROOTS: usize = 4;
fn invalid() -> RuntimeError {
    RuntimeError::module_validation("native key contract mismatch")
}
#[derive(Clone, Copy)]
struct Buffers {
    scratch: usize,
}
impl Buffers {
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
    fn receiver(self, roots: &RootSet) -> Result<Value, RuntimeError> {
        roots.get(0).ok_or_else(invalid)
    }
    fn query(self, roots: &RootSet) -> Result<Value, RuntimeError> {
        roots.get(1).ok_or_else(invalid)
    }
}
fn witness(
    contract: &EngineNativeImport,
    protocol: StandardTrait,
) -> Result<&NativeWitness, RuntimeError> {
    let key = match &contract.signature.params[0] {
        AbiType::Map { key, .. } | AbiType::Set(key, _) => key,
        _ => return Err(invalid()),
    };
    contract
        .witnesses
        .iter()
        .find(|w| {
            w.receiver == **key
                && StandardTrait::from_id(&w.interface.declaration) == Some(protocol)
        })
        .ok_or_else(invalid)
}
pub(super) struct KeyInvocation {
    lookup: Lookup,
    update: Option<MapUpdate>,
    discard: bool,
    unit: bool,
}
impl KeyInvocation {
    pub(super) fn start(
        operation: StandardIntrinsic,
        contract: &EngineNativeImport,
        arguments: &[Value],
    ) -> Result<Self, RuntimeError> {
        let equality = witness(contract, StandardTrait::PartialEq)?;
        let custom = matches!(
            equality.implementation,
            NativeWitnessImplementation::Derived
        ) || matches!(
            equality.implementation,
            NativeWitnessImplementation::Table(_)
        );
        let buffers = Buffers {
            scratch: arguments.len(),
        };
        let update = matches!(
            operation,
            StandardIntrinsic::MapGetOrInsertWith | StandardIntrinsic::MapUpdate
        )
        .then(|| MapUpdate::start(operation, custom, buffers));
        let lookup = Lookup::start(operation, custom, buffers, Some(2));
        Ok(Self {
            lookup,
            update,
            discard: contract.signature.result == AbiType::Builtin(BuiltinType::Unit),
            unit: false,
        })
    }
    fn finish(&mut self, step: KeyStep) -> NativeAction {
        match step {
            KeyStep::Action(action) => action,
            KeyStep::Ready(value) => {
                if self.discard {
                    self.unit = true;
                    NativeAction::Continue
                } else {
                    NativeAction::Complete(value)
                }
            }
        }
    }
    pub(super) fn initialize(
        &mut self,
        runtime: &Runtime,
        roots: &RootSet,
    ) -> Result<NativeAction, RuntimeError> {
        if let Some(update) = &mut self.update {
            return update.initialize(runtime, roots);
        }
        let step = self.lookup.begin(runtime, roots)?;
        Ok(self.finish(step))
    }
    pub(super) fn advance(
        &mut self,
        runtime: &Runtime,
        owner: &LoadedModule,
        contract: &EngineNativeImport,
        roots: &RootSet,
    ) -> Result<NativeAction, RuntimeError> {
        if let Some(update) = &mut self.update {
            return update.advance(runtime, owner, contract, roots);
        }
        if self.unit {
            return Ok(NativeAction::Complete(Value::Unit));
        }
        let step = self.lookup.advance(runtime, owner, contract, roots)?;
        Ok(self.finish(step))
    }
    pub(super) fn receive(
        &mut self,
        runtime: &Runtime,
        owner: &LoadedModule,
        contract: &EngineNativeImport,
        roots: &RootSet,
        value: Value,
    ) -> Result<NativeAction, RuntimeError> {
        if let Some(update) = &mut self.update {
            return update.receive(runtime, owner, contract, roots, value);
        }
        self.lookup.receive(runtime, owner, value)
    }
}
