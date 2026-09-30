//! Delegate one checked constructor on the current frame and shared root set.
use super::{Fallible, invalid};
use crate::{
    LoadedModule, Runtime, RuntimeError,
    gc::RootSet,
    native::{
        NativeAction,
        array_copy::{self, ArrayCopy},
        keys::KeyInvocation,
        protocols,
        sources::SourceSelection,
    },
    value::Value,
};
use kagari_abi::{
    native_import::EngineNativeImport,
    standard::{bindings::NativeProtocolMethod, traits::StandardTrait},
    types::AbiType,
};
use std::slice;

pub(super) enum Factory {
    Script {
        witness: usize,
        source: AbiType,
        root: usize,
    },
    Array(ArrayCopy),
    Keys(Box<KeyInvocation>),
    Fallible(Box<Fallible>),
}
impl Factory {
    // Each lifted child is a strict subtree of its parent's checked output type;
    // portable type limits bound both recursion and the registered scratch roots.
    pub(super) fn select(
        owner: &LoadedModule,
        contract: &EngineNativeImport,
        destination: &AbiType,
        source: &AbiType,
        root: usize,
        scratch: usize,
    ) -> Result<Self, RuntimeError> {
        let AbiType::Array(item, _) = source else {
            return Err(invalid());
        };
        let index = contract
            .witnesses
            .iter()
            .position(|witness| {
                witness.receiver == *destination
                    && StandardTrait::from_id(&witness.interface.declaration)
                        == Some(StandardTrait::FromIterator)
                    && witness.interface.arguments.as_slice() == slice::from_ref(item.as_ref())
            })
            .ok_or_else(invalid)?;
        match protocols::provider(owner, &contract.witnesses[index]) {
            None => Ok(Self::Script {
                witness: index,
                source: source.clone(),
                root,
            }),
            Some(provider) => {
                let iterator = AbiType::Iter(item.clone());
                let source = SourceSelection::select(contract, source, Some(&iterator), root)?;
                match provider {
                    NativeProtocolMethod::CollectionFromIterator => match destination {
                        AbiType::Array(..) => Ok(Self::Array(ArrayCopy::for_source(
                            contract, source, scratch, None,
                        )?)),
                        AbiType::Map { .. } | AbiType::Set(..) => Ok(Self::Keys(Box::new(
                            KeyInvocation::for_source(contract, source, destination, scratch)?,
                        ))),
                        _ => Err(invalid()),
                    },
                    NativeProtocolMethod::OptionFromIterator
                    | NativeProtocolMethod::ResultFromIterator => Ok(Self::Fallible(Box::new(
                        Fallible::for_source(owner, contract, source, destination, scratch)?,
                    ))),
                    _ => Err(invalid()),
                }
            }
        }
    }
    pub(super) fn scratch_roots(&self) -> usize {
        match self {
            Self::Script { .. } => 0,
            Self::Array(_) => array_copy::SCRATCH_ROOTS,
            Self::Keys(state) => state.scratch_roots(),
            Self::Fallible(state) => state.scratch_roots(),
        }
    }
    pub(super) fn initialize(
        &mut self,
        runtime: &Runtime,
        owner: &LoadedModule,
        contract: &EngineNativeImport,
        roots: &RootSet,
    ) -> Result<NativeAction, RuntimeError> {
        match self {
            Self::Script {
                witness,
                source,
                root,
            } => Ok(NativeAction::Callback(protocols::aggregate(
                runtime,
                owner,
                &contract.witnesses[*witness],
                roots.get(*root).ok_or_else(invalid)?,
                source,
            )?)),
            Self::Array(state) => state.initialize(runtime, owner, contract, roots),
            Self::Keys(state) => state.initialize(runtime, owner, contract, roots),
            Self::Fallible(state) => state.initialize(runtime, owner, contract, roots),
        }
    }
    pub(super) fn advance(
        &mut self,
        runtime: &Runtime,
        owner: &LoadedModule,
        contract: &EngineNativeImport,
        roots: &RootSet,
    ) -> Result<NativeAction, RuntimeError> {
        match self {
            Self::Script { .. } => Err(invalid()),
            Self::Array(state) => state.advance(runtime, owner, contract, roots),
            Self::Keys(state) => state.advance(runtime, owner, contract, roots),
            Self::Fallible(state) => state.advance(runtime, owner, contract, roots),
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
        match self {
            Self::Script { witness, .. } => {
                if !runtime.matches_interface_method_abi(
                    &value,
                    &contract.witnesses[*witness].receiver,
                    owner,
                ) {
                    return Err(invalid());
                }
                Ok(NativeAction::Complete(value))
            }
            Self::Array(state) => state.receive(runtime, owner, contract, roots, value),
            Self::Keys(state) => state.receive(runtime, owner, contract, roots, value),
            Self::Fallible(state) => state.receive(runtime, owner, contract, roots, value),
        }
    }
}
