//! Flat traversal owns one live inner cursor and commits it only after conversion.
use super::{ITEM, NEXT, OUTPUT, STATE, StepContext, TEMP, invalid};
use crate::{
    LoadedModule, Runtime, RuntimeError,
    gc::RootSet,
    native::{
        NativeAction, callback,
        protocols::{self, ProtocolStep},
        sources::{IteratorSelection, SourceSelection},
    },
    value::Value,
};
use kagari_abi::{
    native_import::EngineNativeImport, operations::IterOp, standard::bindings::NativeDefaultMethod,
    types::AbiType,
};
#[derive(Clone, Copy)]
enum Phase {
    EndedIndex,
    EndedRead,
    EndedBranch,
    StateIndex,
    StateRead,
    StateTest,
    StateBranch,
    InnerRead,
    InnerNext,
    WaitingInner,
    InnerTest,
    InnerBranch,
    InnerClose,
    Empty,
    ClearIndex,
    Clear,
    OuterJump,
    OuterNext,
    WaitingOuter,
    OuterTest,
    OuterBranch,
    OuterRead,
    Callback,
    WaitingCallback,
    Convert,
    WaitingConvert,
    StoreSome,
    StoreIndex,
    Store,
    HeadJump,
    EndedTrue,
    EndedWriteIndex,
    EndedWrite,
    OuterClose,
    None,
    Return,
}
pub(super) struct FlattenStep {
    operation: NativeDefaultMethod,
    phase: Phase,
    present: bool,
}
impl FlattenStep {
    pub(super) fn start(operation: NativeDefaultMethod) -> Self {
        Self {
            operation,
            phase: Phase::EndedIndex,
            present: false,
        }
    }
    fn source<'a>(&self, contract: &'a EngineNativeImport) -> Result<&'a AbiType, RuntimeError> {
        if self.operation == NativeDefaultMethod::FlatMap {
            let AbiType::Function { result, .. } = &contract.signature.params[1] else {
                return Err(invalid());
            };
            Ok(result)
        } else {
            IteratorSelection::select(contract, &contract.signature.params[0])?.item(contract)
        }
    }
    fn inner(&self, contract: &EngineNativeImport) -> Result<SourceSelection, RuntimeError> {
        SourceSelection::select(contract, self.source(contract)?, None, 0)
    }
    pub(super) fn advance(
        &mut self,
        runtime: &Runtime,
        owner: &LoadedModule,
        contract: &EngineNativeImport,
        roots: &RootSet,
        cx: StepContext,
    ) -> Result<NativeAction, RuntimeError> {
        match self.phase {
            Phase::EndedIndex => self.phase = Phase::EndedRead,
            Phase::EndedRead => {
                let Value::Bool(ended) = cx.read_state(runtime, roots, 2)? else {
                    return Err(invalid());
                };
                self.present = ended;
                self.phase = Phase::EndedBranch;
            }
            Phase::EndedBranch => {
                self.phase = if self.present {
                    Phase::EndedTrue
                } else {
                    Phase::StateIndex
                }
            }
            Phase::StateIndex => self.phase = Phase::StateRead,
            Phase::StateRead => {
                cx.set(runtime, roots, STATE, cx.read_state(runtime, roots, 1)?)?;
                self.phase = Phase::StateTest;
            }
            Phase::StateTest => {
                self.present = cx.present(runtime, &cx.get(roots, STATE)?)?;
                self.phase = Phase::StateBranch;
            }
            Phase::StateBranch => {
                self.phase = if self.present {
                    Phase::InnerRead
                } else {
                    Phase::OuterNext
                }
            }
            Phase::InnerRead => {
                cx.set(
                    runtime,
                    roots,
                    TEMP,
                    cx.read(runtime, &cx.get(roots, STATE)?)?,
                )?;
                self.phase = Phase::InnerNext;
            }
            Phase::InnerNext => {
                let selection = self.inner(contract)?;
                self.phase = Phase::WaitingInner;
                return match protocols::next(
                    runtime,
                    owner,
                    selection.next(contract),
                    cx.get(roots, TEMP)?,
                    &selection.optional(contract)?,
                )? {
                    ProtocolStep::Value(value) => self.receive(runtime, roots, cx, value),
                    ProtocolStep::Call(request) => Ok(NativeAction::Callback(request)),
                    ProtocolStep::BuiltinFailure(error) => Ok(NativeAction::BuiltinFailure(error)),
                };
            }
            Phase::InnerTest => {
                self.present = cx.present(runtime, &cx.get(roots, NEXT)?)?;
                self.phase = Phase::InnerBranch;
            }
            Phase::InnerBranch => {
                self.phase = if self.present {
                    Phase::Return
                } else if matches!(
                    self.inner(contract)?.next(contract).receiver,
                    AbiType::Iter(_)
                ) {
                    Phase::InnerClose
                } else {
                    Phase::Empty
                }
            }
            Phase::InnerClose => {
                runtime.iter_operation(
                    owner,
                    &cx.get(roots, TEMP)?,
                    &self.inner(contract)?.next(contract).receiver,
                    IterOp::Close,
                )?;
                self.phase = Phase::Empty;
            }
            Phase::Empty => {
                cx.set(runtime, roots, STATE, cx.wrap(runtime, None)?)?;
                self.phase = Phase::ClearIndex;
            }
            Phase::ClearIndex => self.phase = Phase::Clear,
            Phase::Clear => {
                cx.write_state(runtime, roots, 1, cx.get(roots, STATE)?)?;
                self.phase = Phase::OuterJump;
            }
            Phase::OuterJump => self.phase = Phase::OuterNext,
            Phase::OuterNext => {
                let selection = IteratorSelection::select(contract, &contract.signature.params[0])?;
                self.phase = Phase::WaitingOuter;
                return match protocols::next(
                    runtime,
                    owner,
                    selection.witness(contract),
                    cx.capture(roots, 0)?,
                    &selection.optional(contract)?,
                )? {
                    ProtocolStep::Value(value) => self.receive(runtime, roots, cx, value),
                    ProtocolStep::Call(request) => Ok(NativeAction::Callback(request)),
                    ProtocolStep::BuiltinFailure(error) => Ok(NativeAction::BuiltinFailure(error)),
                };
            }
            Phase::OuterTest => {
                self.present = cx.present(runtime, &cx.get(roots, NEXT)?)?;
                self.phase = Phase::OuterBranch;
            }
            Phase::OuterBranch => {
                self.phase = if self.present {
                    Phase::OuterRead
                } else {
                    Phase::EndedTrue
                }
            }
            Phase::OuterRead => {
                cx.set(
                    runtime,
                    roots,
                    ITEM,
                    cx.read(runtime, &cx.get(roots, NEXT)?)?,
                )?;
                self.phase = if self.operation == NativeDefaultMethod::FlatMap {
                    Phase::Callback
                } else {
                    Phase::Convert
                };
            }
            Phase::Callback => {
                self.phase = Phase::WaitingCallback;
                return callback(
                    runtime,
                    &cx.capture(roots, 3)?,
                    &contract.signature.params[1],
                    vec![cx.get(roots, ITEM)?],
                )
                .map(NativeAction::Callback);
            }
            Phase::Convert => {
                let selection = self.inner(contract)?;
                self.phase = Phase::WaitingConvert;
                return match protocols::iter(
                    runtime,
                    owner,
                    selection.iterable(contract),
                    cx.get(roots, ITEM)?,
                    &selection.next(contract).receiver,
                )? {
                    ProtocolStep::Value(value) => self.receive(runtime, roots, cx, value),
                    ProtocolStep::Call(request) => Ok(NativeAction::Callback(request)),
                    ProtocolStep::BuiltinFailure(error) => Ok(NativeAction::BuiltinFailure(error)),
                };
            }
            Phase::StoreSome => {
                cx.set(
                    runtime,
                    roots,
                    STATE,
                    cx.wrap(runtime, Some(cx.get(roots, TEMP)?))?,
                )?;
                self.phase = Phase::StoreIndex;
            }
            Phase::StoreIndex => self.phase = Phase::Store,
            Phase::Store => {
                cx.write_state(runtime, roots, 1, cx.get(roots, STATE)?)?;
                self.phase = Phase::HeadJump;
            }
            Phase::HeadJump => self.phase = Phase::StateIndex,
            Phase::EndedTrue => self.phase = Phase::EndedWriteIndex,
            Phase::EndedWriteIndex => self.phase = Phase::EndedWrite,
            Phase::EndedWrite => {
                cx.write_state(runtime, roots, 2, Value::Bool(true))?;
                self.phase = if matches!(contract.signature.params[0], AbiType::Iter(_)) {
                    Phase::OuterClose
                } else {
                    Phase::None
                };
            }
            Phase::OuterClose => {
                runtime.iter_operation(
                    owner,
                    &cx.capture(roots, 0)?,
                    &contract.signature.params[0],
                    IterOp::Close,
                )?;
                self.phase = Phase::None;
            }
            Phase::None => {
                cx.set(runtime, roots, OUTPUT, cx.wrap(runtime, None)?)?;
                self.present = false;
                self.phase = Phase::Return;
            }
            Phase::Return => {
                return Ok(NativeAction::Complete(
                    cx.get(roots, if self.present { NEXT } else { OUTPUT })?,
                ));
            }
            Phase::WaitingInner
            | Phase::WaitingOuter
            | Phase::WaitingCallback
            | Phase::WaitingConvert => return Err(invalid()),
        }
        Ok(NativeAction::Continue)
    }
    pub(super) fn receive(
        &mut self,
        runtime: &Runtime,
        roots: &RootSet,
        cx: StepContext,
        value: Value,
    ) -> Result<NativeAction, RuntimeError> {
        let (slot, phase) = match self.phase {
            Phase::WaitingInner => (NEXT, Phase::InnerTest),
            Phase::WaitingOuter => (NEXT, Phase::OuterTest),
            Phase::WaitingCallback => (ITEM, Phase::Convert),
            Phase::WaitingConvert => (TEMP, Phase::StoreSome),
            _ => return Err(invalid()),
        };
        cx.set(runtime, roots, slot, value)?;
        self.phase = phase;
        Ok(NativeAction::Continue)
    }
}
