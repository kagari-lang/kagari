//! Compare each candidate with the last retained element, then prepare a mask.
use super::{Buffers, LEFT, PreparationStep, RIGHT, VALUES, invalid, witness};
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
    native_import::EngineNativeImport, scalar::BuiltinType, standard::traits::StandardTrait,
    types::AbiType,
};

enum Phase {
    Mask,
    Length,
    Previous,
    Zero,
    Index,
    Jump,
    More,
    MoreBranch,
    First,
    FirstBranch,
    True,
    Left,
    Right,
    Equal,
    Waiting,
    False,
    NotEqual,
    KeepMove,
    KeepJump,
    KeepBranch,
    PreviousMove,
    Unit,
    UnitMove,
    UnitJump,
    Append,
    One,
    Add,
    MoveIndex,
}
pub(super) struct Dedup {
    phase: Phase,
    length: u64,
    index: u64,
    next: u64,
    previous: u64,
    keep: bool,
    equal: bool,
}
impl Dedup {
    pub(super) fn start() -> Self {
        Self {
            phase: Phase::Mask,
            length: 0,
            index: 0,
            next: 0,
            previous: 0,
            keep: false,
            equal: false,
        }
    }
    fn accept(
        &mut self,
        runtime: &Runtime,
        owner: &LoadedModule,
        value: Value,
    ) -> Result<NativeAction, RuntimeError> {
        if !runtime.matches_interface_method_abi(
            &value,
            &AbiType::Builtin(BuiltinType::Bool),
            owner,
        ) {
            return Err(invalid());
        }
        let Value::Bool(equal) = value else {
            return Err(invalid());
        };
        self.equal = equal;
        self.phase = Phase::False;
        Ok(NativeAction::Continue)
    }
    pub(super) fn receive(
        &mut self,
        runtime: &Runtime,
        owner: &LoadedModule,
        value: Value,
    ) -> Result<NativeAction, RuntimeError> {
        if !matches!(self.phase, Phase::Waiting) {
            return Err(invalid());
        }
        self.accept(runtime, owner, value)
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
            Phase::Mask => {
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
                self.phase = Phase::Previous;
            }
            Phase::Previous => {
                self.previous = 0;
                self.phase = Phase::Zero;
            }
            Phase::Zero => self.phase = Phase::Index,
            Phase::Index => {
                self.index = 0;
                self.phase = Phase::Jump;
            }
            Phase::Jump => self.phase = Phase::More,
            Phase::More => {
                self.keep = self.index < self.length;
                self.phase = Phase::MoreBranch;
            }
            Phase::MoreBranch => {
                if !self.keep {
                    return Ok(PreparationStep::Ready(VALUES));
                }
                self.phase = Phase::First;
            }
            Phase::First => {
                self.keep = self.index == 0;
                self.phase = Phase::FirstBranch;
            }
            Phase::FirstBranch => self.phase = if self.keep { Phase::True } else { Phase::Left },
            Phase::True => {
                self.keep = true;
                self.phase = Phase::KeepMove;
            }
            Phase::Left => {
                buffers.read(
                    runtime,
                    roots,
                    roots.get(0).ok_or_else(invalid)?,
                    self.previous,
                    LEFT,
                )?;
                self.phase = Phase::Right;
            }
            Phase::Right => {
                buffers.read(
                    runtime,
                    roots,
                    roots.get(0).ok_or_else(invalid)?,
                    self.index,
                    RIGHT,
                )?;
                self.phase = Phase::Equal;
            }
            Phase::Equal => {
                let AbiType::Array(item, _) = &contract.signature.params[0] else {
                    return Err(invalid());
                };
                match protocols::equal(
                    runtime,
                    owner,
                    witness(contract, item, StandardTrait::PartialEq)?,
                    buffers.get(roots, LEFT)?,
                    buffers.get(roots, RIGHT)?,
                )? {
                    ProtocolStep::Value(value) => {
                        return self
                            .accept(runtime, owner, value)
                            .map(PreparationStep::Action);
                    }
                    ProtocolStep::Call(request) => {
                        self.phase = Phase::Waiting;
                        return Ok(PreparationStep::Action(NativeAction::Callback(request)));
                    }
                    ProtocolStep::BuiltinFailure(error) => {
                        return Ok(PreparationStep::Action(NativeAction::BuiltinFailure(error)));
                    }
                }
            }
            Phase::False => self.phase = Phase::NotEqual,
            Phase::NotEqual => {
                self.keep = !self.equal;
                self.phase = Phase::KeepMove;
            }
            Phase::KeepMove => self.phase = Phase::KeepJump,
            Phase::KeepJump => self.phase = Phase::KeepBranch,
            Phase::KeepBranch => {
                self.phase = if self.keep {
                    Phase::PreviousMove
                } else {
                    Phase::Unit
                }
            }
            Phase::PreviousMove => {
                self.previous = self.index;
                self.phase = Phase::Unit;
            }
            Phase::Unit => self.phase = Phase::UnitMove,
            Phase::UnitMove => self.phase = Phase::UnitJump,
            Phase::UnitJump => self.phase = Phase::Append,
            Phase::Append => {
                if let Some(error) = buffers.push(runtime, roots, VALUES, Value::Bool(self.keep))? {
                    return Ok(PreparationStep::Action(error));
                }
                self.phase = Phase::One;
            }
            Phase::One => self.phase = Phase::Add,
            Phase::Add => {
                self.next = self.index.checked_add(1).ok_or_else(invalid)?;
                self.phase = Phase::MoveIndex;
            }
            Phase::MoveIndex => {
                self.index = self.next;
                self.phase = Phase::Jump;
            }
            Phase::Waiting => return Err(invalid()),
        }
        Ok(PreparationStep::Action(NativeAction::Continue))
    }
}
