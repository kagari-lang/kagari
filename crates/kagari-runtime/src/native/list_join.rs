//! Public List joining selects storage or checked conversion, then shares native traversal.
use crate::{
    LoadedModule, Runtime, RuntimeError,
    gc::RootSet,
    native::{
        NativeAction,
        iterators::IteratorInvocation,
        protocols::{self, ProtocolStep},
    },
    value::Value,
};
use kagari_abi::{
    native_import::EngineNativeImport,
    standard::{StandardIntrinsic, bindings::NativeDefaultMethod, traits::StandardTrait},
    types::AbiType,
};

enum Phase {
    Entry,
    WaitingIterator,
    InitializeJoin,
    Joining(IteratorInvocation),
}

pub(super) struct ListJoin {
    phase: Phase,
    scratch: usize,
    iterator: usize,
}

fn invalid() -> RuntimeError {
    RuntimeError::module_validation("native List join contract mismatch")
}

impl ListJoin {
    pub(super) fn start(
        contract: &EngineNativeImport,
        arguments: &[Value],
    ) -> Result<Self, RuntimeError> {
        let iterator = contract
            .witnesses
            .iter()
            .position(|witness| {
                StandardTrait::from_id(&witness.interface.declaration)
                    == Some(StandardTrait::Iterator)
            })
            .ok_or_else(invalid)?;
        Ok(Self {
            phase: Phase::Entry,
            scratch: arguments.len(),
            iterator,
        })
    }

    pub(super) fn initialize(
        &mut self,
        runtime: &Runtime,
        owner: &LoadedModule,
        contract: &EngineNativeImport,
        roots: &RootSet,
    ) -> Result<NativeAction, RuntimeError> {
        if matches!(contract.signature.params[0], AbiType::Array(_, _)) {
            return match runtime.invoke_standard_builtin(
                StandardIntrinsic::ArrayJoin,
                &[
                    roots.get(0).ok_or_else(invalid)?,
                    roots.get(1).ok_or_else(invalid)?,
                ],
            ) {
                Ok(value) => Ok(NativeAction::Complete(value)),
                Err(error) => Ok(NativeAction::BuiltinFailure(error)),
            };
        }
        let iterable = contract
            .witnesses
            .iter()
            .find(|witness| {
                witness.receiver == contract.signature.params[0]
                    && StandardTrait::from_id(&witness.interface.declaration)
                        == Some(StandardTrait::Iterable)
            })
            .ok_or_else(invalid)?;
        match protocols::iter(
            runtime,
            owner,
            iterable,
            roots.get(0).ok_or_else(invalid)?,
            &contract.witnesses[self.iterator].receiver,
        )? {
            ProtocolStep::Value(value) => self.receive(runtime, owner, contract, roots, value),
            ProtocolStep::Call(request) => {
                self.phase = Phase::WaitingIterator;
                Ok(NativeAction::Callback(request))
            }
            ProtocolStep::BuiltinFailure(error) => Ok(NativeAction::BuiltinFailure(error)),
        }
    }

    pub(super) fn advance(
        &mut self,
        runtime: &Runtime,
        owner: &LoadedModule,
        contract: &EngineNativeImport,
        roots: &RootSet,
    ) -> Result<NativeAction, RuntimeError> {
        match &mut self.phase {
            Phase::InitializeJoin => {
                // The next original operation creates the rooted accumulation list.
                let mut join = IteratorInvocation::start(
                    runtime,
                    NativeDefaultMethod::Join,
                    contract,
                    &[
                        roots.get(self.scratch).ok_or_else(invalid)?,
                        roots.get(1).ok_or_else(invalid)?,
                    ],
                )?;
                for (index, value) in join
                    .initial
                    .take()
                    .ok_or_else(invalid)?
                    .into_iter()
                    .enumerate()
                {
                    roots
                        .set(runtime.gc(), self.scratch + index, value)
                        .ok_or_else(invalid)?;
                }
                self.phase = Phase::Joining(join);
                Ok(NativeAction::Continue)
            }
            Phase::Joining(join) => join.advance(runtime, owner, contract, roots),
            _ => Err(invalid()),
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
        match &mut self.phase {
            Phase::Entry | Phase::WaitingIterator => {
                if !runtime.matches_interface_method_abi(
                    &value,
                    &contract.witnesses[self.iterator].receiver,
                    owner,
                ) {
                    return Err(invalid());
                }
                roots
                    .set(runtime.gc(), self.scratch, value)
                    .ok_or_else(invalid)?;
                self.phase = Phase::InitializeJoin;
            }
            Phase::Joining(join) => join.receive(runtime, owner, contract, roots, value)?,
            Phase::InitializeJoin => return Err(invalid()),
        }
        Ok(NativeAction::Continue)
    }
}
