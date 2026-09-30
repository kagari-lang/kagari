//! Indexed windows/chunks retain the original length and source guard.
use super::{END, INDEX, ITEM, NEXT, OUTPUT, STORAGE, StepContext, TEMP, invalid};
use crate::{
    LoadedModule, Runtime, RuntimeError,
    gc::RootSet,
    native::{
        NativeAction,
        lazy_iterators::source_list,
        protocols::{self, ProtocolStep},
        results,
    },
    numeric,
    value::Value,
};
use kagari_abi::{
    native_import::EngineNativeImport,
    operations::IterOp,
    standard::{bindings::NativeDefaultMethod, surface::StandardEnum, traits::StandardTrait},
    types::AbiType,
};
use kagari_bytecode::BinaryOp;
use kagari_common::identity::associated_type_id;
#[derive(Clone, Copy)]
enum Phase {
    Zero,
    Start,
    Remaining,
    Available,
    AvailableBranch,
    Resume,
    BoundRemaining,
    Short,
    BoundBranch,
    AddEnd,
    EndMove,
    BoundJump,
    Count,
    New,
    OffsetZero,
    LoopJump,
    LoopCompare,
    LoopBranch,
    IndexAdd,
    Get,
    WaitingGet,
    Read,
    Push,
    One,
    OffsetAdd,
    OffsetMove,
    BodyJump,
    Readonly,
    Some,
    AdvanceOne,
    AdvanceAdd,
    Commit,
    Return,
    Close,
    EndCommit,
    None,
}
pub(super) struct WindowStep {
    operation: NativeDefaultMethod,
    phase: Phase,
    start: u64,
    width: u64,
    len: u64,
    end: u64,
    offset: u64,
    branch: bool,
}
fn index(value: Value) -> Result<u64, RuntimeError> {
    let Value::U64(value) = value else {
        return Err(invalid());
    };
    Ok(value)
}
impl WindowStep {
    pub(super) fn start(operation: NativeDefaultMethod) -> Self {
        Self {
            operation,
            phase: Phase::Zero,
            start: 0,
            width: 0,
            len: 0,
            end: 0,
            offset: 0,
            branch: false,
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
            Phase::Zero => self.phase = Phase::Start,
            Phase::Start => {
                self.start = index(cx.read_state(runtime, roots, 2)?)?;
                self.width = index(cx.capture(roots, 1)?)?;
                self.len = index(cx.capture(roots, 4)?)?;
                self.phase = Phase::Remaining;
            }
            Phase::Remaining => {
                cx.set(
                    runtime,
                    roots,
                    TEMP,
                    numeric::binary(BinaryOp::Sub, Value::U64(self.len), Value::U64(self.start))?,
                )?;
                self.phase = Phase::Available;
            }
            Phase::Available => {
                let remaining = index(cx.get(roots, TEMP)?)?;
                self.branch = if self.operation == NativeDefaultMethod::ListWindows {
                    remaining >= self.width
                } else {
                    remaining > 0
                };
                self.phase = Phase::AvailableBranch;
            }
            Phase::AvailableBranch => {
                self.phase = if self.branch {
                    Phase::Resume
                } else {
                    Phase::Close
                }
            }
            Phase::Resume => {
                runtime.gc().resume_iter(&cx.capture(roots, 3)?)?;
                self.phase = Phase::BoundRemaining;
            }
            Phase::BoundRemaining => self.phase = Phase::Short,
            Phase::Short => {
                self.branch = self.len - self.start < self.width;
                self.phase = Phase::BoundBranch;
            }
            Phase::BoundBranch => {
                self.end = self.len;
                self.phase = if self.branch {
                    Phase::EndMove
                } else {
                    Phase::AddEnd
                };
            }
            Phase::AddEnd => {
                self.end = index(numeric::binary(
                    BinaryOp::Add,
                    Value::U64(self.start),
                    Value::U64(self.width),
                )?)?;
                self.phase = Phase::EndMove;
            }
            Phase::EndMove => {
                cx.set(runtime, roots, END, Value::U64(self.end))?;
                self.phase = Phase::BoundJump;
            }
            Phase::BoundJump => self.phase = Phase::Count,
            Phase::Count => self.phase = Phase::New,
            Phase::New => {
                cx.set(
                    runtime,
                    roots,
                    STORAGE,
                    Value::Array(runtime.gc().alloc_array(vec![])?),
                )?;
                self.phase = Phase::OffsetZero;
            }
            Phase::OffsetZero => {
                self.offset = 0;
                self.phase = Phase::LoopJump;
            }
            Phase::LoopJump => self.phase = Phase::LoopCompare,
            Phase::LoopCompare => {
                self.branch = self.offset < self.end - self.start;
                self.phase = Phase::LoopBranch;
            }
            Phase::LoopBranch => {
                self.phase = if self.branch {
                    Phase::IndexAdd
                } else {
                    Phase::Readonly
                }
            }
            Phase::IndexAdd => {
                cx.set(
                    runtime,
                    roots,
                    INDEX,
                    numeric::binary(
                        BinaryOp::Add,
                        Value::U64(self.start),
                        Value::U64(self.offset),
                    )?,
                )?;
                self.phase = Phase::Get;
            }
            Phase::Get => {
                let witness = source_list(owner, contract)?;
                let iterable = contract
                    .witnesses
                    .iter()
                    .find(|witness| {
                        witness.receiver == contract.signature.params[0]
                            && StandardTrait::from_id(&witness.interface.declaration)
                                == Some(StandardTrait::Iterable)
                    })
                    .ok_or_else(invalid)?;
                let item = iterable
                    .interface
                    .associated_types
                    .get(&associated_type_id(&iterable.interface.declaration, "Item"))
                    .ok_or_else(invalid)?;
                let optional = AbiType::StandardEnum {
                    kind: StandardEnum::Option,
                    args: vec![item.clone()],
                };
                self.phase = Phase::WaitingGet;
                return match protocols::list(
                    runtime,
                    owner,
                    witness,
                    2,
                    vec![cx.capture(roots, 0)?, cx.get(roots, INDEX)?],
                    &optional,
                )? {
                    ProtocolStep::Value(value) => self.receive(runtime, roots, cx, value),
                    ProtocolStep::Call(request) => Ok(NativeAction::Callback(request)),
                    ProtocolStep::BuiltinFailure(error) => Ok(NativeAction::BuiltinFailure(error)),
                };
            }
            Phase::Read => {
                if !cx.present(runtime, &cx.get(roots, NEXT)?)? {
                    return Ok(NativeAction::TypeMismatch("standard enum payload variant"));
                }
                cx.set(
                    runtime,
                    roots,
                    ITEM,
                    cx.read(runtime, &cx.get(roots, NEXT)?)?,
                )?;
                self.phase = Phase::Push;
            }
            Phase::Push => {
                let Value::Array(id) = cx.get(roots, STORAGE)? else {
                    return Err(invalid());
                };
                runtime.gc().array_push(id, cx.get(roots, ITEM)?)?;
                self.phase = Phase::One;
            }
            Phase::One => self.phase = Phase::OffsetAdd,
            Phase::OffsetAdd => {
                cx.set(
                    runtime,
                    roots,
                    TEMP,
                    numeric::binary(BinaryOp::Add, Value::U64(self.offset), Value::U64(1))?,
                )?;
                self.phase = Phase::OffsetMove;
            }
            Phase::OffsetMove => {
                self.offset = index(cx.get(roots, TEMP)?)?;
                self.phase = Phase::BodyJump;
            }
            Phase::BodyJump => self.phase = Phase::LoopCompare,
            Phase::Readonly => {
                cx.set(
                    runtime,
                    roots,
                    ITEM,
                    results::readonly_list(runtime, owner, contract, cx.get(roots, STORAGE)?)?,
                )?;
                self.phase = Phase::Some;
            }
            Phase::Some => {
                cx.set(
                    runtime,
                    roots,
                    OUTPUT,
                    cx.wrap(runtime, Some(cx.get(roots, ITEM)?))?,
                )?;
                self.phase = if self.operation == NativeDefaultMethod::ListWindows {
                    Phase::AdvanceOne
                } else {
                    Phase::Commit
                };
            }
            Phase::AdvanceOne => self.phase = Phase::AdvanceAdd,
            Phase::AdvanceAdd => {
                cx.set(
                    runtime,
                    roots,
                    TEMP,
                    numeric::binary(BinaryOp::Add, Value::U64(self.start), Value::U64(1))?,
                )?;
                self.phase = Phase::Commit;
            }
            Phase::Commit => {
                let next = if self.operation == NativeDefaultMethod::ListWindows {
                    cx.get(roots, TEMP)?
                } else {
                    Value::U64(self.end)
                };
                cx.write_state(runtime, roots, 2, next)?;
                self.phase = Phase::Return;
            }
            Phase::Return => return Ok(NativeAction::Complete(cx.get(roots, OUTPUT)?)),
            Phase::Close => {
                let iterator = contract
                    .witnesses
                    .iter()
                    .find(|witness| {
                        StandardTrait::from_id(&witness.interface.declaration)
                            == Some(StandardTrait::Iterator)
                    })
                    .ok_or_else(invalid)?;
                runtime.iter_operation(
                    owner,
                    &cx.capture(roots, 3)?,
                    &iterator.receiver,
                    IterOp::Close,
                )?;
                self.phase = Phase::EndCommit;
            }
            Phase::EndCommit => {
                cx.write_state(runtime, roots, 2, Value::U64(self.len))?;
                self.phase = Phase::None;
            }
            Phase::None => {
                cx.set(runtime, roots, OUTPUT, cx.wrap(runtime, None)?)?;
                self.phase = Phase::Return;
            }
            Phase::WaitingGet => return Err(invalid()),
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
        if !matches!(self.phase, Phase::WaitingGet) {
            return Err(invalid());
        }
        cx.set(runtime, roots, NEXT, value)?;
        self.phase = Phase::Read;
        Ok(NativeAction::Continue)
    }
}
