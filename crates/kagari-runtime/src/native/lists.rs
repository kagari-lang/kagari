//! Positional List queries with the original per-operation charge schedule.
use crate::{
    LoadedModule, Runtime, RuntimeError,
    gc::{CollectionIteration, RootSet},
    native::{
        NativeAction,
        protocols::{self, ProtocolStep},
    },
    value::{EnumTag, Value},
};
use kagari_abi::{
    native_import::{EngineNativeImport, NativeWitness},
    operations::IterOp,
    scalar::BuiltinType,
    standard::{bindings::NativeDefaultMethod, surface::StandardEnum, traits::StandardTrait},
    types::AbiType,
};
use kagari_common::identity::associated_type_id;

const ITERATOR: usize = 0;
const ITEM: usize = 1;
const RESULT: usize = 2;
const ORDER: usize = 3;

#[derive(Clone, Copy)]
enum Call {
    Iter,
    Len,
    Get,
    Compare,
}
#[derive(Clone, Copy)]
enum Phase {
    Iter,
    Begin,
    Len,
    One,
    Empty,
    EmptyBranch,
    None,
    LastIndex,
    Get,
    Move,
    ExitJump,
    Close,
    End,
    Low,
    High,
    EntryJump,
    More,
    MoreBranch,
    Distance,
    Two,
    Half,
    Middle,
    Read,
    Compare,
    Equal,
    EqualBranch,
    Less,
    LessBranch,
    AdvanceOne,
    Advance,
    MoveLow,
    MoveHigh,
    BodyJump,
    Found,
    Missing,
    Waiting(Call),
}
pub(super) struct ListInvocation {
    operation: NativeDefaultMethod,
    phase: Phase,
    scratch: usize,
    guard: Option<CollectionIteration>,
    pub(super) initial: Option<Vec<Value>>,
    length: u64,
    low: u64,
    high: u64,
    middle: u64,
    condition: bool,
}
fn invalid() -> RuntimeError {
    RuntimeError::module_validation("native List query contract mismatch")
}
impl ListInvocation {
    pub(super) fn start(
        operation: NativeDefaultMethod,
        arguments: &[Value],
    ) -> Result<Self, RuntimeError> {
        if !matches!(
            operation,
            NativeDefaultMethod::ListFirst
                | NativeDefaultMethod::ListLast
                | NativeDefaultMethod::ListBinarySearch
        ) {
            return Err(invalid());
        }
        // Entry is the old zero-index constant, already charged by the caller.
        Ok(Self {
            operation,
            phase: if operation == NativeDefaultMethod::ListFirst {
                Phase::Get
            } else {
                Phase::Iter
            },
            scratch: arguments.len(),
            guard: None,
            initial: Some(vec![Value::Unit; 4]),
            length: 0,
            low: 0,
            high: 0,
            middle: 0,
            condition: false,
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
    fn witness<'a>(
        &self,
        contract: &'a EngineNativeImport,
        protocol: StandardTrait,
    ) -> Result<&'a NativeWitness, RuntimeError> {
        contract
            .witnesses
            .iter()
            .find(|witness| {
                StandardTrait::from_id(&witness.interface.declaration) == Some(protocol)
            })
            .ok_or_else(invalid)
    }
    fn iterator_type(&self, contract: &EngineNativeImport) -> Result<AbiType, RuntimeError> {
        let witness = self.witness(contract, StandardTrait::Iterable)?;
        witness
            .interface
            .associated_types
            .get(&associated_type_id(&witness.interface.declaration, "Iter"))
            .cloned()
            .ok_or_else(invalid)
    }
    fn optional(&self, contract: &EngineNativeImport) -> Result<AbiType, RuntimeError> {
        let witness = self.witness(contract, StandardTrait::List)?;
        let [item] = witness.interface.arguments.as_slice() else {
            return Err(invalid());
        };
        Ok(AbiType::StandardEnum {
            kind: StandardEnum::Option,
            args: vec![item.clone()],
        })
    }
    fn complete(
        &self,
        runtime: &Runtime,
        owner: &LoadedModule,
        contract: &EngineNativeImport,
        roots: &RootSet,
    ) -> Result<NativeAction, RuntimeError> {
        let result = self.get(roots, RESULT)?;
        if !runtime.matches_interface_method_abi(&result, &contract.signature.result, owner) {
            return Err(invalid());
        }
        Ok(NativeAction::Complete(result))
    }
    fn accept(
        &mut self,
        runtime: &Runtime,
        owner: &LoadedModule,
        contract: &EngineNativeImport,
        roots: &RootSet,
        call: Call,
        value: Value,
    ) -> Result<NativeAction, RuntimeError> {
        let output = match call {
            Call::Iter => self.iterator_type(contract)?,
            Call::Len => AbiType::Builtin(BuiltinType::USize),
            Call::Get => self.optional(contract)?,
            Call::Compare => AbiType::StandardEnum {
                kind: StandardEnum::Ordering,
                args: vec![],
            },
        };
        if !runtime.matches_interface_method_abi(&value, &output, owner) {
            return Err(invalid());
        }
        match call {
            Call::Iter => {
                self.set(runtime, roots, ITERATOR, value)?;
                self.phase = Phase::Begin;
            }
            Call::Len => {
                let Value::U64(length) = value else {
                    return Err(invalid());
                };
                self.length = length;
                self.phase = Phase::One;
            }
            Call::Get => {
                if self.operation == NativeDefaultMethod::ListBinarySearch {
                    self.set(runtime, roots, ITEM, value)?;
                    self.phase = Phase::Read;
                } else {
                    self.set(runtime, roots, RESULT, value)?;
                    if self.operation == NativeDefaultMethod::ListFirst {
                        return self.complete(runtime, owner, contract, roots);
                    }
                    self.phase = Phase::Move;
                }
            }
            Call::Compare => {
                self.set(runtime, roots, ORDER, value)?;
                self.phase = Phase::Equal;
            }
        }
        Ok(NativeAction::Continue)
    }
    fn request(
        &mut self,
        runtime: &Runtime,
        owner: &LoadedModule,
        contract: &EngineNativeImport,
        roots: &RootSet,
        call: Call,
        step: ProtocolStep,
    ) -> Result<NativeAction, RuntimeError> {
        match step {
            ProtocolStep::Value(value) => self.accept(runtime, owner, contract, roots, call, value),
            ProtocolStep::Call(request) => {
                self.phase = Phase::Waiting(call);
                Ok(NativeAction::Callback(request))
            }
            ProtocolStep::BuiltinFailure(error) => Ok(NativeAction::BuiltinFailure(error)),
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
        let Phase::Waiting(call) = self.phase else {
            return Err(invalid());
        };
        self.accept(runtime, owner, contract, roots, call, value)
    }
    pub(super) fn advance(
        &mut self,
        runtime: &Runtime,
        owner: &LoadedModule,
        contract: &EngineNativeImport,
        roots: &RootSet,
    ) -> Result<NativeAction, RuntimeError> {
        match self.phase {
            Phase::Iter => {
                let step = protocols::iter(
                    runtime,
                    owner,
                    self.witness(contract, StandardTrait::Iterable)?,
                    roots.get(0).ok_or_else(invalid)?,
                    &self.iterator_type(contract)?,
                )?;
                return self.request(runtime, owner, contract, roots, Call::Iter, step);
            }
            Phase::Begin => {
                self.guard = Some(
                    runtime
                        .gc()
                        .begin_collection_iteration(&self.get(roots, ITERATOR)?)?,
                );
                self.phase = Phase::Len;
            }
            Phase::Len => {
                let step = protocols::list(
                    runtime,
                    owner,
                    self.witness(contract, StandardTrait::List)?,
                    0,
                    vec![roots.get(0).ok_or_else(invalid)?],
                    &AbiType::Builtin(BuiltinType::USize),
                )?;
                return self.request(runtime, owner, contract, roots, Call::Len, step);
            }
            Phase::One => {
                self.phase = if self.operation == NativeDefaultMethod::ListLast {
                    Phase::Empty
                } else {
                    Phase::Low
                }
            }
            Phase::Empty => {
                self.condition = self.length == 0;
                self.phase = Phase::EmptyBranch;
            }
            Phase::EmptyBranch => {
                self.phase = if self.condition {
                    Phase::None
                } else {
                    Phase::LastIndex
                }
            }
            Phase::None => {
                self.set(
                    runtime,
                    roots,
                    RESULT,
                    Value::Enum(runtime.alloc_enum(EnumTag::OptionNone, vec![])?),
                )?;
                self.phase = Phase::Move;
            }
            Phase::LastIndex => {
                self.middle = self.length.checked_sub(1).ok_or_else(invalid)?;
                self.phase = Phase::Get;
            }
            Phase::Get => {
                let step = protocols::list(
                    runtime,
                    owner,
                    self.witness(contract, StandardTrait::List)?,
                    2,
                    vec![roots.get(0).ok_or_else(invalid)?, Value::U64(self.middle)],
                    &self.optional(contract)?,
                )?;
                return self.request(runtime, owner, contract, roots, Call::Get, step);
            }
            Phase::Move => self.phase = Phase::ExitJump,
            Phase::ExitJump => self.phase = Phase::Close,
            Phase::Close => {
                runtime.iter_operation(
                    owner,
                    &self.get(roots, ITERATOR)?,
                    &self.iterator_type(contract)?,
                    IterOp::Close,
                )?;
                self.phase = Phase::End;
            }
            Phase::End => {
                self.guard.take();
                return self.complete(runtime, owner, contract, roots);
            }
            Phase::Low => {
                self.low = 0;
                self.phase = Phase::High;
            }
            Phase::High => {
                self.high = self.length;
                self.phase = Phase::EntryJump;
            }
            Phase::EntryJump | Phase::BodyJump => self.phase = Phase::More,
            Phase::More => {
                self.condition = self.low < self.high;
                self.phase = Phase::MoreBranch;
            }
            Phase::MoreBranch => {
                self.phase = if self.condition {
                    Phase::Distance
                } else {
                    Phase::Missing
                }
            }
            Phase::Distance => {
                self.middle = self.high.checked_sub(self.low).ok_or_else(invalid)?;
                self.phase = Phase::Two;
            }
            Phase::Two => self.phase = Phase::Half,
            Phase::Half => {
                self.middle /= 2;
                self.phase = Phase::Middle;
            }
            Phase::Middle => {
                self.middle = self.low.checked_add(self.middle).ok_or_else(invalid)?;
                self.phase = Phase::Get;
            }
            Phase::Read => {
                let Value::Enum(id) = self.get(roots, ITEM)? else {
                    return Err(invalid());
                };
                let snapshot = runtime.gc().enum_snapshot(id).ok_or_else(invalid)?;
                if snapshot.tag != EnumTag::OptionSome {
                    return Ok(NativeAction::TypeMismatch("standard enum payload variant"));
                }
                self.set(
                    runtime,
                    roots,
                    ITEM,
                    snapshot.fields.into_iter().next().ok_or_else(invalid)?,
                )?;
                self.phase = Phase::Compare;
            }
            Phase::Compare => {
                let step = protocols::compare(
                    runtime,
                    owner,
                    self.witness(contract, StandardTrait::Ord)?,
                    self.get(roots, ITEM)?,
                    roots.get(1).ok_or_else(invalid)?,
                )?;
                return self.request(runtime, owner, contract, roots, Call::Compare, step);
            }
            Phase::Equal | Phase::Less => {
                let Value::Enum(id) = self.get(roots, ORDER)? else {
                    return Err(invalid());
                };
                let snapshot = runtime.gc().enum_snapshot(id).ok_or_else(invalid)?;
                self.condition = snapshot.tag
                    == if matches!(self.phase, Phase::Equal) {
                        EnumTag::OrderingEqual
                    } else {
                        EnumTag::OrderingLess
                    };
                self.phase = if matches!(self.phase, Phase::Equal) {
                    Phase::EqualBranch
                } else {
                    Phase::LessBranch
                };
            }
            Phase::EqualBranch => {
                self.phase = if self.condition {
                    Phase::Found
                } else {
                    Phase::Less
                }
            }
            Phase::LessBranch => {
                self.phase = if self.condition {
                    Phase::AdvanceOne
                } else {
                    Phase::MoveHigh
                }
            }
            Phase::AdvanceOne => self.phase = Phase::Advance,
            Phase::Advance => {
                self.middle = self.middle.checked_add(1).ok_or_else(invalid)?;
                self.phase = Phase::MoveLow;
            }
            Phase::MoveLow => {
                self.low = self.middle;
                self.phase = Phase::BodyJump;
            }
            Phase::MoveHigh => {
                self.high = self.middle;
                self.phase = Phase::BodyJump;
            }
            Phase::Found | Phase::Missing => {
                let (tag, index) = if matches!(self.phase, Phase::Found) {
                    (EnumTag::ResultOk, self.middle)
                } else {
                    (EnumTag::ResultErr, self.low)
                };
                self.set(
                    runtime,
                    roots,
                    RESULT,
                    Value::Enum(runtime.alloc_enum(tag, vec![Value::U64(index)])?),
                )?;
                self.phase = Phase::Move;
            }
            Phase::Waiting(_) => return Err(invalid()),
        }
        Ok(NativeAction::Continue)
    }
}
