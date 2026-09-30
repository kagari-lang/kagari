//! Selected key lookups and atomic mutations on existing Map/Set storage.
mod construction;
mod lookup;
mod updates;
use crate::{
    LoadedModule, Runtime, RuntimeError,
    gc::RootSet,
    native::{
        NativeAction,
        keys::{
            construction::Construction,
            lookup::{KeyStep, Lookup},
            updates::MapUpdate,
        },
    },
    value::Value,
};
use kagari_abi::{
    callable::EngineNativeBinding,
    native_import::{EngineNativeImport, NativeWitness, NativeWitnessImplementation},
    scalar::BuiltinType,
    standard::{StandardIntrinsic, bindings::NativeProtocolMethod, traits::StandardTrait},
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
    receiver: usize,
    query: usize,
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
        roots.get(self.receiver).ok_or_else(invalid)
    }
    fn query(self, roots: &RootSet) -> Result<Value, RuntimeError> {
        roots.get(self.query).ok_or_else(invalid)
    }
}
fn witness(
    contract: &EngineNativeImport,
    protocol: StandardTrait,
) -> Result<&NativeWitness, RuntimeError> {
    let storage = if matches!(
        contract.binding,
        EngineNativeBinding::Intrinsic(
            StandardIntrinsic::LinkedHashMapFrom | StandardIntrinsic::LinkedHashSetFrom
        ) | EngineNativeBinding::Protocol(NativeProtocolMethod::CollectionFromIterator)
    ) {
        &contract.signature.result
    } else {
        &contract.signature.params[0]
    };
    let key = match storage {
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
enum State {
    Lookup {
        lookup: Lookup,
        discard: bool,
        unit: bool,
    },
    Update(MapUpdate),
    Construction(Construction),
}
pub(super) struct KeyInvocation {
    state: State,
}
impl KeyInvocation {
    pub(super) fn construct(
        contract: &EngineNativeImport,
        arguments: &[Value],
    ) -> Result<Self, RuntimeError> {
        Construction::start(contract, arguments).map(|state| Self {
            state: State::Construction(state),
        })
    }
    pub(super) fn scratch_roots(&self) -> usize {
        if matches!(self.state, State::Construction(_)) {
            construction::SCRATCH_ROOTS
        } else {
            SCRATCH_ROOTS
        }
    }
    pub(super) fn start(
        operation: StandardIntrinsic,
        contract: &EngineNativeImport,
        arguments: &[Value],
    ) -> Result<Self, RuntimeError> {
        let equality = witness(contract, StandardTrait::PartialEq)?;
        let custom = matches!(
            equality.implementation,
            NativeWitnessImplementation::Derived | NativeWitnessImplementation::Table(_)
        );
        let buffers = Buffers {
            scratch: arguments.len(),
            receiver: 0,
            query: 1,
        };
        if matches!(
            operation,
            StandardIntrinsic::MapGetOrInsertWith | StandardIntrinsic::MapUpdate
        ) {
            return Ok(Self {
                state: State::Update(MapUpdate::start(operation, custom, buffers)),
            });
        }
        Ok(Self {
            state: State::Lookup {
                lookup: Lookup::start(operation, custom, buffers, Some(2)),
                discard: contract.signature.result == AbiType::Builtin(BuiltinType::Unit),
                unit: false,
            },
        })
    }
    fn finish(step: KeyStep, discard: bool, unit: &mut bool) -> NativeAction {
        match step {
            KeyStep::Action(action) => action,
            KeyStep::Ready(value) => {
                if discard {
                    *unit = true;
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
        owner: &LoadedModule,
        contract: &EngineNativeImport,
        roots: &RootSet,
    ) -> Result<NativeAction, RuntimeError> {
        match &mut self.state {
            State::Construction(state) => state.initialize(runtime, owner, contract, roots),
            State::Update(state) => state.initialize(runtime, roots),
            State::Lookup {
                lookup,
                discard,
                unit,
            } => Ok(Self::finish(lookup.begin(runtime, roots)?, *discard, unit)),
        }
    }
    pub(super) fn advance(
        &mut self,
        runtime: &Runtime,
        owner: &LoadedModule,
        contract: &EngineNativeImport,
        roots: &RootSet,
    ) -> Result<NativeAction, RuntimeError> {
        match &mut self.state {
            State::Construction(state) => state.advance(runtime, owner, contract, roots),
            State::Update(state) => state.advance(runtime, owner, contract, roots),
            State::Lookup {
                lookup,
                discard,
                unit,
            } => {
                if *unit {
                    Ok(NativeAction::Complete(Value::Unit))
                } else {
                    Ok(Self::finish(
                        lookup.advance(runtime, owner, contract, roots)?,
                        *discard,
                        unit,
                    ))
                }
            }
        }
    }
    pub(super) fn receive(
        &mut self,
        runtime: &Runtime,
        owner: &LoadedModule,
        contract: &EngineNativeImport,
        roots: &RootSet,
        value: Value,
    ) -> Result<NativeAction, RuntimeError> {
        match &mut self.state {
            State::Construction(state) => state.receive(runtime, owner, contract, roots, value),
            State::Update(state) => state.receive(runtime, owner, contract, roots, value),
            State::Lookup { lookup, .. } => lookup.receive(runtime, owner, value),
        }
    }
}
