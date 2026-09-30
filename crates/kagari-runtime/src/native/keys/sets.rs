//! Guard both Set sources, use selected membership, and publish ordered native results.
use super::{
    Buffers, invalid,
    lookup::{KeyStep, Lookup},
    witness,
};
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
    native_import::{EngineNativeImport, NativeWitness, NativeWitnessImplementation},
    operations::IterOp,
    scalar::BuiltinType,
    standard::{
        StandardIntrinsic, bindings::NativeDefaultMethod, surface::StandardEnum,
        traits::StandardTrait,
    },
    types::AbiType,
};
use kagari_common::identity::associated_type_id;
const LEFT: usize = 0;
const RIGHT: usize = 1;
const NEXT: usize = 2;
const ITEM: usize = 3;
const RESULT: usize = 4;
const LOOKUP: usize = 5;
pub(super) const SCRATCH_ROOTS: usize = LOOKUP + super::SCRATCH_ROOTS;
#[derive(Clone, Copy)]
enum Call {
    LeftIter,
    RightIter,
    Next,
    Contains,
}
#[derive(Clone, Copy)]
enum Phase {
    LeftBegin,
    RightIter,
    RightBegin,
    Result,
    Jump,
    Next,
    Test,
    Branch,
    Read,
    UnionJump,
    Contains,
    ContainsBranch,
    Insert,
    False,
    Move,
    ExitJump,
    RightClose,
    RightEnd,
    LeftClose,
    LeftEnd,
    Waiting(Call),
}
pub(super) struct SetQuery {
    operation: NativeDefaultMethod,
    scratch: usize,
    phase: Phase,
    guards: [Option<CollectionIteration>; 2],
    pass: usize,
    condition: bool,
    relation: bool,
    result: bool,
    lookup: Option<Lookup>,
}
impl SetQuery {
    pub(super) fn start(operation: NativeDefaultMethod, arguments: &[Value]) -> Self {
        Self {
            operation,
            scratch: arguments.len(),
            phase: Phase::Waiting(Call::LeftIter),
            guards: [None, None],
            pass: 0,
            condition: false,
            relation: matches!(
                operation,
                NativeDefaultMethod::SetIsSubset
                    | NativeDefaultMethod::SetIsSuperset
                    | NativeDefaultMethod::SetIsDisjoint
            ),
            result: true,
            lookup: None,
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
    fn selected<'a>(
        &self,
        contract: &'a EngineNativeImport,
        source: usize,
        protocol: StandardTrait,
    ) -> Result<&'a NativeWitness, RuntimeError> {
        contract
            .witnesses
            .iter()
            .find(|w| {
                w.receiver == contract.signature.params[source]
                    && StandardTrait::from_id(&w.interface.declaration) == Some(protocol)
            })
            .ok_or_else(invalid)
    }
    fn iterator_type(
        &self,
        contract: &EngineNativeImport,
        source: usize,
    ) -> Result<AbiType, RuntimeError> {
        let iterable = self.selected(contract, source, StandardTrait::Iterable)?;
        iterable
            .interface
            .associated_types
            .get(&associated_type_id(&iterable.interface.declaration, "Iter"))
            .cloned()
            .ok_or_else(invalid)
    }
    fn next_witness<'a>(
        &self,
        contract: &'a EngineNativeImport,
    ) -> Result<&'a NativeWitness, RuntimeError> {
        let ty = self.iterator_type(contract, 0)?;
        contract
            .witnesses
            .iter()
            .find(|w| {
                w.receiver == ty
                    && StandardTrait::from_id(&w.interface.declaration)
                        == Some(StandardTrait::Iterator)
            })
            .ok_or_else(invalid)
    }
    fn optional(&self, contract: &EngineNativeImport) -> Result<AbiType, RuntimeError> {
        let selected = self.selected(contract, 0, StandardTrait::Set)?;
        let [item] = selected.interface.arguments.as_slice() else {
            return Err(invalid());
        };
        Ok(AbiType::StandardEnum {
            kind: StandardEnum::Option,
            args: vec![item.clone()],
        })
    }
    fn traversed(&self) -> usize {
        usize::from(self.pass == 1 || self.operation == NativeDefaultMethod::SetIsSuperset)
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
        self.phase = Phase::Waiting(call);
        match step {
            ProtocolStep::Value(value) => self.receive(runtime, owner, contract, roots, value),
            ProtocolStep::Call(request) => Ok(NativeAction::Callback(request)),
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
        let step = protocols::iter(
            runtime,
            owner,
            self.selected(contract, 0, StandardTrait::Iterable)?,
            roots.get(0).ok_or_else(invalid)?,
            &self.iterator_type(contract, 0)?,
        )?;
        self.request(runtime, owner, contract, roots, Call::LeftIter, step)
    }
    pub(super) fn receive(
        &mut self,
        runtime: &Runtime,
        owner: &LoadedModule,
        contract: &EngineNativeImport,
        roots: &RootSet,
        value: Value,
    ) -> Result<NativeAction, RuntimeError> {
        if matches!(self.phase, Phase::Insert | Phase::Contains)
            && let Some(lookup) = &mut self.lookup
        {
            return lookup.receive(runtime, owner, value);
        }
        let Phase::Waiting(call) = self.phase else {
            return Err(invalid());
        };
        let expected = match call {
            Call::LeftIter => self.iterator_type(contract, 0)?,
            Call::RightIter => self.iterator_type(contract, 1)?,
            Call::Next => self.optional(contract)?,
            Call::Contains => AbiType::Builtin(BuiltinType::Bool),
        };
        if !runtime.matches_interface_method_abi(&value, &expected, owner) {
            return Err(invalid());
        }
        match call {
            Call::LeftIter => {
                self.set(runtime, roots, LEFT, value)?;
                self.phase = Phase::LeftBegin;
            }
            Call::RightIter => {
                self.set(runtime, roots, RIGHT, value)?;
                self.phase = Phase::RightBegin;
            }
            Call::Next => {
                self.set(runtime, roots, NEXT, value)?;
                self.phase = Phase::Test;
            }
            Call::Contains => {
                let Value::Bool(condition) = value else {
                    return Err(invalid());
                };
                self.condition = condition;
                self.phase = Phase::ContainsBranch;
            }
        }
        Ok(NativeAction::Continue)
    }
    fn key_lookup(
        &self,
        contract: &EngineNativeImport,
        receiver: usize,
        operation: StandardIntrinsic,
    ) -> Result<Lookup, RuntimeError> {
        let equality = witness(contract, StandardTrait::PartialEq)?;
        let custom = matches!(
            equality.implementation,
            NativeWitnessImplementation::Derived | NativeWitnessImplementation::Table(_)
        );
        Ok(Lookup::start(
            operation,
            custom,
            Buffers {
                scratch: self.scratch + LOOKUP,
                receiver,
                query: self.scratch + ITEM,
            },
            None,
        ))
    }
    fn finish_pass(&mut self) {
        if self.pass == 0
            && matches!(
                self.operation,
                NativeDefaultMethod::SetUnion | NativeDefaultMethod::SetSymmetricDifference
            )
        {
            self.pass = 1;
            self.phase = Phase::Jump;
        } else {
            self.phase = Phase::RightClose;
        }
    }
    pub(super) fn advance(
        &mut self,
        runtime: &Runtime,
        owner: &LoadedModule,
        contract: &EngineNativeImport,
        roots: &RootSet,
    ) -> Result<NativeAction, RuntimeError> {
        match self.phase {
            Phase::LeftBegin => {
                self.guards[0] = Some(
                    runtime
                        .gc()
                        .begin_collection_iteration(&self.get(roots, LEFT)?)?,
                );
                self.phase = Phase::RightIter;
            }
            Phase::RightIter => {
                let step = protocols::iter(
                    runtime,
                    owner,
                    self.selected(contract, 1, StandardTrait::Iterable)?,
                    roots.get(1).ok_or_else(invalid)?,
                    &self.iterator_type(contract, 1)?,
                )?;
                return self.request(runtime, owner, contract, roots, Call::RightIter, step);
            }
            Phase::RightBegin => {
                self.guards[1] = Some(
                    runtime
                        .gc()
                        .begin_collection_iteration(&self.get(roots, RIGHT)?)?,
                );
                self.phase = Phase::Result;
            }
            Phase::Result => {
                if !self.relation {
                    match runtime.invoke_standard_builtin(StandardIntrinsic::LinkedHashSetNew, &[])
                    {
                        Ok(value) => self.set(runtime, roots, RESULT, value)?,
                        Err(error) => return Ok(NativeAction::BuiltinFailure(error)),
                    }
                }
                self.phase = Phase::Jump;
            }
            Phase::Jump => self.phase = Phase::Next,
            Phase::Next => {
                let step = protocols::next(
                    runtime,
                    owner,
                    self.next_witness(contract)?,
                    self.get(roots, self.traversed())?,
                    &self.optional(contract)?,
                )?;
                return self.request(runtime, owner, contract, roots, Call::Next, step);
            }
            Phase::Test => {
                let Value::Enum(id) = self.get(roots, NEXT)? else {
                    return Err(invalid());
                };
                self.condition =
                    runtime.gc().enum_snapshot(id).ok_or_else(invalid)?.tag == EnumTag::OptionSome;
                self.phase = Phase::Branch;
            }
            Phase::Branch => {
                if self.condition {
                    self.phase = Phase::Read;
                } else {
                    self.finish_pass();
                }
            }
            Phase::Read => {
                let Value::Enum(id) = self.get(roots, NEXT)? else {
                    return Err(invalid());
                };
                let value = runtime
                    .gc()
                    .enum_snapshot(id)
                    .ok_or_else(invalid)?
                    .fields
                    .into_iter()
                    .next()
                    .ok_or_else(invalid)?;
                self.set(runtime, roots, ITEM, value)?;
                self.phase = if self.operation == NativeDefaultMethod::SetUnion {
                    Phase::UnionJump
                } else {
                    Phase::Contains
                };
            }
            Phase::UnionJump => self.phase = Phase::Insert,
            Phase::Contains => {
                let queried = 1 - self.traversed();
                if queried == 0 && matches!(contract.signature.params[0], AbiType::Set(..)) {
                    if self.lookup.is_none() {
                        self.lookup =
                            Some(self.key_lookup(contract, 0, StandardIntrinsic::SetContains)?);
                    }
                    match self
                        .lookup
                        .as_mut()
                        .ok_or_else(invalid)?
                        .advance(runtime, owner, contract, roots)?
                    {
                        KeyStep::Action(action) => return Ok(action),
                        KeyStep::Ready(value) => {
                            let Value::Bool(condition) = value else {
                                return Err(invalid());
                            };
                            self.condition = condition;
                            self.lookup = None;
                            self.phase = Phase::ContainsBranch;
                        }
                    }
                } else {
                    let step = protocols::set_contains(
                        runtime,
                        owner,
                        self.selected(contract, queried, StandardTrait::Set)?,
                        roots.get(queried).ok_or_else(invalid)?,
                        self.get(roots, ITEM)?,
                    )?;
                    return self.request(runtime, owner, contract, roots, Call::Contains, step);
                }
            }
            Phase::ContainsBranch => {
                let positive = matches!(
                    self.operation,
                    NativeDefaultMethod::SetIntersection | NativeDefaultMethod::SetIsDisjoint
                );
                self.phase = if self.condition == positive {
                    if self.relation {
                        Phase::False
                    } else {
                        Phase::Insert
                    }
                } else {
                    Phase::Next
                };
            }
            Phase::Insert => {
                if self.lookup.is_none() {
                    self.lookup = Some(self.key_lookup(
                        contract,
                        self.scratch + RESULT,
                        StandardIntrinsic::SetInsert,
                    )?);
                }
                match self
                    .lookup
                    .as_mut()
                    .ok_or_else(invalid)?
                    .advance(runtime, owner, contract, roots)?
                {
                    KeyStep::Action(action) => return Ok(action),
                    KeyStep::Ready(_) => {
                        self.lookup = None;
                        self.phase = Phase::Jump;
                    }
                }
            }
            Phase::False => {
                self.result = false;
                self.phase = Phase::Move;
            }
            Phase::Move => self.phase = Phase::ExitJump,
            Phase::ExitJump => self.phase = Phase::RightClose,
            Phase::RightClose => {
                runtime.iter_operation(
                    owner,
                    &self.get(roots, RIGHT)?,
                    &self.iterator_type(contract, 1)?,
                    IterOp::Close,
                )?;
                self.phase = Phase::RightEnd;
            }
            Phase::RightEnd => {
                self.guards[1].take();
                self.phase = Phase::LeftClose;
            }
            Phase::LeftClose => {
                runtime.iter_operation(
                    owner,
                    &self.get(roots, LEFT)?,
                    &self.iterator_type(contract, 0)?,
                    IterOp::Close,
                )?;
                self.phase = Phase::LeftEnd;
            }
            Phase::LeftEnd => {
                self.guards[0].take();
                return Ok(NativeAction::Complete(if self.relation {
                    Value::Bool(self.result)
                } else {
                    self.get(roots, RESULT)?
                }));
            }
            Phase::Waiting(_) => return Err(invalid()),
        }
        Ok(NativeAction::Continue)
    }
}
