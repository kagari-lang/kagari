//! Membership and prefix/suffix matching with selected core equality callbacks.
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
const SOURCE_ITER: usize = 0;
const NEEDLE_ITER: usize = 1;
const LEFT: usize = 2;
const RIGHT: usize = 3;
#[derive(Clone, Copy)]
enum Call {
    SourceIter,
    SourceLen,
    NeedleIter,
    NeedleLen,
    Left,
    Right,
    Equal,
}
#[derive(Clone, Copy)]
enum Phase {
    SourceIter,
    SourceBegin,
    SourceLen,
    One,
    NeedleIter,
    NeedleBegin,
    NeedleLen,
    Result,
    Index,
    EntryJump,
    Longer,
    LongerBranch,
    More,
    MoreBranch,
    OffsetStart,
    OffsetAdd,
    Left,
    ReadLeft,
    Right,
    ReadRight,
    Equal,
    EqualBranch,
    Advance,
    MoveIndex,
    BodyJump,
    Found,
    Mismatch,
    MoveResult,
    ExitJump,
    NeedleClose,
    NeedleEnd,
    SourceClose,
    SourceEnd,
    Waiting(Call),
}
pub(super) struct EqualityInvocation {
    operation: NativeDefaultMethod,
    phase: Phase,
    scratch: usize,
    guards: [Option<CollectionIteration>; 2],
    pub(super) initial: Option<Vec<Value>>,
    length: u64,
    limit: u64,
    index: u64,
    offset: u64,
    condition: bool,
    result: bool,
}
fn invalid() -> RuntimeError {
    RuntimeError::module_validation("native List equality contract mismatch")
}
impl EqualityInvocation {
    pub(super) fn start(
        operation: NativeDefaultMethod,
        arguments: &[Value],
    ) -> Result<Self, RuntimeError> {
        if !matches!(
            operation,
            NativeDefaultMethod::ListContains
                | NativeDefaultMethod::ListStartsWith
                | NativeDefaultMethod::ListEndsWith
        ) {
            return Err(invalid());
        }
        // The caller's entry charge replaces the old unused zero constant.
        Ok(Self {
            operation,
            phase: Phase::SourceIter,
            scratch: arguments.len(),
            guards: [None, None],
            initial: Some(vec![Value::Unit; 4]),
            length: 0,
            limit: 0,
            index: 0,
            offset: 0,
            condition: false,
            result: false,
        })
    }
    fn sequence(&self) -> bool {
        self.operation != NativeDefaultMethod::ListContains
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
        slot: usize,
        protocol: StandardTrait,
    ) -> Result<&'a NativeWitness, RuntimeError> {
        let receiver = contract.signature.params.get(slot).ok_or_else(invalid)?;
        contract
            .witnesses
            .iter()
            .find(|witness| {
                &witness.receiver == receiver
                    && StandardTrait::from_id(&witness.interface.declaration) == Some(protocol)
            })
            .ok_or_else(invalid)
    }
    fn iterator_type(
        &self,
        contract: &EngineNativeImport,
        slot: usize,
    ) -> Result<AbiType, RuntimeError> {
        let witness = self.witness(contract, slot, StandardTrait::Iterable)?;
        witness
            .interface
            .associated_types
            .get(&associated_type_id(&witness.interface.declaration, "Iter"))
            .cloned()
            .ok_or_else(invalid)
    }
    fn item(&self, contract: &EngineNativeImport) -> Result<AbiType, RuntimeError> {
        let witness = self.witness(contract, 0, StandardTrait::List)?;
        let [item] = witness.interface.arguments.as_slice() else {
            return Err(invalid());
        };
        Ok(item.clone())
    }
    fn optional(&self, contract: &EngineNativeImport) -> Result<AbiType, RuntimeError> {
        Ok(AbiType::StandardEnum {
            kind: StandardEnum::Option,
            args: vec![self.item(contract)?],
        })
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
            Call::SourceIter => self.iterator_type(contract, 0)?,
            Call::NeedleIter => self.iterator_type(contract, 1)?,
            Call::SourceLen | Call::NeedleLen => AbiType::Builtin(BuiltinType::USize),
            Call::Left | Call::Right => self.optional(contract)?,
            Call::Equal => AbiType::Builtin(BuiltinType::Bool),
        };
        if !runtime.matches_interface_method_abi(&value, &output, owner) {
            return Err(invalid());
        }
        match call {
            Call::SourceIter => {
                self.set(runtime, roots, SOURCE_ITER, value)?;
                self.phase = Phase::SourceBegin;
            }
            Call::NeedleIter => {
                self.set(runtime, roots, NEEDLE_ITER, value)?;
                self.phase = Phase::NeedleBegin;
            }
            Call::SourceLen | Call::NeedleLen => {
                let Value::U64(length) = value else {
                    return Err(invalid());
                };
                if matches!(call, Call::SourceLen) {
                    self.length = length;
                    self.limit = length;
                    self.phase = Phase::One;
                } else {
                    self.limit = length;
                    self.phase = Phase::Result;
                }
            }
            Call::Left => {
                self.set(runtime, roots, LEFT, value)?;
                self.phase = Phase::ReadLeft;
            }
            Call::Right => {
                self.set(runtime, roots, RIGHT, value)?;
                self.phase = Phase::ReadRight;
            }
            Call::Equal => {
                let Value::Bool(equal) = value else {
                    return Err(invalid());
                };
                self.condition = equal;
                self.phase = Phase::EqualBranch;
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
    fn read(
        &self,
        runtime: &Runtime,
        roots: &RootSet,
        slot: usize,
    ) -> Result<Option<NativeAction>, RuntimeError> {
        let Value::Enum(id) = self.get(roots, slot)? else {
            return Err(invalid());
        };
        let snapshot = runtime.gc().enum_snapshot(id).ok_or_else(invalid)?;
        if snapshot.tag != EnumTag::OptionSome {
            return Ok(Some(NativeAction::TypeMismatch(
                "standard enum payload variant",
            )));
        }
        self.set(
            runtime,
            roots,
            slot,
            snapshot.fields.into_iter().next().ok_or_else(invalid)?,
        )?;
        Ok(None)
    }
    pub(super) fn advance(
        &mut self,
        runtime: &Runtime,
        owner: &LoadedModule,
        contract: &EngineNativeImport,
        roots: &RootSet,
    ) -> Result<NativeAction, RuntimeError> {
        match self.phase {
            Phase::SourceIter | Phase::NeedleIter => {
                let slot = usize::from(matches!(self.phase, Phase::NeedleIter));
                let step = protocols::iter(
                    runtime,
                    owner,
                    self.witness(contract, slot, StandardTrait::Iterable)?,
                    roots.get(slot).ok_or_else(invalid)?,
                    &self.iterator_type(contract, slot)?,
                )?;
                return self.request(
                    runtime,
                    owner,
                    contract,
                    roots,
                    if slot == 0 {
                        Call::SourceIter
                    } else {
                        Call::NeedleIter
                    },
                    step,
                );
            }
            Phase::SourceBegin | Phase::NeedleBegin => {
                let slot = usize::from(matches!(self.phase, Phase::NeedleBegin));
                self.guards[slot] = Some(
                    runtime
                        .gc()
                        .begin_collection_iteration(&self.get(roots, slot)?)?,
                );
                self.phase = if slot == 0 {
                    Phase::SourceLen
                } else {
                    Phase::NeedleLen
                };
            }
            Phase::SourceLen | Phase::NeedleLen => {
                let slot = usize::from(matches!(self.phase, Phase::NeedleLen));
                let step = protocols::list(
                    runtime,
                    owner,
                    self.witness(contract, slot, StandardTrait::List)?,
                    0,
                    vec![roots.get(slot).ok_or_else(invalid)?],
                    &AbiType::Builtin(BuiltinType::USize),
                )?;
                return self.request(
                    runtime,
                    owner,
                    contract,
                    roots,
                    if slot == 0 {
                        Call::SourceLen
                    } else {
                        Call::NeedleLen
                    },
                    step,
                );
            }
            Phase::One => {
                self.phase = if self.sequence() {
                    Phase::NeedleIter
                } else {
                    Phase::Result
                }
            }
            Phase::Result => {
                self.result = self.sequence();
                self.phase = Phase::Index;
            }
            Phase::Index => {
                self.index = 0;
                self.phase = if self.sequence() {
                    Phase::Longer
                } else {
                    Phase::EntryJump
                };
            }
            Phase::EntryJump | Phase::BodyJump => self.phase = Phase::More,
            Phase::Longer => {
                self.condition = self.limit > self.length;
                self.phase = Phase::LongerBranch;
            }
            Phase::LongerBranch => {
                self.phase = if self.condition {
                    Phase::Mismatch
                } else {
                    Phase::More
                }
            }
            Phase::More => {
                self.condition = self.index < self.limit;
                self.phase = Phase::MoreBranch;
            }
            Phase::MoreBranch => {
                self.offset = self.index;
                self.phase = if self.condition {
                    if self.operation == NativeDefaultMethod::ListEndsWith {
                        Phase::OffsetStart
                    } else {
                        Phase::Left
                    }
                } else {
                    if self.sequence() {
                        Phase::NeedleClose
                    } else {
                        Phase::SourceClose
                    }
                };
            }
            Phase::OffsetStart => {
                self.offset = self.length.checked_sub(self.limit).ok_or_else(invalid)?;
                self.phase = Phase::OffsetAdd;
            }
            Phase::OffsetAdd => {
                self.offset = self.offset.checked_add(self.index).ok_or_else(invalid)?;
                self.phase = Phase::Left;
            }
            Phase::Left | Phase::Right => {
                let slot = usize::from(matches!(self.phase, Phase::Right));
                let index = if slot == 0 { self.offset } else { self.index };
                let step = protocols::list(
                    runtime,
                    owner,
                    self.witness(contract, slot, StandardTrait::List)?,
                    2,
                    vec![roots.get(slot).ok_or_else(invalid)?, Value::U64(index)],
                    &self.optional(contract)?,
                )?;
                return self.request(
                    runtime,
                    owner,
                    contract,
                    roots,
                    if slot == 0 { Call::Left } else { Call::Right },
                    step,
                );
            }
            Phase::ReadLeft => {
                if let Some(error) = self.read(runtime, roots, LEFT)? {
                    return Ok(error);
                }
                if self.sequence() {
                    self.phase = Phase::Right;
                } else {
                    self.set(runtime, roots, RIGHT, roots.get(1).ok_or_else(invalid)?)?;
                    self.phase = Phase::Equal;
                }
            }
            Phase::ReadRight => {
                if let Some(error) = self.read(runtime, roots, RIGHT)? {
                    return Ok(error);
                }
                self.phase = Phase::Equal;
            }
            Phase::Equal => {
                let item = self.item(contract)?;
                let witness = contract
                    .witnesses
                    .iter()
                    .find(|witness| {
                        witness.receiver == item
                            && StandardTrait::from_id(&witness.interface.declaration)
                                == Some(StandardTrait::PartialEq)
                    })
                    .ok_or_else(invalid)?;
                let step = protocols::equal(
                    runtime,
                    owner,
                    witness,
                    self.get(roots, LEFT)?,
                    self.get(roots, RIGHT)?,
                )?;
                return self.request(runtime, owner, contract, roots, Call::Equal, step);
            }
            Phase::EqualBranch => {
                self.phase = if self.condition {
                    if self.sequence() {
                        Phase::Advance
                    } else {
                        Phase::Found
                    }
                } else {
                    if self.sequence() {
                        Phase::Mismatch
                    } else {
                        Phase::Advance
                    }
                }
            }
            Phase::Advance => {
                self.offset = self.index.checked_add(1).ok_or_else(invalid)?;
                self.phase = Phase::MoveIndex;
            }
            Phase::MoveIndex => {
                self.index = self.offset;
                self.phase = Phase::BodyJump;
            }
            Phase::Found | Phase::Mismatch => {
                self.result = matches!(self.phase, Phase::Found);
                self.phase = Phase::MoveResult;
            }
            Phase::MoveResult => self.phase = Phase::ExitJump,
            Phase::ExitJump => {
                self.phase = if self.sequence() {
                    Phase::NeedleClose
                } else {
                    Phase::SourceClose
                }
            }
            Phase::NeedleClose => {
                runtime.iter_operation(
                    owner,
                    &self.get(roots, NEEDLE_ITER)?,
                    &self.iterator_type(contract, 1)?,
                    IterOp::Close,
                )?;
                self.phase = Phase::NeedleEnd;
            }
            Phase::NeedleEnd => {
                self.guards[1].take();
                self.phase = Phase::SourceClose;
            }
            Phase::SourceClose => {
                runtime.iter_operation(
                    owner,
                    &self.get(roots, SOURCE_ITER)?,
                    &self.iterator_type(contract, 0)?,
                    IterOp::Close,
                )?;
                self.phase = Phase::SourceEnd;
            }
            Phase::SourceEnd => {
                self.guards[0].take();
                return Ok(NativeAction::Complete(Value::Bool(self.result)));
            }
            Phase::Waiting(_) => return Err(invalid()),
        }
        Ok(NativeAction::Continue)
    }
}
