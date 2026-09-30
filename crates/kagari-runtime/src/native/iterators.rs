//! Terminal traversal policy; each phase preserves an original logical operation.
use crate::{
    LoadedModule, Runtime, RuntimeError, RuntimeErrorKind,
    gc::{CollectionIteration, RootSet},
    native::{
        NativeAction, callback,
        protocols::{self, ProtocolStep},
    },
    value::{EnumTag, Value},
};
use kagari_abi::{
    native_import::{EngineNativeImport, NativeWitness},
    operations::IterOp,
    standard::{bindings::NativeDefaultMethod, surface::StandardEnum, traits::StandardTrait},
    types::AbiType,
};
use kagari_common::identity::associated_type_id;
const RESULT: usize = 0;
const NEXT: usize = 1;
const ITEM: usize = 2;

enum Phase {
    Begin,
    EntryJump,
    Next,
    WaitingNext,
    Test,
    Branch,
    Read,
    Callback,
    WaitingCallback,
    PredicateBranch,
    Decision,
    DecisionMove,
    ExitJump,
    One,
    Add,
    Move,
    BodyJump,
    Close,
    End,
}

pub(super) struct IteratorInvocation {
    operation: NativeDefaultMethod,
    phase: Phase,
    scratch: usize,
    guarded: bool,
    guard: Option<CollectionIteration>,
    pub(super) initial: Option<Value>,
    present: bool,
}

fn invalid() -> RuntimeError {
    RuntimeError::module_validation("native terminal contract mismatch")
}

impl IteratorInvocation {
    pub(super) fn start(
        runtime: &Runtime,
        operation: NativeDefaultMethod,
        contract: &EngineNativeImport,
        arguments: &[Value],
    ) -> Result<Self, RuntimeError> {
        let initial = match operation {
            NativeDefaultMethod::Count => Value::U64(0),
            NativeDefaultMethod::Fold => arguments[1].clone(),
            NativeDefaultMethod::ForEach => Value::Unit,
            NativeDefaultMethod::Any | NativeDefaultMethod::All => {
                Value::Bool(operation == NativeDefaultMethod::All)
            }
            NativeDefaultMethod::Find | NativeDefaultMethod::Last => {
                Value::Enum(runtime.alloc_enum(EnumTag::OptionNone, vec![])?)
            }
            _ => return Err(invalid()),
        };
        let guarded = matches!(contract.signature.params.first(), Some(AbiType::Iter(_)));
        Ok(Self {
            operation,
            phase: if guarded {
                Phase::Begin
            } else {
                Phase::EntryJump
            },
            scratch: arguments.len(),
            guarded,
            guard: None,
            initial: Some(initial),
            present: false,
        })
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
    fn complete(
        &self,
        runtime: &Runtime,
        owner: &LoadedModule,
        contract: &EngineNativeImport,
        roots: &RootSet,
    ) -> Result<NativeAction, RuntimeError> {
        let value = self.get(roots, RESULT)?;
        if !runtime.matches_interface_method_abi(&value, &contract.signature.result, owner) {
            return Err(invalid());
        }
        Ok(NativeAction::Complete(value))
    }
    fn exit(
        &mut self,
        runtime: &Runtime,
        owner: &LoadedModule,
        contract: &EngineNativeImport,
        roots: &RootSet,
    ) -> Result<NativeAction, RuntimeError> {
        if self.guarded {
            self.phase = Phase::Close;
            Ok(NativeAction::Continue)
        } else {
            self.complete(runtime, owner, contract, roots)
        }
    }
    fn witness<'a>(
        &self,
        contract: &'a EngineNativeImport,
    ) -> Result<&'a NativeWitness, RuntimeError> {
        contract
            .witnesses
            .iter()
            .find(|witness| {
                witness.receiver == contract.signature.params[0]
                    && StandardTrait::from_id(&witness.interface.declaration)
                        == Some(StandardTrait::Iterator)
            })
            .ok_or_else(invalid)
    }
    fn optional(&self, contract: &EngineNativeImport) -> Result<AbiType, RuntimeError> {
        let witness = self.witness(contract)?;
        let item = witness
            .interface
            .associated_types
            .get(&associated_type_id(&witness.interface.declaration, "Item"))
            .ok_or_else(invalid)?;
        Ok(AbiType::StandardEnum {
            kind: StandardEnum::Option,
            args: vec![item.clone()],
        })
    }
    pub(super) fn advance(
        &mut self,
        runtime: &Runtime,
        owner: &LoadedModule,
        contract: &EngineNativeImport,
        roots: &RootSet,
    ) -> Result<NativeAction, RuntimeError> {
        match self.phase {
            Phase::Begin => {
                self.guard = Some(
                    runtime
                        .gc()
                        .begin_collection_iteration(&roots.get(0).ok_or_else(invalid)?)?,
                );
                self.phase = Phase::EntryJump;
            }
            Phase::EntryJump | Phase::BodyJump => self.phase = Phase::Next,
            Phase::Next => {
                let optional = self.optional(contract)?;
                match protocols::next(
                    runtime,
                    owner,
                    self.witness(contract)?,
                    roots.get(0).ok_or_else(invalid)?,
                    &optional,
                )? {
                    ProtocolStep::Value(value) => {
                        self.set(runtime, roots, NEXT, value)?;
                        self.phase = Phase::Test;
                    }
                    ProtocolStep::Call(request) => {
                        self.phase = Phase::WaitingNext;
                        return Ok(NativeAction::Callback(request));
                    }
                }
            }
            Phase::Test => {
                let Value::Enum(id) = self.get(roots, NEXT)? else {
                    return Err(invalid());
                };
                let snapshot = runtime.gc().enum_snapshot(id).ok_or_else(invalid)?;
                self.present = match (snapshot.tag, snapshot.fields.len()) {
                    (EnumTag::OptionSome, 1) => true,
                    (EnumTag::OptionNone, 0) => false,
                    _ => return Err(invalid()),
                };
                if !runtime.matches_interface_method_abi(
                    &Value::Enum(id),
                    &self.optional(contract)?,
                    owner,
                ) {
                    return Err(invalid());
                }
                self.phase = Phase::Branch;
            }
            Phase::Branch => {
                if !self.present {
                    return self.exit(runtime, owner, contract, roots);
                }
                self.phase = Phase::Read;
            }
            Phase::Read => {
                let Value::Enum(id) = self.get(roots, NEXT)? else {
                    return Err(invalid());
                };
                let item = runtime
                    .gc()
                    .enum_snapshot(id)
                    .and_then(|snapshot| snapshot.fields.into_iter().next())
                    .ok_or_else(invalid)?;
                self.set(runtime, roots, ITEM, item)?;
                self.phase = match self.operation {
                    NativeDefaultMethod::Count => Phase::One,
                    NativeDefaultMethod::Last => Phase::Move,
                    _ => Phase::Callback,
                };
            }
            Phase::Callback => {
                let slot = if self.operation == NativeDefaultMethod::Fold {
                    2
                } else {
                    1
                };
                let mut arguments = vec![];
                if self.operation == NativeDefaultMethod::Fold {
                    arguments.push(self.get(roots, RESULT)?);
                }
                arguments.push(self.get(roots, ITEM)?);
                let request = callback(
                    runtime,
                    &roots.get(slot).ok_or_else(invalid)?,
                    &contract.signature.params[slot],
                    arguments,
                )?;
                self.phase = Phase::WaitingCallback;
                return Ok(NativeAction::Callback(request));
            }
            Phase::PredicateBranch => {
                let Value::Bool(matched) = self.get(roots, ITEM)? else {
                    return Err(invalid());
                };
                let found = if self.operation == NativeDefaultMethod::All {
                    !matched
                } else {
                    matched
                };
                self.phase = if found {
                    if self.operation == NativeDefaultMethod::Find {
                        Phase::DecisionMove
                    } else {
                        Phase::Decision
                    }
                } else {
                    Phase::Next
                };
            }
            Phase::Decision => {
                self.set(
                    runtime,
                    roots,
                    ITEM,
                    Value::Bool(self.operation == NativeDefaultMethod::Any),
                )?;
                self.phase = Phase::DecisionMove;
            }
            Phase::DecisionMove => {
                let value = self.get(
                    roots,
                    if self.operation == NativeDefaultMethod::Find {
                        NEXT
                    } else {
                        ITEM
                    },
                )?;
                self.set(runtime, roots, RESULT, value)?;
                self.phase = Phase::ExitJump;
            }
            Phase::ExitJump => return self.exit(runtime, owner, contract, roots),
            Phase::One => self.phase = Phase::Add,
            Phase::Add => {
                let Value::U64(value) = self.get(roots, RESULT)? else {
                    return Err(invalid());
                };
                let value = value.checked_add(1).ok_or_else(|| {
                    RuntimeError::new(RuntimeErrorKind::ScriptTrap, "integer overflow")
                })?;
                self.set(runtime, roots, ITEM, Value::U64(value))?;
                self.phase = Phase::Move;
            }
            Phase::Move => {
                let slot = if self.operation == NativeDefaultMethod::Last {
                    NEXT
                } else {
                    ITEM
                };
                self.set(runtime, roots, RESULT, self.get(roots, slot)?)?;
                self.phase = Phase::BodyJump;
            }
            Phase::Close => {
                runtime.iter_operation(
                    owner,
                    &roots.get(0).ok_or_else(invalid)?,
                    &contract.signature.params[0],
                    IterOp::Close,
                )?;
                self.phase = Phase::End;
            }
            Phase::End => {
                self.guard.take();
                return self.complete(runtime, owner, contract, roots);
            }
            Phase::WaitingNext | Phase::WaitingCallback => return Err(invalid()),
        }
        Ok(NativeAction::Continue)
    }
    pub(super) fn receive(
        &mut self,
        runtime: &Runtime,
        roots: &RootSet,
        value: Value,
    ) -> Result<(), RuntimeError> {
        match self.phase {
            Phase::WaitingNext => {
                self.set(runtime, roots, NEXT, value)?;
                self.phase = Phase::Test;
            }
            Phase::WaitingCallback => {
                self.set(runtime, roots, ITEM, value)?;
                self.phase = match self.operation {
                    NativeDefaultMethod::Fold => Phase::Move,
                    NativeDefaultMethod::ForEach => Phase::BodyJump,
                    _ => Phase::PredicateBranch,
                };
            }
            _ => return Err(invalid()),
        }
        Ok(())
    }
}
