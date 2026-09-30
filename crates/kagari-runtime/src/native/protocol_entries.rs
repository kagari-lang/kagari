//! Public parsing and assertion entries delegate to selected, generation-pinned protocols.
use crate::{
    LoadedModule, Runtime, RuntimeError,
    gc::RootSet,
    native::{
        NativeAction,
        protocols::{self, ProtocolStep},
    },
    value::Value,
};
use kagari_abi::{
    native_import::EngineNativeImport,
    standard::{StandardIntrinsic, traits::StandardTrait},
};

enum Phase {
    Entry,
    Waiting,
    Assert,
}
pub(super) struct ProtocolEntry {
    operation: StandardIntrinsic,
    phase: Phase,
    witness: usize,
    scratch: usize,
}
pub(super) const SCRATCH_ROOTS: usize = 1;
fn invalid() -> RuntimeError {
    RuntimeError::module_validation("native protocol entry contract mismatch")
}
impl ProtocolEntry {
    pub(super) fn start(
        operation: StandardIntrinsic,
        contract: &EngineNativeImport,
        arguments: &[Value],
    ) -> Result<Self, RuntimeError> {
        let protocol = match operation {
            StandardIntrinsic::StringParse => StandardTrait::FromStr,
            StandardIntrinsic::DebugAssertEq => StandardTrait::PartialEq,
            _ => return Err(invalid()),
        };
        let witness = contract
            .witnesses
            .iter()
            .position(|witness| {
                StandardTrait::from_id(&witness.interface.declaration) == Some(protocol)
            })
            .ok_or_else(invalid)?;
        Ok(Self {
            operation,
            phase: Phase::Entry,
            witness,
            scratch: arguments.len(),
        })
    }
    pub(super) fn initialize(
        &mut self,
        runtime: &Runtime,
        owner: &LoadedModule,
        contract: &EngineNativeImport,
        roots: &RootSet,
    ) -> Result<NativeAction, RuntimeError> {
        let selected = &contract.witnesses[self.witness];
        let step = match self.operation {
            StandardIntrinsic::StringParse => protocols::parse(
                runtime,
                owner,
                selected,
                roots.get(0).ok_or_else(invalid)?,
                &contract.signature.result,
            )?,
            StandardIntrinsic::DebugAssertEq => protocols::equal(
                runtime,
                owner,
                selected,
                roots.get(0).ok_or_else(invalid)?,
                roots.get(1).ok_or_else(invalid)?,
            )?,
            _ => return Err(invalid()),
        };
        match step {
            ProtocolStep::Value(value) => self.receive(runtime, owner, contract, roots, value),
            ProtocolStep::Call(request) => {
                self.phase = Phase::Waiting;
                Ok(NativeAction::Callback(request))
            }
            ProtocolStep::BuiltinFailure(error) => Ok(NativeAction::BuiltinFailure(error)),
        }
    }
    pub(super) fn advance(
        &self,
        runtime: &Runtime,
        roots: &RootSet,
    ) -> Result<NativeAction, RuntimeError> {
        if !matches!(self.phase, Phase::Assert) {
            return Err(invalid());
        }
        match runtime.invoke_standard_builtin(
            StandardIntrinsic::DebugAssert,
            &[
                roots.get(self.scratch).ok_or_else(invalid)?,
                roots.get(2).ok_or_else(invalid)?,
            ],
        ) {
            Ok(value) => Ok(NativeAction::Complete(value)),
            Err(error) => Ok(NativeAction::BuiltinFailure(error)),
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
        if !matches!(self.phase, Phase::Entry | Phase::Waiting) {
            return Err(invalid());
        }
        if self.operation == StandardIntrinsic::StringParse {
            if !runtime.matches_interface_method_abi(&value, &contract.signature.result, owner) {
                return Err(invalid());
            }
            return Ok(NativeAction::Complete(value));
        }
        if !matches!(value, Value::Bool(_)) {
            return Err(invalid());
        }
        roots
            .set(runtime.gc(), self.scratch, value)
            .ok_or_else(invalid)?;
        self.phase = Phase::Assert;
        Ok(NativeAction::Continue)
    }
}
