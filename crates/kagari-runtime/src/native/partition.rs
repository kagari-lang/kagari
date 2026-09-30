//! Partition once, then construct the two selected destinations from left to right.
use crate::{
    LoadedModule, Runtime, RuntimeError,
    gc::{CollectionIteration, RootSet},
    native::{
        NativeAction, callback,
        destination_factory::Factory,
        protocols::{self, ProtocolStep},
        sources::IteratorSelection,
    },
    value::{EnumTag, Value},
};
use kagari_abi::{
    native_import::EngineNativeImport, operations::IterOp, standard::StandardIntrinsic,
    types::AbiType,
};
use kagari_common::collection::CollectionAccess;

const ACCEPTED: usize = 0;
const REJECTED: usize = 1;
const NEXT: usize = 2;
const ITEM: usize = 3;
const LEFT: usize = 4;
const RIGHT: usize = 5;
const SCRATCH_ROOTS: usize = 6;
fn invalid() -> RuntimeError {
    RuntimeError::module_validation("native partition contract mismatch")
}
#[derive(Clone, Copy)]
enum Phase {
    NewRejected,
    Begin,
    Jump,
    Next,
    WaitingNext,
    Test,
    Branch,
    Read,
    Predicate,
    WaitingPredicate,
    PredicateBranch,
    Append,
    Close,
    End,
    Left,
    WaitingLeft,
    Right,
    WaitingRight,
    Tuple,
}
pub(super) struct Partition {
    scratch: usize,
    next: IteratorSelection,
    factories: [Factory; 2],
    phase: Phase,
    guarded: bool,
    condition: bool,
    guard: Option<CollectionIteration>,
}
impl Partition {
    pub(super) fn start(
        owner: &LoadedModule,
        contract: &EngineNativeImport,
        arguments: &[Value],
    ) -> Result<Self, RuntimeError> {
        let next = IteratorSelection::select(contract, &contract.signature.params[0])?;
        let buffer = AbiType::Array(
            Box::new(next.item(contract)?.clone()),
            CollectionAccess::Mutable,
        );
        let AbiType::Tuple(outputs) = &contract.signature.result else {
            return Err(invalid());
        };
        let [left, right] = outputs.as_slice() else {
            return Err(invalid());
        };
        let scratch = arguments.len();
        let left = Factory::select(
            owner,
            contract,
            left,
            &buffer,
            scratch + ACCEPTED,
            scratch + SCRATCH_ROOTS,
        )?;
        let right = Factory::select(
            owner,
            contract,
            right,
            &buffer,
            scratch + REJECTED,
            scratch + SCRATCH_ROOTS + left.scratch_roots(),
        )?;
        Ok(Self {
            scratch,
            next,
            factories: [left, right],
            phase: Phase::NewRejected,
            guarded: matches!(next.witness(contract).receiver, AbiType::Iter(_)),
            condition: false,
            guard: None,
        })
    }
    pub(super) fn scratch_roots(&self) -> usize {
        SCRATCH_ROOTS
            + self
                .factories
                .iter()
                .map(Factory::scratch_roots)
                .sum::<usize>()
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
    fn allocate(
        &self,
        runtime: &Runtime,
        roots: &RootSet,
        slot: usize,
    ) -> Result<NativeAction, RuntimeError> {
        match runtime.invoke_standard_builtin(StandardIntrinsic::ArrayListNew, &[]) {
            Ok(value) => {
                self.set(runtime, roots, slot, value)?;
                Ok(NativeAction::Continue)
            }
            Err(error) => Ok(NativeAction::BuiltinFailure(error)),
        }
    }
    pub(super) fn initialize(
        &self,
        runtime: &Runtime,
        roots: &RootSet,
    ) -> Result<NativeAction, RuntimeError> {
        self.allocate(runtime, roots, ACCEPTED)
    }
    fn snapshot(
        &self,
        runtime: &Runtime,
        roots: &RootSet,
    ) -> Result<(EnumTag, Option<Value>), RuntimeError> {
        let Value::Enum(id) = self.get(roots, NEXT)? else {
            return Err(invalid());
        };
        let snapshot = runtime.gc().enum_snapshot(id).ok_or_else(invalid)?;
        Ok((snapshot.tag, snapshot.fields.into_iter().next()))
    }
    fn factory_action(
        &mut self,
        runtime: &Runtime,
        owner: &LoadedModule,
        contract: &EngineNativeImport,
        roots: &RootSet,
        side: usize,
        action: NativeAction,
    ) -> Result<NativeAction, RuntimeError> {
        if let NativeAction::Complete(value) = action {
            let AbiType::Tuple(outputs) = &contract.signature.result else {
                return Err(invalid());
            };
            if !runtime.matches_interface_method_abi(&value, &outputs[side], owner) {
                return Err(invalid());
            }
            self.set(runtime, roots, if side == 0 { LEFT } else { RIGHT }, value)?;
            self.phase = if side == 0 {
                Phase::Right
            } else {
                Phase::Tuple
            };
            Ok(NativeAction::Continue)
        } else {
            Ok(action)
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
        match self.phase {
            Phase::WaitingNext => {
                if !runtime.matches_interface_method_abi(
                    &value,
                    &self.next.optional(contract)?,
                    owner,
                ) {
                    return Err(invalid());
                }
                self.set(runtime, roots, NEXT, value)?;
                self.phase = Phase::Test;
            }
            Phase::WaitingPredicate => {
                let Value::Bool(condition) = value else {
                    return Err(invalid());
                };
                self.condition = condition;
                self.phase = Phase::PredicateBranch;
            }
            Phase::WaitingLeft | Phase::WaitingRight => {
                let side = usize::from(matches!(self.phase, Phase::WaitingRight));
                let action =
                    self.factories[side].receive(runtime, owner, contract, roots, value)?;
                return self.factory_action(runtime, owner, contract, roots, side, action);
            }
            _ => return Err(invalid()),
        }
        Ok(NativeAction::Continue)
    }
    pub(super) fn advance(
        &mut self,
        runtime: &Runtime,
        owner: &LoadedModule,
        contract: &EngineNativeImport,
        roots: &RootSet,
    ) -> Result<NativeAction, RuntimeError> {
        match self.phase {
            Phase::NewRejected => {
                let action = self.allocate(runtime, roots, REJECTED)?;
                self.phase = if self.guarded {
                    Phase::Begin
                } else {
                    Phase::Jump
                };
                return Ok(action);
            }
            Phase::Begin => {
                self.guard = Some(
                    runtime
                        .gc()
                        .begin_collection_iteration(&roots.get(0).ok_or_else(invalid)?)?,
                );
                self.phase = Phase::Jump;
            }
            Phase::Jump => self.phase = Phase::Next,
            Phase::Next => {
                let step = protocols::next(
                    runtime,
                    owner,
                    self.next.witness(contract),
                    roots.get(0).ok_or_else(invalid)?,
                    &self.next.optional(contract)?,
                )?;
                self.phase = Phase::WaitingNext;
                return match step {
                    ProtocolStep::Value(value) => {
                        self.receive(runtime, owner, contract, roots, value)
                    }
                    ProtocolStep::Call(request) => Ok(NativeAction::Callback(request)),
                    ProtocolStep::BuiltinFailure(error) => Ok(NativeAction::BuiltinFailure(error)),
                };
            }
            Phase::Test => {
                self.condition = self.snapshot(runtime, roots)?.0 == EnumTag::OptionSome;
                self.phase = Phase::Branch;
            }
            Phase::Branch => {
                self.phase = if self.condition {
                    Phase::Read
                } else if self.guarded {
                    Phase::Close
                } else {
                    Phase::Left
                }
            }
            Phase::Read => {
                self.set(
                    runtime,
                    roots,
                    ITEM,
                    self.snapshot(runtime, roots)?.1.ok_or_else(invalid)?,
                )?;
                self.phase = Phase::Predicate;
            }
            Phase::Predicate => {
                let request = callback(
                    runtime,
                    &roots.get(1).ok_or_else(invalid)?,
                    &contract.signature.params[1],
                    vec![self.get(roots, ITEM)?],
                )?;
                self.phase = Phase::WaitingPredicate;
                return Ok(NativeAction::Callback(request));
            }
            Phase::PredicateBranch => self.phase = Phase::Append,
            Phase::Append => {
                if let Err(error) = runtime.invoke_standard_builtin(
                    StandardIntrinsic::ArrayPush,
                    &[
                        self.get(roots, if self.condition { ACCEPTED } else { REJECTED })?,
                        self.get(roots, ITEM)?,
                    ],
                ) {
                    return Ok(NativeAction::BuiltinFailure(error));
                }
                self.phase = Phase::Jump;
            }
            Phase::Close => {
                runtime.iter_operation(
                    owner,
                    &roots.get(0).ok_or_else(invalid)?,
                    &self.next.witness(contract).receiver,
                    IterOp::Close,
                )?;
                self.phase = Phase::End;
            }
            Phase::End => {
                self.guard.take();
                self.phase = Phase::Left;
            }
            Phase::Left | Phase::Right => {
                let side = usize::from(matches!(self.phase, Phase::Right));
                self.phase = if side == 0 {
                    Phase::WaitingLeft
                } else {
                    Phase::WaitingRight
                };
                let action = self.factories[side].initialize(runtime, owner, contract, roots)?;
                return self.factory_action(runtime, owner, contract, roots, side, action);
            }
            Phase::WaitingLeft | Phase::WaitingRight => {
                let side = usize::from(matches!(self.phase, Phase::WaitingRight));
                let action = self.factories[side].advance(runtime, owner, contract, roots)?;
                return self.factory_action(runtime, owner, contract, roots, side, action);
            }
            Phase::Tuple => {
                return Ok(NativeAction::Complete(Value::Tuple(vec![
                    self.get(roots, LEFT)?,
                    self.get(roots, RIGHT)?,
                ])));
            }
            Phase::WaitingNext | Phase::WaitingPredicate => return Err(invalid()),
        }
        Ok(NativeAction::Continue)
    }
}
