//! Terminal traversal policy; each phase preserves an original logical operation.
use crate::{
    LoadedModule, Runtime, RuntimeError, RuntimeErrorKind,
    gc::{CollectionIteration, RootSet},
    native::{
        NativeAction, callback,
        protocols::{self, ProtocolStep},
    },
    numeric,
    value::{EnumTag, Value},
};
use kagari_abi::{
    native_import::{EngineNativeImport, NativeWitness},
    operations::IterOp,
    representation::ValueType,
    scalar::BuiltinType,
    standard::{
        StandardIntrinsic, bindings::NativeDefaultMethod, surface::StandardEnum,
        traits::StandardTrait,
    },
    types::AbiType,
};
use kagari_bytecode::BinaryOp;
use kagari_common::identity::associated_type_id;
const RESULT: usize = 0;
const NEXT: usize = 1;
const ITEM: usize = 2;
const CALLBACK: usize = 3;
const COUNTER: usize = 4;
const PREVIOUS: usize = 5;
const KEY_STATE: usize = 6;
const CURRENT_KEY: usize = 7;

#[derive(Clone, Copy)]
enum DecisionPhase {
    CounterInit,
    ResultNone,
    CounterZero,
    CounterEquals,
    CounterWrap,
    CounterOne,
    CounterAdvance,
    CounterMove,
    MappedTest,
    MappedMove,
    MappedBranch,
    ResultTest,
    ResultBranch,
    ResultRead,
    CombinedWrap,
    CombinedMove,
    GreaterTest,
    CompareBranch,
    Replace,
    KeyNone,
    KeyRead,
    KeyWrap,
    KeyMove,
    Compare,
}

#[derive(Clone, Copy)]
enum RangeCheck {
    Message,
    Minimum,
    MinimumCompare,
    MinimumAssert,
    Maximum,
    MaximumCompare,
    MaximumAssert,
}

enum Phase {
    DecisionStep(DecisionPhase),
    Begin,
    EntryJump,
    Next,
    WaitingNext,
    Test,
    Branch,
    Read,
    Callback,
    WaitingCallback,
    KeyCallback,
    WaitingKey,
    WaitingComparison,
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
    Append,
    Join,
    Aggregate,
    RangeCheck(RangeCheck),
}

pub(super) struct IteratorInvocation {
    operation: NativeDefaultMethod,
    phase: Phase,
    scratch: usize,
    guarded: bool,
    guard: Option<CollectionIteration>,
    pub(super) initial: Option<Vec<Value>>,
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
            NativeDefaultMethod::Sum | NativeDefaultMethod::Product => {
                let AbiType::Builtin(scalar) = contract.signature.result else {
                    return Err(invalid());
                };
                let one = operation == NativeDefaultMethod::Product;
                match AbiType::Builtin(scalar).representation() {
                    ValueType::I32 => Value::I32(i32::from(one)),
                    ValueType::I64 => Value::I64(i64::from(one)),
                    ValueType::U64 => Value::U64(u64::from(one)),
                    ValueType::F32 => Value::F32(if one { 1.0 } else { 0.0 }),
                    ValueType::F64 => Value::F64(if one { 1.0 } else { 0.0 }),
                    _ => return Err(invalid()),
                }
            }
            NativeDefaultMethod::Join => runtime
                .invoke_standard_builtin(StandardIntrinsic::ArrayListNew, &[])
                .map_err(|error| error.into_runtime_error())?,
            NativeDefaultMethod::Count => Value::U64(0),
            NativeDefaultMethod::Fold => arguments[1].clone(),
            NativeDefaultMethod::ForEach => Value::Unit,
            NativeDefaultMethod::Any | NativeDefaultMethod::All => {
                Value::Bool(operation == NativeDefaultMethod::All)
            }
            NativeDefaultMethod::Position => Value::U64(0),
            NativeDefaultMethod::Nth => Value::Unit,
            NativeDefaultMethod::Find
            | NativeDefaultMethod::Last
            | NativeDefaultMethod::FindMap
            | NativeDefaultMethod::Reduce
            | NativeDefaultMethod::MinBy
            | NativeDefaultMethod::MaxBy
            | NativeDefaultMethod::Min
            | NativeDefaultMethod::Max
            | NativeDefaultMethod::MinByKey
            | NativeDefaultMethod::MaxByKey => {
                Value::Enum(runtime.alloc_enum(EnumTag::OptionNone, vec![])?)
            }
            _ => return Err(invalid()),
        };
        let guarded = matches!(contract.signature.params.first(), Some(AbiType::Iter(_)));
        Ok(Self {
            operation,
            phase: if matches!(
                operation,
                NativeDefaultMethod::MinByKey | NativeDefaultMethod::MaxByKey
            ) {
                Phase::DecisionStep(DecisionPhase::KeyNone)
            } else if operation == NativeDefaultMethod::Position {
                Phase::DecisionStep(DecisionPhase::CounterInit)
            } else if operation == NativeDefaultMethod::Nth {
                Phase::DecisionStep(DecisionPhase::ResultNone)
            } else if guarded {
                Phase::Begin
            } else {
                Phase::EntryJump
            },
            scratch: arguments.len(),
            guarded,
            guard: None,
            initial: Some(vec![
                initial,
                Value::Unit,
                Value::Unit,
                Value::Unit,
                if operation == NativeDefaultMethod::Nth {
                    arguments[1].clone()
                } else {
                    Value::Unit
                },
                Value::Unit,
                Value::Unit,
                Value::Unit,
            ]),
            present: false,
        })
    }
    fn keyed(&self) -> bool {
        matches!(
            self.operation,
            NativeDefaultMethod::MinByKey | NativeDefaultMethod::MaxByKey
        )
    }
    fn replacement(&self) -> Phase {
        Phase::DecisionStep(if self.keyed() {
            DecisionPhase::KeyWrap
        } else {
            DecisionPhase::Replace
        })
    }
    fn ordinal_witness<'a>(
        &self,
        contract: &'a EngineNativeImport,
    ) -> Result<&'a NativeWitness, RuntimeError> {
        let receiver = if self.keyed() {
            let Some(AbiType::Function { result, .. }) = contract.signature.params.get(1) else {
                return Err(invalid());
            };
            result.as_ref().clone()
        } else {
            let AbiType::StandardEnum { args, .. } = self.optional(contract)? else {
                return Err(invalid());
            };
            args.into_iter().next().ok_or_else(invalid)?
        };
        contract
            .witnesses
            .iter()
            .find(|witness| {
                witness.receiver == receiver
                    && StandardTrait::from_id(&witness.interface.declaration)
                        == Some(StandardTrait::Ord)
            })
            .ok_or_else(invalid)
    }
    fn is_some(&self, runtime: &Runtime, value: &Value) -> Result<bool, RuntimeError> {
        let Value::Enum(id) = value else {
            return Err(invalid());
        };
        let snapshot = runtime.gc().enum_snapshot(*id).ok_or_else(invalid)?;
        match (snapshot.tag, snapshot.fields.len()) {
            (EnumTag::OptionSome, 1) => Ok(true),
            (EnumTag::OptionNone, 0) => Ok(false),
            _ => Err(invalid()),
        }
    }
    fn read_some(&self, runtime: &Runtime, value: &Value) -> Result<Value, RuntimeError> {
        let Value::Enum(id) = value else {
            return Err(invalid());
        };
        let snapshot = runtime.gc().enum_snapshot(*id).ok_or_else(invalid)?;
        if snapshot.tag != EnumTag::OptionSome || snapshot.fields.len() != 1 {
            return Err(invalid());
        }
        snapshot.fields.into_iter().next().ok_or_else(invalid)
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
        } else if self.operation == NativeDefaultMethod::Join {
            self.phase = Phase::Join;
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
    fn advance_decision(
        &mut self,
        phase: DecisionPhase,
        runtime: &Runtime,
        owner: &LoadedModule,
        contract: &EngineNativeImport,
        roots: &RootSet,
    ) -> Result<NativeAction, RuntimeError> {
        match phase {
            DecisionPhase::CounterInit => {
                self.set(runtime, roots, COUNTER, self.get(roots, RESULT)?)?;
                self.phase = Phase::DecisionStep(DecisionPhase::ResultNone);
            }
            DecisionPhase::ResultNone => {
                self.set(
                    runtime,
                    roots,
                    RESULT,
                    Value::Enum(runtime.alloc_enum(EnumTag::OptionNone, vec![])?),
                )?;
                self.phase = if self.guarded {
                    Phase::Begin
                } else {
                    Phase::EntryJump
                };
            }
            DecisionPhase::CounterZero => {
                self.phase = Phase::DecisionStep(DecisionPhase::CounterEquals)
            }
            DecisionPhase::CounterEquals => {
                let Value::U64(counter) = self.get(roots, COUNTER)? else {
                    return Err(invalid());
                };
                self.set(runtime, roots, CALLBACK, Value::Bool(counter == 0))?;
                self.phase = Phase::PredicateBranch;
            }
            DecisionPhase::CounterWrap => {
                let value = Value::Enum(
                    runtime.alloc_enum(EnumTag::OptionSome, vec![self.get(roots, COUNTER)?])?,
                );
                self.set(runtime, roots, CALLBACK, value)?;
                self.phase = Phase::DecisionMove;
            }
            DecisionPhase::CounterOne => {
                self.phase = Phase::DecisionStep(DecisionPhase::CounterAdvance)
            }
            DecisionPhase::CounterAdvance => {
                let Value::U64(counter) = self.get(roots, COUNTER)? else {
                    return Err(invalid());
                };
                let next = if self.operation == NativeDefaultMethod::Position {
                    counter.checked_add(1)
                } else {
                    counter.checked_sub(1)
                }
                .ok_or_else(|| {
                    RuntimeError::new(RuntimeErrorKind::ScriptTrap, "integer overflow")
                })?;
                self.set(runtime, roots, CALLBACK, Value::U64(next))?;
                self.phase = Phase::DecisionStep(DecisionPhase::CounterMove);
            }
            DecisionPhase::CounterMove => {
                self.set(runtime, roots, COUNTER, self.get(roots, CALLBACK)?)?;
                self.phase = Phase::BodyJump;
            }
            DecisionPhase::MappedTest => {
                self.present = self.is_some(runtime, &self.get(roots, CALLBACK)?)?;
                self.phase = Phase::DecisionStep(DecisionPhase::MappedMove);
            }
            DecisionPhase::MappedMove => {
                self.set(runtime, roots, RESULT, self.get(roots, CALLBACK)?)?;
                self.phase = Phase::DecisionStep(DecisionPhase::MappedBranch);
            }
            DecisionPhase::MappedBranch => {
                if self.present {
                    return self.exit(runtime, owner, contract, roots);
                }
                self.phase = Phase::Next;
            }
            DecisionPhase::ResultTest => {
                self.present = self.is_some(runtime, &self.get(roots, RESULT)?)?;
                self.phase = Phase::DecisionStep(DecisionPhase::ResultBranch);
            }
            DecisionPhase::ResultBranch => {
                self.phase = if self.present {
                    Phase::DecisionStep(DecisionPhase::ResultRead)
                } else {
                    self.replacement()
                }
            }
            DecisionPhase::ResultRead => {
                let value = self.read_some(runtime, &self.get(roots, RESULT)?)?;
                self.set(runtime, roots, PREVIOUS, value)?;
                self.phase = if self.keyed() {
                    Phase::DecisionStep(DecisionPhase::KeyRead)
                } else if matches!(
                    self.operation,
                    NativeDefaultMethod::Min | NativeDefaultMethod::Max
                ) {
                    Phase::DecisionStep(DecisionPhase::Compare)
                } else {
                    Phase::Callback
                };
            }
            DecisionPhase::CombinedWrap => {
                let value = Value::Enum(
                    runtime.alloc_enum(EnumTag::OptionSome, vec![self.get(roots, CALLBACK)?])?,
                );
                self.set(runtime, roots, CALLBACK, value)?;
                self.phase = Phase::DecisionStep(DecisionPhase::CombinedMove);
            }
            DecisionPhase::CombinedMove => {
                self.set(runtime, roots, RESULT, self.get(roots, CALLBACK)?)?;
                self.phase = Phase::BodyJump;
            }
            DecisionPhase::GreaterTest => {
                let Value::Enum(id) = self.get(roots, CALLBACK)? else {
                    return Err(invalid());
                };
                let snapshot = runtime.gc().enum_snapshot(id).ok_or_else(invalid)?;
                if !snapshot.fields.is_empty()
                    || !matches!(
                        snapshot.tag,
                        EnumTag::OrderingLess | EnumTag::OrderingEqual | EnumTag::OrderingGreater
                    )
                {
                    return Err(invalid());
                }
                self.present = snapshot.tag == EnumTag::OrderingGreater;
                self.phase = Phase::DecisionStep(DecisionPhase::CompareBranch);
            }
            DecisionPhase::CompareBranch => {
                let replace = if matches!(
                    self.operation,
                    NativeDefaultMethod::MinBy
                        | NativeDefaultMethod::Min
                        | NativeDefaultMethod::MinByKey
                ) {
                    self.present
                } else {
                    !self.present
                };
                self.phase = if replace {
                    self.replacement()
                } else {
                    Phase::Next
                };
            }
            DecisionPhase::KeyNone => {
                self.set(
                    runtime,
                    roots,
                    KEY_STATE,
                    Value::Enum(runtime.alloc_enum(EnumTag::OptionNone, vec![])?),
                )?;
                self.phase = if self.guarded {
                    Phase::Begin
                } else {
                    Phase::EntryJump
                };
            }
            DecisionPhase::KeyRead => {
                let key = self.read_some(runtime, &self.get(roots, KEY_STATE)?)?;
                self.set(runtime, roots, PREVIOUS, key)?;
                self.phase = Phase::DecisionStep(DecisionPhase::Compare);
            }
            DecisionPhase::KeyWrap => {
                let value = Value::Enum(
                    runtime.alloc_enum(EnumTag::OptionSome, vec![self.get(roots, CURRENT_KEY)?])?,
                );
                self.set(runtime, roots, CALLBACK, value)?;
                self.phase = Phase::DecisionStep(DecisionPhase::KeyMove);
            }
            DecisionPhase::KeyMove => {
                self.set(runtime, roots, KEY_STATE, self.get(roots, CALLBACK)?)?;
                self.phase = Phase::DecisionStep(DecisionPhase::Replace);
            }
            DecisionPhase::Compare => {
                let step = protocols::compare(
                    runtime,
                    owner,
                    self.ordinal_witness(contract)?,
                    self.get(roots, PREVIOUS)?,
                    self.get(roots, if self.keyed() { CURRENT_KEY } else { ITEM })?,
                )?;
                match step {
                    ProtocolStep::Value(value) => {
                        self.set(runtime, roots, CALLBACK, value)?;
                        self.phase = Phase::DecisionStep(DecisionPhase::GreaterTest);
                    }
                    ProtocolStep::Call(request) => {
                        self.phase = Phase::WaitingComparison;
                        return Ok(NativeAction::Callback(request));
                    }
                }
            }
            DecisionPhase::Replace => {
                self.set(runtime, roots, RESULT, self.get(roots, NEXT)?)?;
                self.phase = Phase::BodyJump;
            }
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
            Phase::DecisionStep(phase) => {
                return self.advance_decision(phase, runtime, owner, contract, roots);
            }
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
                self.present = self.is_some(runtime, &self.get(roots, NEXT)?)?;
                if !runtime.matches_interface_method_abi(
                    &self.get(roots, NEXT)?,
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
                let item = self.read_some(runtime, &self.get(roots, NEXT)?)?;
                self.set(runtime, roots, ITEM, item)?;
                self.phase = match self.operation {
                    NativeDefaultMethod::Count => Phase::One,
                    NativeDefaultMethod::Last => Phase::Move,
                    NativeDefaultMethod::Join => Phase::Append,
                    NativeDefaultMethod::Sum | NativeDefaultMethod::Product => Phase::Aggregate,
                    NativeDefaultMethod::Nth => Phase::DecisionStep(DecisionPhase::CounterZero),
                    NativeDefaultMethod::Reduce
                    | NativeDefaultMethod::MinBy
                    | NativeDefaultMethod::MaxBy
                    | NativeDefaultMethod::Min
                    | NativeDefaultMethod::Max => Phase::DecisionStep(DecisionPhase::ResultTest),
                    NativeDefaultMethod::MinByKey | NativeDefaultMethod::MaxByKey => {
                        Phase::KeyCallback
                    }
                    _ => Phase::Callback,
                };
            }
            Phase::KeyCallback => {
                let request = callback(
                    runtime,
                    &roots.get(1).ok_or_else(invalid)?,
                    &contract.signature.params[1],
                    vec![self.get(roots, ITEM)?],
                )?;
                self.phase = Phase::WaitingKey;
                return Ok(NativeAction::Callback(request));
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
                if matches!(
                    self.operation,
                    NativeDefaultMethod::Reduce
                        | NativeDefaultMethod::MinBy
                        | NativeDefaultMethod::MaxBy
                ) {
                    arguments.push(self.get(roots, PREVIOUS)?);
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
                let Value::Bool(matched) = self.get(roots, CALLBACK)? else {
                    return Err(invalid());
                };
                let found = if self.operation == NativeDefaultMethod::All {
                    !matched
                } else {
                    matched
                };
                self.phase = if found {
                    if self.operation == NativeDefaultMethod::Position {
                        Phase::DecisionStep(DecisionPhase::CounterWrap)
                    } else if matches!(
                        self.operation,
                        NativeDefaultMethod::Find | NativeDefaultMethod::Nth
                    ) {
                        Phase::DecisionMove
                    } else {
                        Phase::Decision
                    }
                } else if matches!(
                    self.operation,
                    NativeDefaultMethod::Position | NativeDefaultMethod::Nth
                ) {
                    Phase::DecisionStep(DecisionPhase::CounterOne)
                } else {
                    Phase::Next
                };
            }
            Phase::Decision => {
                self.set(
                    runtime,
                    roots,
                    CALLBACK,
                    Value::Bool(self.operation == NativeDefaultMethod::Any),
                )?;
                self.phase = Phase::DecisionMove;
            }
            Phase::DecisionMove => {
                let value = self.get(
                    roots,
                    if matches!(
                        self.operation,
                        NativeDefaultMethod::Find | NativeDefaultMethod::Nth
                    ) {
                        NEXT
                    } else {
                        CALLBACK
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
                } else if matches!(
                    self.operation,
                    NativeDefaultMethod::Fold
                        | NativeDefaultMethod::Sum
                        | NativeDefaultMethod::Product
                ) {
                    CALLBACK
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
                if self.operation == NativeDefaultMethod::Join {
                    self.phase = Phase::Join;
                    return Ok(NativeAction::Continue);
                }
                return self.complete(runtime, owner, contract, roots);
            }
            Phase::Append => {
                runtime
                    .invoke_standard_builtin(
                        StandardIntrinsic::ArrayPush,
                        &[self.get(roots, RESULT)?, self.get(roots, ITEM)?],
                    )
                    .map_err(|error| error.into_runtime_error())?;
                self.phase = Phase::BodyJump;
            }
            Phase::Aggregate => {
                let value = numeric::binary(
                    if self.operation == NativeDefaultMethod::Sum {
                        BinaryOp::Add
                    } else {
                        BinaryOp::Mul
                    },
                    self.get(roots, RESULT)?,
                    self.get(roots, ITEM)?,
                )?;
                self.set(runtime, roots, CALLBACK, value)?;
                self.phase = if self.integer_bounds(contract).is_some() {
                    Phase::RangeCheck(RangeCheck::Message)
                } else {
                    Phase::Move
                };
            }
            Phase::RangeCheck(step) => {
                let (minimum, maximum) = self.integer_bounds(contract).ok_or_else(invalid)?;
                self.phase = match step {
                    RangeCheck::Message => Phase::RangeCheck(RangeCheck::Minimum),
                    RangeCheck::Minimum => Phase::RangeCheck(RangeCheck::MinimumCompare),
                    RangeCheck::Maximum => Phase::RangeCheck(RangeCheck::MaximumCompare),
                    RangeCheck::MinimumCompare | RangeCheck::MaximumCompare => {
                        let value = match self.get(roots, CALLBACK)? {
                            Value::I32(value) => i64::from(value),
                            Value::I64(value) => value,
                            _ => return Err(invalid()),
                        };
                        self.present = match step {
                            RangeCheck::MinimumCompare => value >= minimum,
                            _ => value <= maximum,
                        };
                        Phase::RangeCheck(match step {
                            RangeCheck::MinimumCompare => RangeCheck::MinimumAssert,
                            _ => RangeCheck::MaximumAssert,
                        })
                    }
                    RangeCheck::MinimumAssert | RangeCheck::MaximumAssert => {
                        if let Err(error) = runtime.invoke_standard_builtin(
                            StandardIntrinsic::DebugAssert,
                            &[
                                Value::Bool(self.present),
                                Value::Str("integer overflow".into()),
                            ],
                        ) {
                            return Ok(NativeAction::BuiltinFailure(error));
                        }
                        match step {
                            RangeCheck::MinimumAssert => Phase::RangeCheck(RangeCheck::Maximum),
                            _ => Phase::Move,
                        }
                    }
                };
            }
            Phase::Join => {
                let value = runtime
                    .invoke_standard_builtin(
                        StandardIntrinsic::ArrayJoin,
                        &[self.get(roots, RESULT)?, roots.get(1).ok_or_else(invalid)?],
                    )
                    .map_err(|error| error.into_runtime_error())?;
                self.set(runtime, roots, RESULT, value)?;
                return self.complete(runtime, owner, contract, roots);
            }
            Phase::WaitingNext
            | Phase::WaitingCallback
            | Phase::WaitingKey
            | Phase::WaitingComparison => return Err(invalid()),
        }
        Ok(NativeAction::Continue)
    }
    fn integer_bounds(&self, contract: &EngineNativeImport) -> Option<(i64, i64)> {
        match contract.signature.result {
            AbiType::Builtin(BuiltinType::I8) => Some((i64::from(i8::MIN), i64::from(i8::MAX))),
            AbiType::Builtin(BuiltinType::I16) => Some((i64::from(i16::MIN), i64::from(i16::MAX))),
            AbiType::Builtin(BuiltinType::U8) => Some((0, i64::from(u8::MAX))),
            AbiType::Builtin(BuiltinType::U16) => Some((0, i64::from(u16::MAX))),
            AbiType::Builtin(BuiltinType::U32) => Some((0, i64::from(u32::MAX))),
            _ => None,
        }
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
            Phase::WaitingKey => {
                self.set(runtime, roots, CURRENT_KEY, value)?;
                self.phase = Phase::DecisionStep(DecisionPhase::ResultTest);
            }
            Phase::WaitingComparison => {
                self.set(runtime, roots, CALLBACK, value)?;
                self.phase = Phase::DecisionStep(DecisionPhase::GreaterTest);
            }
            Phase::WaitingCallback => {
                self.set(runtime, roots, CALLBACK, value)?;
                self.phase = match self.operation {
                    NativeDefaultMethod::FindMap => Phase::DecisionStep(DecisionPhase::MappedTest),
                    NativeDefaultMethod::Reduce => Phase::DecisionStep(DecisionPhase::CombinedWrap),
                    NativeDefaultMethod::MinBy | NativeDefaultMethod::MaxBy => {
                        Phase::DecisionStep(DecisionPhase::GreaterTest)
                    }
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
