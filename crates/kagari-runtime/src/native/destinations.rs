//! Rooted fallible traversal and selected nested destination construction.
mod factory;
use crate::{
    LoadedModule, Runtime, RuntimeError,
    gc::{CollectionIteration, RootSet},
    native::{
        NativeAction,
        destinations::factory::Factory,
        protocols::{self, ProtocolStep},
        sources::SourceSelection,
    },
    value::{EnumTag, Value},
};
use kagari_abi::{
    native_import::{EngineNativeImport, NativeWitnessImplementation},
    operations::IterOp,
    standard::{StandardIntrinsic, surface::StandardEnum},
    types::AbiType,
};
use kagari_common::collection::CollectionAccess;

const BUFFER: usize = 0;
const ITERATOR: usize = 1;
const NEXT: usize = 2;
const ITEM: usize = 3;
const PAYLOAD: usize = 4;
const RESULT: usize = 5;
const SCRATCH_ROOTS: usize = 6;

fn invalid() -> RuntimeError {
    RuntimeError::module_validation("native fallible destination mismatch")
}
#[derive(Clone, Copy)]
enum Phase {
    WaitingIter,
    New,
    Begin,
    Jump,
    Next,
    WaitingNext,
    Test,
    Branch,
    Read,
    Valid,
    ValidBranch,
    Payload,
    Append,
    Error,
    Failure,
    FailureMove,
    Close,
    End,
    Destination,
    WaitingDestination,
    Wrap,
    Move,
    Done,
}
pub(super) struct Fallible {
    source: SourceSelection,
    output: AbiType,
    scratch: usize,
    phase: Phase,
    factory: Factory,
    guarded: bool,
    present: bool,
    succeeded: bool,
    guard: Option<CollectionIteration>,
}
impl Fallible {
    pub(super) fn start(
        owner: &LoadedModule,
        contract: &EngineNativeImport,
        arguments: &[Value],
    ) -> Result<Self, RuntimeError> {
        let source = SourceSelection::select(contract, &contract.signature.params[0], None, 0)?;
        Self::for_source(
            owner,
            contract,
            source,
            &contract.signature.result,
            arguments.len(),
        )
    }
    fn for_source(
        owner: &LoadedModule,
        contract: &EngineNativeImport,
        source: SourceSelection,
        output: &AbiType,
        scratch: usize,
    ) -> Result<Self, RuntimeError> {
        let optional = source.optional(contract)?;
        let AbiType::StandardEnum { args, .. } = optional else {
            return Err(invalid());
        };
        let Some(AbiType::StandardEnum { args: inputs, .. }) = args.first() else {
            return Err(invalid());
        };
        let AbiType::StandardEnum { args: outputs, .. } = output else {
            return Err(invalid());
        };
        let buffer = AbiType::Array(
            Box::new(inputs.first().ok_or_else(invalid)?.clone()),
            CollectionAccess::Mutable,
        );
        let factory = Factory::select(
            owner,
            contract,
            outputs.first().ok_or_else(invalid)?,
            &buffer,
            scratch + BUFFER,
            scratch + SCRATCH_ROOTS,
        )?;
        Ok(Self {
            source,
            output: output.clone(),
            scratch,
            phase: Phase::WaitingIter,
            factory,
            guarded: matches!(source.next(contract).receiver, AbiType::Iter(_)),
            present: false,
            succeeded: true,
            guard: None,
        })
    }
    pub(super) fn scratch_roots(&self) -> usize {
        SCRATCH_ROOTS + self.factory.scratch_roots()
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
    fn new_array(
        &mut self,
        runtime: &Runtime,
        roots: &RootSet,
    ) -> Result<NativeAction, RuntimeError> {
        match runtime.invoke_standard_builtin(StandardIntrinsic::ArrayListNew, &[]) {
            Ok(value) => {
                self.set(runtime, roots, BUFFER, value)?;
                self.phase = if self.guarded {
                    Phase::Begin
                } else {
                    Phase::Jump
                };
                Ok(NativeAction::Continue)
            }
            Err(error) => Ok(NativeAction::BuiltinFailure(error)),
        }
    }
    pub(super) fn initialize(
        &mut self,
        runtime: &Runtime,
        owner: &LoadedModule,
        contract: &EngineNativeImport,
        roots: &RootSet,
    ) -> Result<NativeAction, RuntimeError> {
        let value = roots.get(self.source.root).ok_or_else(invalid)?;
        let witness = self.source.iterable(contract);
        let output = &self.source.next(contract).receiver;
        if witness.implementation == NativeWitnessImplementation::Primitive
            && witness.receiver == *output
        {
            self.set(runtime, roots, ITERATOR, value)?;
            return self.new_array(runtime, roots);
        }
        let step = protocols::iter(runtime, owner, witness, value, output)?;
        self.request(runtime, owner, contract, roots, step)
    }
    fn request(
        &mut self,
        runtime: &Runtime,
        owner: &LoadedModule,
        contract: &EngineNativeImport,
        roots: &RootSet,
        step: ProtocolStep,
    ) -> Result<NativeAction, RuntimeError> {
        match step {
            ProtocolStep::Call(request) => Ok(NativeAction::Callback(request)),
            ProtocolStep::Value(value) => self.receive(runtime, owner, contract, roots, value),
            ProtocolStep::BuiltinFailure(error) => Ok(NativeAction::BuiltinFailure(error)),
        }
    }
    fn destination_action(
        &mut self,
        runtime: &Runtime,
        owner: &LoadedModule,
        roots: &RootSet,
        action: NativeAction,
    ) -> Result<NativeAction, RuntimeError> {
        if let NativeAction::Complete(value) = action {
            let AbiType::StandardEnum { args, .. } = &self.output else {
                return Err(invalid());
            };
            if !runtime.matches_interface_method_abi(&value, &args[0], owner) {
                return Err(invalid());
            }
            self.set(runtime, roots, RESULT, value)?;
            self.phase = Phase::Wrap;
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
        if matches!(self.phase, Phase::WaitingDestination) {
            let action = self
                .factory
                .receive(runtime, owner, contract, roots, value)?;
            return self.destination_action(runtime, owner, roots, action);
        }
        let (slot, ty, next) = match self.phase {
            Phase::WaitingIter => (
                ITERATOR,
                self.source.next(contract).receiver.clone(),
                Phase::New,
            ),
            Phase::WaitingNext => (NEXT, self.source.optional(contract)?, Phase::Test),
            _ => return Err(invalid()),
        };
        if !runtime.matches_interface_method_abi(&value, &ty, owner) {
            return Err(invalid());
        }
        self.set(runtime, roots, slot, value)?;
        self.phase = next;
        Ok(NativeAction::Continue)
    }
    fn snapshot(
        &self,
        runtime: &Runtime,
        roots: &RootSet,
        slot: usize,
    ) -> Result<(EnumTag, Option<Value>), RuntimeError> {
        let Value::Enum(id) = self.get(roots, slot)? else {
            return Err(invalid());
        };
        let snapshot = runtime.gc().enum_snapshot(id).ok_or_else(invalid)?;
        Ok((snapshot.tag, snapshot.fields.into_iter().next()))
    }
    pub(super) fn advance(
        &mut self,
        runtime: &Runtime,
        owner: &LoadedModule,
        contract: &EngineNativeImport,
        roots: &RootSet,
    ) -> Result<NativeAction, RuntimeError> {
        match self.phase {
            Phase::New => return self.new_array(runtime, roots),
            Phase::Begin => {
                self.guard = Some(
                    runtime
                        .gc()
                        .begin_collection_iteration(&self.get(roots, ITERATOR)?)?,
                );
                self.phase = Phase::Jump;
            }
            Phase::Jump => self.phase = Phase::Next,
            Phase::Next => {
                let step = protocols::next(
                    runtime,
                    owner,
                    self.source.next(contract),
                    self.get(roots, ITERATOR)?,
                    &self.source.optional(contract)?,
                )?;
                self.phase = Phase::WaitingNext;
                return self.request(runtime, owner, contract, roots, step);
            }
            Phase::Test => {
                self.present = self.snapshot(runtime, roots, NEXT)?.0 == EnumTag::OptionSome;
                self.phase = Phase::Branch;
            }
            Phase::Branch => {
                self.phase = if self.present {
                    Phase::Read
                } else if self.guarded {
                    Phase::Close
                } else {
                    Phase::Destination
                }
            }
            Phase::Read => {
                self.set(
                    runtime,
                    roots,
                    ITEM,
                    self.snapshot(runtime, roots, NEXT)?.1.ok_or_else(invalid)?,
                )?;
                self.phase = Phase::Valid;
            }
            Phase::Valid => {
                self.succeeded = matches!(
                    self.snapshot(runtime, roots, ITEM)?.0,
                    EnumTag::OptionSome | EnumTag::ResultOk
                );
                self.phase = Phase::ValidBranch;
            }
            Phase::ValidBranch => {
                self.phase = if self.succeeded {
                    Phase::Payload
                } else if matches!(
                    self.output,
                    AbiType::StandardEnum {
                        kind: StandardEnum::Result,
                        ..
                    }
                ) {
                    Phase::Error
                } else {
                    Phase::Failure
                }
            }
            Phase::Payload => {
                self.set(
                    runtime,
                    roots,
                    PAYLOAD,
                    self.snapshot(runtime, roots, ITEM)?.1.ok_or_else(invalid)?,
                )?;
                self.phase = Phase::Append;
            }
            Phase::Append => {
                if let Err(error) = runtime.invoke_standard_builtin(
                    StandardIntrinsic::ArrayPush,
                    &[self.get(roots, BUFFER)?, self.get(roots, PAYLOAD)?],
                ) {
                    return Ok(NativeAction::BuiltinFailure(error));
                }
                self.phase = Phase::Jump;
            }
            Phase::Error => {
                self.set(
                    runtime,
                    roots,
                    PAYLOAD,
                    self.snapshot(runtime, roots, ITEM)?.1.ok_or_else(invalid)?,
                )?;
                self.phase = Phase::Failure;
            }
            Phase::Failure => {
                let value = if matches!(
                    self.output,
                    AbiType::StandardEnum {
                        kind: StandardEnum::Result,
                        ..
                    }
                ) {
                    runtime.map_result_error(
                        owner,
                        &self.get(roots, ITEM)?,
                        self.get(roots, PAYLOAD)?,
                        &self.output,
                    )?
                } else {
                    Value::Enum(runtime.alloc_enum(EnumTag::OptionNone, vec![])?)
                };
                self.set(runtime, roots, RESULT, value)?;
                self.phase = Phase::FailureMove;
            }
            Phase::FailureMove => {
                self.phase = if self.guarded {
                    Phase::Close
                } else {
                    Phase::Done
                }
            }
            Phase::Close => {
                runtime.iter_operation(
                    owner,
                    &self.get(roots, ITERATOR)?,
                    &self.source.next(contract).receiver,
                    IterOp::Close,
                )?;
                self.phase = Phase::End;
            }
            Phase::End => {
                self.guard.take();
                self.phase = if self.succeeded {
                    Phase::Destination
                } else {
                    Phase::Done
                };
            }
            Phase::Destination => {
                self.phase = Phase::WaitingDestination;
                let action = self.factory.initialize(runtime, owner, contract, roots)?;
                return self.destination_action(runtime, owner, roots, action);
            }
            Phase::WaitingDestination => {
                let action = self.factory.advance(runtime, owner, contract, roots)?;
                return self.destination_action(runtime, owner, roots, action);
            }
            Phase::Wrap => {
                let tag = if matches!(
                    self.output,
                    AbiType::StandardEnum {
                        kind: StandardEnum::Result,
                        ..
                    }
                ) {
                    EnumTag::ResultOk
                } else {
                    EnumTag::OptionSome
                };
                let value = Value::Enum(runtime.alloc_enum(tag, vec![self.get(roots, RESULT)?])?);
                self.set(runtime, roots, RESULT, value)?;
                self.phase = Phase::Move;
            }
            Phase::Move => self.phase = Phase::Done,
            Phase::Done => return self.get(roots, RESULT).map(NativeAction::Complete),
            Phase::WaitingIter | Phase::WaitingNext => return Err(invalid()),
        }
        Ok(NativeAction::Continue)
    }
}
