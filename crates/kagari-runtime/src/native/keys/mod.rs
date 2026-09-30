//! Selected key lookups and atomic mutations on existing Map/Set storage.
mod construction;
mod grouping;
mod lookup;
mod sets;
mod updates;
use crate::{
    LoadedModule, Runtime, RuntimeError,
    gc::RootSet,
    native::{
        NativeAction,
        keys::{
            construction::Construction,
            grouping::Grouping,
            lookup::{KeyStep, Lookup},
            sets::SetQuery,
            updates::MapUpdate,
        },
        sources::SourceSelection,
    },
    value::Value,
};
use kagari_abi::{
    callable::EngineNativeBinding,
    native_import::{EngineNativeImport, NativeWitness, NativeWitnessImplementation},
    scalar::BuiltinType,
    standard::{
        StandardIntrinsic,
        bindings::{NativeDefaultMethod, NativeProtocolMethod},
        traits::StandardTrait,
    },
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
    ) || contract.binding
        == EngineNativeBinding::TraitDefault(NativeDefaultMethod::GroupBy)
        || matches!(contract.signature.result, AbiType::Set(..))
            && matches!(contract.binding, EngineNativeBinding::TraitDefault(_))
    {
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
/// Key methods are selected for the actual storage, including nested destinations.
#[derive(Clone, Copy)]
struct KeySelection {
    equality: usize,
    hash: usize,
    custom: bool,
}
impl KeySelection {
    fn declared(contract: &EngineNativeImport) -> Result<Self, RuntimeError> {
        let equality = witness(contract, StandardTrait::PartialEq)?;
        Self::for_key(contract, &equality.receiver)
    }
    fn for_storage(contract: &EngineNativeImport, storage: &AbiType) -> Result<Self, RuntimeError> {
        let key = match storage {
            AbiType::Map { key, .. } | AbiType::Set(key, _) => key,
            _ => return Err(invalid()),
        };
        Self::for_key(contract, key)
    }
    fn for_key(contract: &EngineNativeImport, key: &AbiType) -> Result<Self, RuntimeError> {
        let select = |protocol| {
            contract
                .witnesses
                .iter()
                .position(|witness| {
                    witness.receiver == *key
                        && StandardTrait::from_id(&witness.interface.declaration) == Some(protocol)
                })
                .ok_or_else(invalid)
        };
        let equality = select(StandardTrait::PartialEq)?;
        Ok(Self {
            equality,
            hash: select(StandardTrait::Hash)?,
            custom: matches!(
                contract.witnesses[equality].implementation,
                NativeWitnessImplementation::Derived | NativeWitnessImplementation::Table(_)
            ),
        })
    }
}
enum State {
    Lookup {
        lookup: Lookup,
        discard: bool,
        unit: bool,
    },
    Update(MapUpdate),
    Construction(Construction),
    Sets(SetQuery),
    Grouping(Grouping),
}
pub(super) struct KeyInvocation {
    state: State,
}
impl KeyInvocation {
    pub(super) fn group(
        contract: &EngineNativeImport,
        arguments: &[Value],
    ) -> Result<Self, RuntimeError> {
        Ok(Self {
            state: State::Grouping(Grouping::start(contract, arguments)?),
        })
    }
    pub(super) fn sets(operation: NativeDefaultMethod, arguments: &[Value]) -> Self {
        Self {
            state: State::Sets(SetQuery::start(operation, arguments)),
        }
    }

    pub(super) fn for_source(
        contract: &EngineNativeImport,
        source: SourceSelection,
        storage: &AbiType,
        scratch: usize,
    ) -> Result<Self, RuntimeError> {
        Construction::for_source(contract, source, storage, scratch).map(|state| Self {
            state: State::Construction(state),
        })
    }
    pub(super) fn construct(
        contract: &EngineNativeImport,
        arguments: &[Value],
    ) -> Result<Self, RuntimeError> {
        Construction::start(contract, arguments).map(|state| Self {
            state: State::Construction(state),
        })
    }
    pub(super) fn scratch_roots(&self) -> usize {
        match self.state {
            State::Construction(_) => construction::SCRATCH_ROOTS,
            State::Sets(_) => sets::SCRATCH_ROOTS,
            State::Grouping(_) => grouping::SCRATCH_ROOTS,
            _ => SCRATCH_ROOTS,
        }
    }
    pub(super) fn start(
        operation: StandardIntrinsic,
        contract: &EngineNativeImport,
        arguments: &[Value],
    ) -> Result<Self, RuntimeError> {
        let keys = KeySelection::declared(contract)?;
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
                state: State::Update(MapUpdate::start(operation, keys, buffers)),
            });
        }
        Ok(Self {
            state: State::Lookup {
                lookup: Lookup::start(operation, keys, buffers, Some(2)),
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
            State::Grouping(state) => state.initialize(runtime, roots),
            State::Sets(state) => state.initialize(runtime, owner, contract, roots),
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
            State::Grouping(state) => state.advance(runtime, owner, contract, roots),
            State::Sets(state) => state.advance(runtime, owner, contract, roots),
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
            State::Grouping(state) => state.receive(runtime, owner, contract, roots, value),
            State::Sets(state) => state.receive(runtime, owner, contract, roots, value),
            State::Construction(state) => state.receive(runtime, owner, contract, roots, value),
            State::Update(state) => state.receive(runtime, owner, contract, roots, value),
            State::Lookup { lookup, .. } => lookup.receive(runtime, owner, value),
        }
    }
}
