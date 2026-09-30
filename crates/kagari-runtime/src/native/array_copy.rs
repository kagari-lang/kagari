//! Rooted source snapshots and final array construction or atomic copy/append.
use crate::{
    LoadedModule, Runtime, RuntimeError,
    gc::{CollectionIteration, RootSet},
    native::{
        NativeAction,
        protocols::{self, ProtocolStep},
        sources::SourceSelection,
    },
    value::{EnumTag, Value},
};
use kagari_abi::{
    callable::EngineNativeBinding,
    native_import::{EngineNativeImport, NativeWitnessImplementation},
    operations::IterOp,
    standard::StandardIntrinsic,
    types::AbiType,
};

const ARRAY: usize = 0;
const ITERATOR: usize = 1;
const NEXT: usize = 2;
const ITEM: usize = 3;
pub(super) const SCRATCH_ROOTS: usize = 4;

#[derive(Clone, Copy)]
enum Phase {
    New,
    WaitingIter,
    Begin,
    Jump,
    Next,
    WaitingNext,
    Test,
    Branch,
    Read,
    Append,
    Close,
    End,
    Move,
    Commit,
}

pub(super) struct ArrayCopy {
    source: SourceSelection,
    commit: Option<StandardIntrinsic>,
    scratch: usize,
    phase: Phase,
    guarded: bool,
    present: bool,
    guard: Option<CollectionIteration>,
}
fn invalid() -> RuntimeError {
    RuntimeError::module_validation("native array snapshot contract mismatch")
}

impl ArrayCopy {
    pub(super) fn start(
        contract: &EngineNativeImport,
        arguments: &[Value],
    ) -> Result<Self, RuntimeError> {
        let source = usize::from(matches!(
            contract.binding,
            EngineNativeBinding::Intrinsic(
                StandardIntrinsic::ArrayCopyFrom | StandardIntrinsic::ArrayExtend
            )
        ));
        let selection =
            SourceSelection::select(contract, &contract.signature.params[source], None, source)?;
        let commit = (source == 1).then_some(
            if contract.binding == EngineNativeBinding::Intrinsic(StandardIntrinsic::ArrayExtend) {
                StandardIntrinsic::ArrayExtendStorage
            } else {
                StandardIntrinsic::ArrayCopyFromStorage
            },
        );
        Self::for_source(contract, selection, arguments.len(), commit)
    }
    pub(super) fn for_source(
        contract: &EngineNativeImport,
        source: SourceSelection,
        scratch: usize,
        commit: Option<StandardIntrinsic>,
    ) -> Result<Self, RuntimeError> {
        let guarded = matches!(source.next(contract).receiver, AbiType::Iter(_));
        Ok(Self {
            source,
            commit,
            scratch,
            phase: Phase::WaitingIter,
            guarded,
            present: false,
            guard: None,
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
    fn new_array(
        &mut self,
        runtime: &Runtime,
        roots: &RootSet,
    ) -> Result<NativeAction, RuntimeError> {
        match runtime.invoke_standard_builtin(StandardIntrinsic::ArrayListNew, &[]) {
            Ok(value) => {
                self.set(runtime, roots, ARRAY, value)?;
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
    /// The first original operation is conversion, or allocation for identity Iterable.
    /// It runs only after all argument/scratch roots have been registered.
    pub(super) fn initialize(
        &mut self,
        runtime: &Runtime,
        owner: &LoadedModule,
        contract: &EngineNativeImport,
        roots: &RootSet,
    ) -> Result<NativeAction, RuntimeError> {
        let source = roots.get(self.source.root).ok_or_else(invalid)?;
        let witness = self.source.iterable(contract);
        let output = &self.source.next(contract).receiver;
        if witness.implementation == NativeWitnessImplementation::Primitive
            && witness.receiver == *output
        {
            self.set(runtime, roots, ITERATOR, source)?;
            return self.new_array(runtime, roots);
        }
        let step = protocols::iter(runtime, owner, witness, source, output)?;
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
    pub(super) fn receive(
        &mut self,
        runtime: &Runtime,
        owner: &LoadedModule,
        contract: &EngineNativeImport,
        roots: &RootSet,
        value: Value,
    ) -> Result<NativeAction, RuntimeError> {
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
                let Value::Enum(id) = self.get(roots, NEXT)? else {
                    return Err(invalid());
                };
                self.present =
                    runtime.gc().enum_snapshot(id).ok_or_else(invalid)?.tag == EnumTag::OptionSome;
                self.phase = Phase::Branch;
            }
            Phase::Branch => {
                self.phase = if self.present {
                    Phase::Read
                } else if self.guarded {
                    Phase::Close
                } else {
                    Phase::Move
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
                self.phase = Phase::Append;
            }
            Phase::Append => {
                if let Err(error) = runtime.invoke_standard_builtin(
                    StandardIntrinsic::ArrayPush,
                    &[self.get(roots, ARRAY)?, self.get(roots, ITEM)?],
                ) {
                    return Ok(NativeAction::BuiltinFailure(error));
                }
                self.phase = Phase::Jump;
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
                self.phase = Phase::Move;
            }
            Phase::Move => {
                if self.commit.is_none() {
                    return self.get(roots, ARRAY).map(NativeAction::Complete);
                }
                self.phase = Phase::Commit;
            }
            Phase::Commit => {
                let operation = self.commit.ok_or_else(invalid)?;
                return Ok(
                    match runtime.invoke_standard_builtin(
                        operation,
                        &[roots.get(0).ok_or_else(invalid)?, self.get(roots, ARRAY)?],
                    ) {
                        Ok(value) => NativeAction::Complete(value),
                        Err(error) => NativeAction::BuiltinFailure(error),
                    },
                );
            }
            Phase::WaitingIter | Phase::WaitingNext => return Err(invalid()),
        }
        Ok(NativeAction::Continue)
    }
}
