//! Stable bottom-up merging with once-only, ordered key decoration.
use super::{
    Buffers, ITERATOR, LEFT, OUTPUT, PreparationStep, RESULT, RIGHT, VALUES, invalid, witness,
};
use crate::{
    LoadedModule, Runtime, RuntimeError,
    gc::{CollectionIteration, RootSet},
    native::{
        NativeAction, callback,
        protocols::{self, ProtocolStep},
    },
    value::{EnumTag, Value},
};
use kagari_abi::{
    native_import::EngineNativeImport,
    operations::IterOp,
    standard::{StandardIntrinsic, surface::StandardEnum, traits::StandardTrait},
    types::AbiType,
};

#[derive(Clone, Copy)]
enum Bound {
    Middle,
    End,
    Width,
}
#[derive(Clone, Copy)]
enum BoundPhase {
    Remaining,
    Short,
    Branch,
    Add,
    Move,
    Jump,
}
#[derive(Clone, Copy)]
enum Phase {
    Values,
    Length,
    Iterator,
    Begin,
    FillIndex,
    FillJump,
    FillMore,
    FillBranch,
    FillRead,
    Extract,
    WaitingKey,
    Decorate,
    FillAppend,
    FillOne,
    FillAdd,
    FillMove,
    Width,
    WidthJump,
    WidthMore,
    WidthBranch,
    Output,
    Start,
    StartJump,
    StartMore,
    StartBranch,
    Bound(Bound, BoundPhase),
    LeftStart,
    RightStart,
    MergeJump,
    LeftMore,
    RightMore,
    AvailableBranch,
    AvailableTrue,
    AvailableMove,
    AvailableJump,
    MergeBranch,
    LeftAvailable,
    LeftAvailableBranch,
    RightDone,
    RightDoneBranch,
    TakeTrue,
    CompareLeft,
    CompareRight,
    LeftKeyIndex,
    LeftKey,
    RightKeyIndex,
    RightKey,
    Compare,
    WaitingComparison,
    Greater,
    False,
    NotGreater,
    RightChoiceMove,
    RightChoiceJump,
    TakeFalse,
    OuterChoiceMove,
    OuterChoiceJump,
    SelectedBranch,
    SelectedRead,
    SelectedOne,
    SelectedAdd,
    SelectedIndex,
    SelectedMove,
    SelectedJump,
    Append,
    StartMove,
    ValuesMove,
    WidthMove,
    Close,
    EndIteration,
    Undecorated,
    StripIndex,
    StripJump,
    StripMore,
    StripBranch,
    StripRead,
    StripFieldIndex,
    StripField,
    StripAppend,
    StripOne,
    StripAdd,
    StripMove,
}
pub(super) struct Sorting {
    operation: StandardIntrinsic,
    phase: Phase,
    guard: Option<CollectionIteration>,
    length: u64,
    index: u64,
    next: u64,
    width: u64,
    start: u64,
    middle: u64,
    end: u64,
    left: u64,
    right: u64,
    bounded: u64,
    condition: bool,
    right_more: bool,
    take_left: bool,
}
impl Sorting {
    pub(super) fn start(operation: StandardIntrinsic) -> Self {
        Self {
            operation,
            phase: Phase::Values,
            guard: None,
            length: 0,
            index: 0,
            next: 0,
            width: 0,
            start: 0,
            middle: 0,
            end: 0,
            left: 0,
            right: 0,
            bounded: 0,
            condition: false,
            right_more: false,
            take_left: false,
        }
    }
    fn keyed(&self) -> bool {
        self.operation == StandardIntrinsic::ArraySortByKey
    }
    fn accept(
        &mut self,
        runtime: &Runtime,
        owner: &LoadedModule,
        contract: &EngineNativeImport,
        roots: &RootSet,
        buffers: Buffers,
        value: Value,
    ) -> Result<NativeAction, RuntimeError> {
        let key = matches!(self.phase, Phase::WaitingKey);
        let output = if key {
            let AbiType::Function { result, .. } = &contract.signature.params[1] else {
                return Err(invalid());
            };
            result.as_ref().clone()
        } else {
            AbiType::StandardEnum {
                kind: StandardEnum::Ordering,
                args: vec![],
            }
        };
        if !runtime.matches_interface_method_abi(&value, &output, owner) {
            return Err(invalid());
        }
        buffers.set(runtime, roots, if key { RIGHT } else { RESULT }, value)?;
        self.phase = if key { Phase::Decorate } else { Phase::Greater };
        Ok(NativeAction::Continue)
    }
    pub(super) fn receive(
        &mut self,
        runtime: &Runtime,
        owner: &LoadedModule,
        contract: &EngineNativeImport,
        roots: &RootSet,
        buffers: Buffers,
        value: Value,
    ) -> Result<NativeAction, RuntimeError> {
        if !matches!(self.phase, Phase::WaitingKey | Phase::WaitingComparison) {
            return Err(invalid());
        }
        self.accept(runtime, owner, contract, roots, buffers, value)
    }

    fn bound_start(&self, bound: Bound) -> u64 {
        match bound {
            Bound::Middle => self.start,
            Bound::End => self.middle,
            Bound::Width => self.width,
        }
    }
    fn bound(&mut self, bound: Bound, phase: BoundPhase) -> Result<(), RuntimeError> {
        self.phase = Phase::Bound(
            bound,
            match phase {
                BoundPhase::Remaining => {
                    self.bounded = self
                        .length
                        .checked_sub(self.bound_start(bound))
                        .ok_or_else(invalid)?;
                    BoundPhase::Short
                }
                BoundPhase::Short => {
                    self.condition = self.bounded < self.width;
                    BoundPhase::Branch
                }
                BoundPhase::Branch => {
                    if self.condition {
                        self.bounded = self.length;
                        BoundPhase::Move
                    } else {
                        BoundPhase::Add
                    }
                }
                BoundPhase::Add => {
                    self.bounded = self
                        .bound_start(bound)
                        .checked_add(self.width)
                        .ok_or_else(invalid)?;
                    BoundPhase::Move
                }
                BoundPhase::Move => {
                    match bound {
                        Bound::Middle => self.middle = self.bounded,
                        Bound::End => self.end = self.bounded,
                        Bound::Width => self.next = self.bounded,
                    };
                    BoundPhase::Jump
                }
                BoundPhase::Jump => {
                    self.phase = match bound {
                        Bound::Middle => Phase::Bound(Bound::End, BoundPhase::Remaining),
                        Bound::End => Phase::LeftStart,
                        Bound::Width => Phase::WidthMove,
                    };
                    return Ok(());
                }
            },
        );
        Ok(())
    }
    fn advance_index(&mut self) -> Result<(), RuntimeError> {
        self.next = self.index.checked_add(1).ok_or_else(invalid)?;
        Ok(())
    }
    pub(super) fn advance(
        &mut self,
        runtime: &Runtime,
        owner: &LoadedModule,
        contract: &EngineNativeImport,
        roots: &RootSet,
        buffers: Buffers,
    ) -> Result<PreparationStep, RuntimeError> {
        match self.phase {
            Phase::Values => {
                if let Some(error) = buffers.new_array(runtime, roots, VALUES)? {
                    return Ok(PreparationStep::Action(error));
                }
                self.phase = Phase::Length;
            }
            Phase::Length => {
                let Value::Array(id) = roots.get(0).ok_or_else(invalid)? else {
                    return Err(invalid());
                };
                self.length = runtime.gc().array_len(id).ok_or_else(invalid)? as u64;
                self.phase = Phase::Iterator;
            }
            Phase::Iterator => {
                let value = runtime.iter_operation(
                    owner,
                    &roots.get(0).ok_or_else(invalid)?,
                    &contract.signature.params[0],
                    IterOp::New,
                )?;
                buffers.set(runtime, roots, ITERATOR, value)?;
                self.phase = Phase::Begin;
            }
            Phase::Begin => {
                self.guard = Some(
                    runtime
                        .gc()
                        .begin_collection_iteration(&buffers.get(roots, ITERATOR)?)?,
                );
                self.phase = Phase::FillIndex;
            }
            Phase::FillIndex => {
                self.index = 0;
                self.phase = Phase::FillJump;
            }
            Phase::FillJump => self.phase = Phase::FillMore,
            Phase::FillMore => {
                self.condition = self.index < self.length;
                self.phase = Phase::FillBranch;
            }
            Phase::FillBranch => {
                self.phase = if self.condition {
                    Phase::FillRead
                } else {
                    Phase::Width
                }
            }
            Phase::FillRead => {
                buffers.read(
                    runtime,
                    roots,
                    roots.get(0).ok_or_else(invalid)?,
                    self.index,
                    LEFT,
                )?;
                self.phase = if self.keyed() {
                    Phase::Extract
                } else {
                    Phase::FillAppend
                };
            }
            Phase::Extract => {
                let request = callback(
                    runtime,
                    &roots.get(1).ok_or_else(invalid)?,
                    &contract.signature.params[1],
                    vec![buffers.get(roots, LEFT)?],
                )?;
                self.phase = Phase::WaitingKey;
                return Ok(PreparationStep::Action(NativeAction::Callback(request)));
            }
            Phase::Decorate => {
                let pair =
                    Value::Tuple(vec![buffers.get(roots, RIGHT)?, buffers.get(roots, LEFT)?]);
                buffers.set(runtime, roots, LEFT, pair)?;
                self.phase = Phase::FillAppend;
            }
            Phase::FillAppend => {
                if let Some(error) =
                    buffers.push(runtime, roots, VALUES, buffers.get(roots, LEFT)?)?
                {
                    return Ok(PreparationStep::Action(error));
                }
                self.phase = Phase::FillOne;
            }
            Phase::FillOne => self.phase = Phase::FillAdd,
            Phase::FillAdd => {
                self.advance_index()?;
                self.phase = Phase::FillMove;
            }
            Phase::FillMove => {
                self.index = self.next;
                self.phase = Phase::FillJump;
            }
            Phase::Width => {
                self.width = 1;
                self.phase = Phase::WidthJump;
            }
            Phase::WidthJump => self.phase = Phase::WidthMore,
            Phase::WidthMore => {
                self.condition = self.width < self.length;
                self.phase = Phase::WidthBranch;
            }
            Phase::WidthBranch => {
                self.phase = if self.condition {
                    Phase::Output
                } else {
                    Phase::Close
                }
            }
            Phase::Output => {
                if let Some(error) = buffers.new_array(runtime, roots, OUTPUT)? {
                    return Ok(PreparationStep::Action(error));
                }
                self.phase = Phase::Start;
            }
            Phase::Start => {
                self.start = 0;
                self.phase = Phase::StartJump;
            }
            Phase::StartJump => self.phase = Phase::StartMore,
            Phase::StartMore => {
                self.condition = self.start < self.length;
                self.phase = Phase::StartBranch;
            }
            Phase::StartBranch => {
                self.phase = if self.condition {
                    Phase::Bound(Bound::Middle, BoundPhase::Remaining)
                } else {
                    Phase::ValuesMove
                }
            }
            Phase::Bound(bound, phase) => self.bound(bound, phase)?,
            Phase::LeftStart => {
                self.left = self.start;
                self.phase = Phase::RightStart;
            }
            Phase::RightStart => {
                self.right = self.middle;
                self.phase = Phase::MergeJump;
            }
            Phase::MergeJump => self.phase = Phase::LeftMore,
            Phase::LeftMore => {
                self.condition = self.left < self.middle;
                self.phase = Phase::RightMore;
            }
            Phase::RightMore => {
                self.right_more = self.right < self.end;
                self.phase = Phase::AvailableBranch;
            }
            Phase::AvailableBranch => {
                self.phase = if self.condition {
                    Phase::AvailableTrue
                } else {
                    self.condition = self.right_more;
                    Phase::AvailableMove
                };
            }
            Phase::AvailableTrue => {
                self.condition = true;
                self.phase = Phase::AvailableMove;
            }
            Phase::AvailableMove => self.phase = Phase::AvailableJump,
            Phase::AvailableJump => self.phase = Phase::MergeBranch,
            Phase::MergeBranch => {
                self.phase = if self.condition {
                    Phase::LeftAvailable
                } else {
                    Phase::StartMove
                }
            }
            Phase::LeftAvailable => {
                self.condition = self.left < self.middle;
                self.phase = Phase::LeftAvailableBranch;
            }
            Phase::LeftAvailableBranch => {
                self.phase = if self.condition {
                    Phase::RightDone
                } else {
                    Phase::TakeFalse
                }
            }
            Phase::RightDone => {
                self.condition = self.right == self.end;
                self.phase = Phase::RightDoneBranch;
            }
            Phase::RightDoneBranch => {
                self.phase = if self.condition {
                    Phase::TakeTrue
                } else {
                    Phase::CompareLeft
                }
            }
            Phase::TakeTrue => {
                self.take_left = true;
                self.phase = Phase::RightChoiceMove;
            }
            Phase::CompareLeft => {
                buffers.read(runtime, roots, buffers.get(roots, VALUES)?, self.left, LEFT)?;
                self.phase = Phase::CompareRight;
            }
            Phase::CompareRight => {
                buffers.read(
                    runtime,
                    roots,
                    buffers.get(roots, VALUES)?,
                    self.right,
                    RIGHT,
                )?;
                self.phase = if self.keyed() {
                    Phase::LeftKeyIndex
                } else {
                    Phase::Compare
                };
            }
            Phase::LeftKeyIndex => self.phase = Phase::LeftKey,
            Phase::LeftKey => {
                buffers.field(runtime, roots, LEFT, 0)?;
                self.phase = Phase::RightKeyIndex;
            }
            Phase::RightKeyIndex => self.phase = Phase::RightKey,
            Phase::RightKey => {
                buffers.field(runtime, roots, RIGHT, 0)?;
                self.phase = Phase::Compare;
            }
            Phase::Compare => {
                let left = buffers.get(roots, LEFT)?;
                let right = buffers.get(roots, RIGHT)?;
                let step = if self.operation == StandardIntrinsic::ArraySortBy {
                    ProtocolStep::Call(callback(
                        runtime,
                        &roots.get(1).ok_or_else(invalid)?,
                        &contract.signature.params[1],
                        vec![left, right],
                    )?)
                } else {
                    let ty = if self.keyed() {
                        let AbiType::Function { result, .. } = &contract.signature.params[1] else {
                            return Err(invalid());
                        };
                        result.as_ref()
                    } else {
                        let AbiType::Array(item, _) = &contract.signature.params[0] else {
                            return Err(invalid());
                        };
                        item.as_ref()
                    };
                    protocols::compare(
                        runtime,
                        owner,
                        witness(contract, ty, StandardTrait::Ord)?,
                        left,
                        right,
                    )?
                };
                match step {
                    ProtocolStep::Value(value) => {
                        return self
                            .accept(runtime, owner, contract, roots, buffers, value)
                            .map(PreparationStep::Action);
                    }
                    ProtocolStep::Call(request) => {
                        self.phase = Phase::WaitingComparison;
                        return Ok(PreparationStep::Action(NativeAction::Callback(request)));
                    }
                    ProtocolStep::BuiltinFailure(error) => {
                        return Ok(PreparationStep::Action(NativeAction::BuiltinFailure(error)));
                    }
                }
            }
            Phase::Greater => {
                let Value::Enum(id) = buffers.get(roots, RESULT)? else {
                    return Err(invalid());
                };
                self.condition = runtime.gc().enum_snapshot(id).ok_or_else(invalid)?.tag
                    == EnumTag::OrderingGreater;
                self.phase = Phase::False;
            }
            Phase::False => self.phase = Phase::NotGreater,
            Phase::NotGreater => {
                self.take_left = !self.condition;
                self.phase = Phase::RightChoiceMove;
            }
            Phase::RightChoiceMove => self.phase = Phase::RightChoiceJump,
            Phase::RightChoiceJump => self.phase = Phase::OuterChoiceMove,
            Phase::TakeFalse => {
                self.take_left = false;
                self.phase = Phase::OuterChoiceMove;
            }
            Phase::OuterChoiceMove => self.phase = Phase::OuterChoiceJump,
            Phase::OuterChoiceJump => self.phase = Phase::SelectedBranch,
            Phase::SelectedBranch => self.phase = Phase::SelectedRead,
            Phase::SelectedRead => {
                buffers.read(
                    runtime,
                    roots,
                    buffers.get(roots, VALUES)?,
                    if self.take_left {
                        self.left
                    } else {
                        self.right
                    },
                    RESULT,
                )?;
                self.phase = Phase::SelectedOne;
            }
            Phase::SelectedOne => self.phase = Phase::SelectedAdd,
            Phase::SelectedAdd => {
                self.next = if self.take_left {
                    self.left
                } else {
                    self.right
                }
                .checked_add(1)
                .ok_or_else(invalid)?;
                self.phase = Phase::SelectedIndex;
            }
            Phase::SelectedIndex => {
                if self.take_left {
                    self.left = self.next
                } else {
                    self.right = self.next
                }
                self.phase = Phase::SelectedMove;
            }
            Phase::SelectedMove => self.phase = Phase::SelectedJump,
            Phase::SelectedJump => self.phase = Phase::Append,
            Phase::Append => {
                if let Some(error) =
                    buffers.push(runtime, roots, OUTPUT, buffers.get(roots, RESULT)?)?
                {
                    return Ok(PreparationStep::Action(error));
                }
                self.phase = Phase::MergeJump;
            }
            Phase::StartMove => {
                self.start = self.end;
                self.phase = Phase::StartJump;
            }
            Phase::ValuesMove => {
                buffers.set(runtime, roots, VALUES, buffers.get(roots, OUTPUT)?)?;
                self.phase = Phase::Bound(Bound::Width, BoundPhase::Remaining);
            }
            Phase::WidthMove => {
                self.width = self.next;
                self.phase = Phase::WidthJump;
            }
            Phase::Close => {
                let AbiType::Array(item, _) = &contract.signature.params[0] else {
                    return Err(invalid());
                };
                runtime.iter_operation(
                    owner,
                    &buffers.get(roots, ITERATOR)?,
                    &AbiType::Iter(item.clone()),
                    IterOp::Close,
                )?;
                self.phase = Phase::EndIteration;
            }
            Phase::EndIteration => {
                self.guard.take();
                if !self.keyed() {
                    return Ok(PreparationStep::Ready(VALUES));
                }
                self.phase = Phase::Undecorated;
            }
            Phase::Undecorated => {
                if let Some(error) = buffers.new_array(runtime, roots, OUTPUT)? {
                    return Ok(PreparationStep::Action(error));
                }
                self.phase = Phase::StripIndex;
            }
            Phase::StripIndex => {
                self.index = 0;
                self.phase = Phase::StripJump;
            }
            Phase::StripJump => self.phase = Phase::StripMore,
            Phase::StripMore => {
                self.condition = self.index < self.length;
                self.phase = Phase::StripBranch;
            }
            Phase::StripBranch => {
                if !self.condition {
                    return Ok(PreparationStep::Ready(OUTPUT));
                }
                self.phase = Phase::StripRead;
            }
            Phase::StripRead => {
                buffers.read(
                    runtime,
                    roots,
                    buffers.get(roots, VALUES)?,
                    self.index,
                    LEFT,
                )?;
                self.phase = Phase::StripFieldIndex;
            }
            Phase::StripFieldIndex => self.phase = Phase::StripField,
            Phase::StripField => {
                buffers.field(runtime, roots, LEFT, 1)?;
                self.phase = Phase::StripAppend;
            }
            Phase::StripAppend => {
                if let Some(error) =
                    buffers.push(runtime, roots, OUTPUT, buffers.get(roots, LEFT)?)?
                {
                    return Ok(PreparationStep::Action(error));
                }
                self.phase = Phase::StripOne;
            }
            Phase::StripOne => self.phase = Phase::StripAdd,
            Phase::StripAdd => {
                self.advance_index()?;
                self.phase = Phase::StripMove;
            }
            Phase::StripMove => {
                self.index = self.next;
                self.phase = Phase::StripJump;
            }
            Phase::WaitingKey | Phase::WaitingComparison => return Err(invalid()),
        }
        Ok(PreparationStep::Action(NativeAction::Continue))
    }
}
