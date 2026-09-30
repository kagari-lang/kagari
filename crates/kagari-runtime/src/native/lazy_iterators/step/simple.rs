//! Scalar, predicate and paired adapters preserve their cursor commit order.
use super::{ITEM, MAPPED, NEXT, OTHER, OUTPUT, STATE, StepContext, TEMP, invalid};
use crate::{
    LoadedModule, Runtime, RuntimeError,
    gc::RootSet,
    native::{
        NativeAction, callback,
        protocols::{self, ProtocolStep},
        sources::{IteratorSelection, SourceSelection},
    },
    numeric,
    value::Value,
};
use kagari_abi::{
    native_import::EngineNativeImport,
    operations::IterOp,
    standard::{bindings::NativeDefaultMethod, surface::StandardEnum},
    types::AbiType,
};
use kagari_bytecode::BinaryOp;

#[derive(Clone, Copy)]
enum Phase {
    Entry,
    Head,
    StateRead,
    CheckZero,
    Check,
    CheckBranch,
    One,
    Change,
    WriteIndex,
    Write,
    Next,
    WaitingNext,
    Test,
    Branch,
    Read,
    Callback,
    WaitingCallback,
    MappedTest,
    MappedBranch,
    SkipPassingIndex,
    SkipPassingRead,
    SkipPassingBranch,
    PassTrue,
    PassIndex,
    PassWrite,
    PassJump,
    Pair,
    Some,
    Return,
    SwitchOne,
    SwitchIndex,
    SwitchWrite,
    SwitchClose,
    SwitchJump,
    ChainOne,
    ChainCompare,
    ChainBranch,
    RightNext,
    WaitingRight,
    RightTest,
    RightBranch,
    RightRead,
    Done,
    DoneIndex,
    DoneWrite,
    CloseLeft,
    CloseRight,
    None,
}
pub(super) struct SimpleStep {
    operation: NativeDefaultMethod,
    phase: Phase,
    present: bool,
    counter: u64,
    branch: bool,
    counter_after: Phase,
}
impl SimpleStep {
    pub(super) fn start(operation: NativeDefaultMethod) -> Self {
        Self {
            operation,
            phase: Phase::Entry,
            present: false,
            counter: 0,
            branch: false,
            counter_after: Phase::Some,
        }
    }
    fn state_slot(&self) -> usize {
        if matches!(
            self.operation,
            NativeDefaultMethod::TakeWhile
                | NativeDefaultMethod::SkipWhile
                | NativeDefaultMethod::Chain
        ) {
            2
        } else {
            1
        }
    }
    fn after_state(&self) -> Phase {
        match self.operation {
            NativeDefaultMethod::Fuse | NativeDefaultMethod::TakeWhile => Phase::CheckBranch,
            NativeDefaultMethod::Enumerate => Phase::One,
            _ => Phase::CheckZero,
        }
    }
    fn head(&self) -> Phase {
        if matches!(
            self.operation,
            NativeDefaultMethod::Fuse
                | NativeDefaultMethod::TakeWhile
                | NativeDefaultMethod::Take
                | NativeDefaultMethod::Chain
        ) {
            Phase::Head
        } else {
            Phase::Next
        }
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
            Phase::Entry => self.phase = self.head(),
            Phase::Head => self.phase = Phase::StateRead,
            Phase::StateRead => {
                let value = cx.read_state(runtime, roots, self.state_slot())?;
                self.branch = match &value {
                    Value::Bool(value) => *value,
                    Value::U64(value) => *value == 0,
                    Value::I32(value) => *value == 0,
                    _ => return Err(invalid()),
                };
                self.counter = match value {
                    Value::U64(value) => value,
                    Value::I32(value) => value as u64,
                    _ => 0,
                };
                cx.set(runtime, roots, STATE, value)?;
                self.phase = self.after_state();
            }
            Phase::CheckZero => self.phase = Phase::Check,
            Phase::Check => self.phase = Phase::CheckBranch,
            Phase::CheckBranch => {
                self.phase = match self.operation {
                    NativeDefaultMethod::Chain => {
                        if self.branch {
                            Phase::Next
                        } else {
                            Phase::ChainOne
                        }
                    }
                    NativeDefaultMethod::Fuse | NativeDefaultMethod::TakeWhile => {
                        if self.branch {
                            self.done_phase(contract)?
                        } else {
                            Phase::Next
                        }
                    }
                    NativeDefaultMethod::Skip => {
                        if self.branch {
                            Phase::Some
                        } else {
                            self.counter_after = Phase::Entry;
                            Phase::One
                        }
                    }
                    _ => {
                        if self.branch {
                            self.done_phase(contract)?
                        } else {
                            self.counter_after = Phase::Next;
                            Phase::One
                        }
                    }
                }
            }
            Phase::One => self.phase = Phase::Change,
            Phase::Change => {
                let value = numeric::binary(
                    if self.operation == NativeDefaultMethod::Enumerate {
                        BinaryOp::Add
                    } else {
                        BinaryOp::Sub
                    },
                    Value::U64(self.counter),
                    Value::U64(1),
                )?;
                cx.set(runtime, roots, TEMP, value)?;
                self.phase = Phase::WriteIndex;
            }
            Phase::WriteIndex => self.phase = Phase::Write,
            Phase::Write => {
                cx.write_state(runtime, roots, self.state_slot(), cx.get(roots, TEMP)?)?;
                self.phase = if self.operation == NativeDefaultMethod::Enumerate {
                    Phase::Pair
                } else {
                    self.counter_after
                };
            }
            Phase::Next => {
                let selection = IteratorSelection::select(contract, &contract.signature.params[0])?;
                self.phase = Phase::WaitingNext;
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
            Phase::Test => {
                self.present = cx.present(runtime, &cx.get(roots, NEXT)?)?;
                self.phase = Phase::Branch;
            }
            Phase::Branch => {
                self.phase = if self.present {
                    if self.operation == NativeDefaultMethod::Chain {
                        Phase::Return
                    } else {
                        Phase::Read
                    }
                } else if self.operation == NativeDefaultMethod::Chain {
                    Phase::SwitchOne
                } else {
                    self.done_phase(contract)?
                }
            }
            Phase::Read => {
                cx.set(
                    runtime,
                    roots,
                    ITEM,
                    cx.read(runtime, &cx.get(roots, NEXT)?)?,
                )?;
                self.phase = self.after_read();
            }
            Phase::Callback => {
                self.phase = Phase::WaitingCallback;
                return callback(
                    runtime,
                    &cx.capture(roots, 1)?,
                    &contract.signature.params[1],
                    vec![cx.get(roots, ITEM)?],
                )
                .map(NativeAction::Callback);
            }
            Phase::MappedTest => {
                self.present = cx.present(runtime, &cx.get(roots, MAPPED)?)?;
                self.phase = Phase::MappedBranch;
            }
            Phase::MappedBranch => {
                if self.operation == NativeDefaultMethod::FilterMap {
                    self.phase = if self.present {
                        Phase::Return
                    } else {
                        self.head()
                    };
                } else {
                    let Value::Bool(keep) = cx.get(roots, MAPPED)? else {
                        return Err(invalid());
                    };
                    self.phase = match self.operation {
                        NativeDefaultMethod::TakeWhile => {
                            if keep {
                                Phase::Some
                            } else {
                                Phase::Done
                            }
                        }
                        NativeDefaultMethod::SkipWhile => {
                            if keep {
                                self.head()
                            } else {
                                Phase::PassTrue
                            }
                        }
                        _ => {
                            if keep {
                                Phase::Some
                            } else {
                                self.head()
                            }
                        }
                    };
                }
            }
            Phase::SkipPassingIndex => self.phase = Phase::SkipPassingRead,
            Phase::SkipPassingRead => {
                let Value::Bool(passing) = cx.read_state(runtime, roots, 2)? else {
                    return Err(invalid());
                };
                self.branch = passing;
                self.phase = Phase::SkipPassingBranch;
            }
            Phase::SkipPassingBranch => {
                self.phase = if self.branch {
                    Phase::Some
                } else {
                    Phase::Callback
                }
            }
            Phase::PassTrue => self.phase = Phase::PassIndex,
            Phase::PassIndex => self.phase = Phase::PassWrite,
            Phase::PassWrite => {
                cx.write_state(runtime, roots, 2, Value::Bool(true))?;
                self.phase = Phase::PassJump;
            }
            Phase::PassJump => self.phase = Phase::Some,
            Phase::Pair => {
                let left = if self.operation == NativeDefaultMethod::Enumerate {
                    Value::U64(self.counter)
                } else {
                    cx.get(roots, ITEM)?
                };
                let right = if self.operation == NativeDefaultMethod::Enumerate {
                    cx.get(roots, ITEM)?
                } else {
                    cx.get(roots, OTHER)?
                };
                cx.set(runtime, roots, ITEM, Value::Tuple(vec![left, right]))?;
                self.phase = Phase::Some;
            }
            Phase::Some => {
                cx.set(
                    runtime,
                    roots,
                    OUTPUT,
                    cx.wrap(runtime, Some(cx.get(roots, ITEM)?))?,
                )?;
                self.phase = Phase::Return;
            }
            Phase::Return => {
                let slot = if !self.present {
                    OUTPUT
                } else if self.operation == NativeDefaultMethod::Chain {
                    if self.counter == 0 { NEXT } else { OTHER }
                } else if self.operation == NativeDefaultMethod::FilterMap && self.present {
                    MAPPED
                } else {
                    OUTPUT
                };
                let value = cx.get(roots, slot)?;
                let AbiType::Iter(item) = &contract.signature.result else {
                    return Err(invalid());
                };
                let output = AbiType::StandardEnum {
                    kind: StandardEnum::Option,
                    args: vec![item.as_ref().clone()],
                };
                if !runtime.matches_interface_method_abi(&value, &output, owner) {
                    return Err(invalid());
                }
                return Ok(NativeAction::Complete(value));
            }
            Phase::SwitchOne => self.phase = Phase::SwitchIndex,
            Phase::SwitchIndex => self.phase = Phase::SwitchWrite,
            Phase::SwitchWrite => {
                cx.write_state(runtime, roots, 2, Value::I32(1))?;
                self.phase = if matches!(contract.signature.params[0], AbiType::Iter(_)) {
                    Phase::SwitchClose
                } else {
                    Phase::SwitchJump
                };
            }
            Phase::SwitchClose => {
                self.close_left(runtime, owner, contract, roots, cx)?;
                self.phase = Phase::SwitchJump;
            }
            Phase::SwitchJump => self.phase = self.head(),
            Phase::ChainOne => self.phase = Phase::ChainCompare,
            Phase::ChainCompare => {
                self.branch = self.counter == 1;
                self.phase = Phase::ChainBranch;
            }
            Phase::ChainBranch => {
                self.phase = if self.branch {
                    Phase::RightNext
                } else {
                    Phase::Done
                }
            }
            Phase::RightNext => {
                let selection = self.right(contract)?;
                self.phase = Phase::WaitingRight;
                return match protocols::next(
                    runtime,
                    owner,
                    selection.witness(contract),
                    cx.capture(roots, 1)?,
                    &selection.optional(contract)?,
                )? {
                    ProtocolStep::Value(value) => self.receive(runtime, roots, cx, value),
                    ProtocolStep::Call(request) => Ok(NativeAction::Callback(request)),
                    ProtocolStep::BuiltinFailure(error) => Ok(NativeAction::BuiltinFailure(error)),
                };
            }
            Phase::RightTest => {
                self.present = cx.present(runtime, &cx.get(roots, OTHER)?)?;
                self.phase = Phase::RightBranch;
            }
            Phase::RightBranch => {
                self.phase = if self.present {
                    if self.operation == NativeDefaultMethod::Chain {
                        Phase::Return
                    } else {
                        Phase::RightRead
                    }
                } else {
                    self.done_phase(contract)?
                }
            }
            Phase::RightRead => {
                cx.set(
                    runtime,
                    roots,
                    OTHER,
                    cx.read(runtime, &cx.get(roots, OTHER)?)?,
                )?;
                self.phase = Phase::Pair;
            }
            Phase::Done => match self.operation {
                NativeDefaultMethod::Fuse | NativeDefaultMethod::TakeWhile => {
                    cx.set(runtime, roots, TEMP, Value::Bool(true))?;
                    self.phase = Phase::DoneIndex;
                }
                NativeDefaultMethod::Chain => {
                    cx.set(runtime, roots, TEMP, Value::I32(2))?;
                    self.phase = Phase::DoneIndex;
                }
                NativeDefaultMethod::Skip => {
                    cx.set(runtime, roots, TEMP, Value::U64(0))?;
                    self.phase = Phase::DoneIndex;
                }
                _ => return Err(invalid()),
            },
            Phase::DoneIndex => self.phase = Phase::DoneWrite,
            Phase::DoneWrite => {
                cx.write_state(runtime, roots, self.state_slot(), cx.get(roots, TEMP)?)?;
                self.phase = self.close_phase(contract)?;
            }
            Phase::CloseLeft => {
                self.close_left(runtime, owner, contract, roots, cx)?;
                self.phase = self.close_right_phase(contract)?;
            }
            Phase::CloseRight => {
                let witness = self.right(contract)?.witness(contract);
                runtime.iter_operation(
                    owner,
                    &cx.capture(roots, 1)?,
                    &witness.receiver,
                    IterOp::Close,
                )?;
                self.phase = Phase::None;
            }
            Phase::None => {
                cx.set(runtime, roots, OUTPUT, cx.wrap(runtime, None)?)?;
                self.present = false;
                self.phase = Phase::Return;
            }
            Phase::WaitingNext | Phase::WaitingCallback | Phase::WaitingRight => {
                return Err(invalid());
            }
        }
        Ok(NativeAction::Continue)
    }
    fn after_read(&self) -> Phase {
        match self.operation {
            NativeDefaultMethod::Skip | NativeDefaultMethod::Enumerate => Phase::Head,
            NativeDefaultMethod::SkipWhile => Phase::SkipPassingIndex,
            NativeDefaultMethod::Map
            | NativeDefaultMethod::Filter
            | NativeDefaultMethod::FilterMap
            | NativeDefaultMethod::Inspect
            | NativeDefaultMethod::TakeWhile => Phase::Callback,
            NativeDefaultMethod::Zip => Phase::RightNext,
            _ => Phase::Some,
        }
    }
    fn right(&self, contract: &EngineNativeImport) -> Result<IteratorSelection, RuntimeError> {
        let source = SourceSelection::select(contract, &contract.signature.params[1], None, 1)?;
        IteratorSelection::select(contract, &source.next(contract).receiver)
    }
    fn done_phase(&self, contract: &EngineNativeImport) -> Result<Phase, RuntimeError> {
        if matches!(
            self.operation,
            NativeDefaultMethod::Fuse
                | NativeDefaultMethod::TakeWhile
                | NativeDefaultMethod::Skip
                | NativeDefaultMethod::Chain
        ) {
            Ok(Phase::Done)
        } else {
            self.close_phase(contract)
        }
    }
    fn close_phase(&self, contract: &EngineNativeImport) -> Result<Phase, RuntimeError> {
        if matches!(contract.signature.params[0], AbiType::Iter(_)) {
            Ok(Phase::CloseLeft)
        } else {
            self.close_right_phase(contract)
        }
    }
    fn close_right_phase(&self, contract: &EngineNativeImport) -> Result<Phase, RuntimeError> {
        if matches!(
            self.operation,
            NativeDefaultMethod::Zip | NativeDefaultMethod::Chain
        ) && matches!(
            self.right(contract)?.witness(contract).receiver,
            AbiType::Iter(_)
        ) {
            Ok(Phase::CloseRight)
        } else {
            Ok(Phase::None)
        }
    }
    fn close_left(
        &self,
        runtime: &Runtime,
        owner: &LoadedModule,
        contract: &EngineNativeImport,
        roots: &RootSet,
        cx: StepContext,
    ) -> Result<(), RuntimeError> {
        runtime
            .iter_operation(
                owner,
                &cx.capture(roots, 0)?,
                &contract.signature.params[0],
                IterOp::Close,
            )
            .map(|_| ())
    }
    pub(super) fn receive(
        &mut self,
        runtime: &Runtime,
        roots: &RootSet,
        cx: StepContext,
        value: Value,
    ) -> Result<NativeAction, RuntimeError> {
        let (slot, phase) = match self.phase {
            Phase::WaitingNext => (NEXT, Phase::Test),
            Phase::WaitingRight => (OTHER, Phase::RightTest),
            Phase::WaitingCallback => match self.operation {
                NativeDefaultMethod::Map => (ITEM, Phase::Some),
                NativeDefaultMethod::Inspect => (MAPPED, Phase::Some),
                NativeDefaultMethod::FilterMap => (MAPPED, Phase::MappedTest),
                _ => (MAPPED, Phase::MappedBranch),
            },
            _ => return Err(invalid()),
        };
        cx.set(runtime, roots, slot, value)?;
        self.phase = phase;
        Ok(NativeAction::Continue)
    }
}
