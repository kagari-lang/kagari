//! Selected bound evaluation and prepared interval storage operations.
use crate::{
    LoadedModule, Runtime, RuntimeError,
    gc::{CollectionIteration, RootSet},
    native::{
        NativeAction,
        protocols::{self, ProtocolStep},
        results,
    },
    value::Value,
};
use kagari_abi::{
    callable::EngineNativeBinding,
    native_import::{EngineNativeImport, NativeWitness},
    scalar::BuiltinType,
    standard::{StandardIntrinsic, surface::StandardEnum, traits::StandardTrait},
    types::AbiType,
};
const START: usize = 0;
const END: usize = 1;
const PREPARED: usize = 2;
const REMAINING: usize = 3;
const REMOVED: usize = 4;
const RESULT: usize = 5;
pub(super) const SCRATCH_ROOTS: usize = 6;
#[derive(Clone, Copy)]
enum Phase {
    WaitingStart,
    End,
    WaitingEnd,
    Copy,
    Begin,
    Prepare,
    RemainingIndex,
    Remaining,
    RemovedIndex,
    Removed,
    Result,
    Release,
    Commit,
}
pub(super) struct ArrayRange {
    scratch: usize,
    remove: bool,
    phase: Phase,
    guard: Option<CollectionIteration>,
}
fn invalid() -> RuntimeError {
    RuntimeError::module_validation("native array interval contract mismatch")
}
impl ArrayRange {
    pub(super) fn start(contract: &EngineNativeImport, arguments: &[Value]) -> Self {
        Self {
            scratch: arguments.len(),
            remove: contract.binding
                == EngineNativeBinding::Intrinsic(StandardIntrinsic::ArrayRemoveRange),
            phase: Phase::WaitingStart,
            guard: None,
        }
    }
    fn get(&self, roots: &RootSet, slot: usize) -> Result<Value, RuntimeError> {
        roots.get(self.scratch + slot).ok_or_else(invalid)
    }
    fn set(
        &self,
        runtime: &Runtime,
        roots: &RootSet,
        slot: usize,
        value: Value,
    ) -> Result<(), RuntimeError> {
        roots
            .set(runtime.gc(), self.scratch + slot, value)
            .ok_or_else(invalid)
    }
    fn witness<'a>(
        &self,
        contract: &'a EngineNativeImport,
    ) -> Result<&'a NativeWitness, RuntimeError> {
        contract
            .witnesses
            .iter()
            .find(|w| {
                StandardTrait::from_id(&w.interface.declaration) == Some(StandardTrait::RangeBounds)
            })
            .ok_or_else(invalid)
    }
    fn request(
        &mut self,
        runtime: &Runtime,
        owner: &LoadedModule,
        contract: &EngineNativeImport,
        roots: &RootSet,
        upper: bool,
    ) -> Result<NativeAction, RuntimeError> {
        let step = protocols::range_bound(
            runtime,
            owner,
            self.witness(contract)?,
            roots.get(1).ok_or_else(invalid)?,
            upper,
        )?;
        match step {
            ProtocolStep::Call(request) => Ok(NativeAction::Callback(request)),
            ProtocolStep::Value(value) => self.receive(runtime, owner, contract, roots, value),
            ProtocolStep::BuiltinFailure(error) => Ok(NativeAction::BuiltinFailure(error)),
        }
    }
    pub(super) fn initialize(
        &mut self,
        runtime: &Runtime,
        owner: &LoadedModule,
        contract: &EngineNativeImport,
        roots: &RootSet,
    ) -> Result<NativeAction, RuntimeError> {
        self.request(runtime, owner, contract, roots, false)
    }
    pub(super) fn receive(
        &mut self,
        runtime: &Runtime,
        owner: &LoadedModule,
        _contract: &EngineNativeImport,
        roots: &RootSet,
        value: Value,
    ) -> Result<NativeAction, RuntimeError> {
        let ty = AbiType::StandardEnum {
            kind: StandardEnum::Bound,
            args: vec![AbiType::Builtin(BuiltinType::USize)],
        };
        if !runtime.matches_interface_method_abi(&value, &ty, owner) {
            return Err(invalid());
        }
        let (slot, next) = match self.phase {
            Phase::WaitingStart => (START, Phase::End),
            Phase::WaitingEnd => (
                END,
                if self.remove {
                    Phase::Begin
                } else {
                    Phase::Copy
                },
            ),
            _ => return Err(invalid()),
        };
        self.set(runtime, roots, slot, value)?;
        self.phase = next;
        Ok(NativeAction::Continue)
    }
    fn field(
        &self,
        runtime: &Runtime,
        contract: &EngineNativeImport,
        owner: &LoadedModule,
        roots: &RootSet,
        index: usize,
        slot: usize,
    ) -> Result<(), RuntimeError> {
        let Value::Tuple(fields) = self.get(roots, PREPARED)? else {
            return Err(invalid());
        };
        let value = fields.get(index).cloned().ok_or_else(invalid)?;
        if !runtime.matches_interface_method_abi(&value, &contract.signature.params[0], owner) {
            return Err(invalid());
        }
        self.set(runtime, roots, slot, value)
    }
    pub(super) fn advance(
        &mut self,
        runtime: &Runtime,
        owner: &LoadedModule,
        contract: &EngineNativeImport,
        roots: &RootSet,
    ) -> Result<NativeAction, RuntimeError> {
        match self.phase {
            Phase::End => {
                self.phase = Phase::WaitingEnd;
                return self.request(runtime, owner, contract, roots, true);
            }
            Phase::Copy => {
                return Ok(
                    match runtime.invoke_standard_builtin(
                        StandardIntrinsic::ArrayCopyWithinBounds,
                        &[
                            roots.get(0).ok_or_else(invalid)?,
                            self.get(roots, START)?,
                            self.get(roots, END)?,
                            roots.get(2).ok_or_else(invalid)?,
                        ],
                    ) {
                        Ok(value) => NativeAction::Complete(value),
                        Err(error) => NativeAction::BuiltinFailure(error),
                    },
                );
            }
            Phase::Begin => {
                self.guard = Some(
                    runtime
                        .gc()
                        .begin_collection_mutation(&roots.get(0).ok_or_else(invalid)?)?,
                );
                self.phase = Phase::Prepare;
            }
            Phase::Prepare => {
                let value = match runtime.invoke_standard_builtin(
                    StandardIntrinsic::ArrayRemoveRangePrepare,
                    &[
                        roots.get(0).ok_or_else(invalid)?,
                        self.get(roots, START)?,
                        self.get(roots, END)?,
                    ],
                ) {
                    Ok(value) => value,
                    Err(error) => return Ok(NativeAction::BuiltinFailure(error)),
                };
                self.set(runtime, roots, PREPARED, value)?;
                self.phase = Phase::RemainingIndex;
            }
            Phase::RemainingIndex => self.phase = Phase::Remaining,
            Phase::Remaining => {
                self.field(runtime, contract, owner, roots, 0, REMAINING)?;
                self.phase = Phase::RemovedIndex;
            }
            Phase::RemovedIndex => self.phase = Phase::Removed,
            Phase::Removed => {
                self.field(runtime, contract, owner, roots, 1, REMOVED)?;
                self.phase = Phase::Result;
            }
            Phase::Result => {
                let value =
                    results::readonly_list(runtime, owner, contract, self.get(roots, REMOVED)?)?;
                self.set(runtime, roots, RESULT, value)?;
                self.phase = Phase::Release;
            }
            Phase::Release => {
                self.guard.take();
                self.phase = Phase::Commit;
            }
            Phase::Commit => {
                return Ok(
                    match runtime.invoke_standard_builtin(
                        StandardIntrinsic::ArrayReplaceStorage,
                        &[
                            roots.get(0).ok_or_else(invalid)?,
                            self.get(roots, REMAINING)?,
                        ],
                    ) {
                        Ok(_) => NativeAction::Complete(self.get(roots, RESULT)?),
                        Err(error) => NativeAction::BuiltinFailure(error),
                    },
                );
            }
            Phase::WaitingStart | Phase::WaitingEnd => return Err(invalid()),
        }
        Ok(NativeAction::Continue)
    }
}
