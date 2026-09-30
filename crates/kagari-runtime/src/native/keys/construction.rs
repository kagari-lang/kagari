//! Construct ordered hash storage through selected traversal and the shared key lookup.
use super::{
    Buffers, KeySelection, invalid,
    lookup::{KeyStep, Lookup},
};
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
    native_import::{EngineNativeImport, NativeWitnessImplementation},
    operations::IterOp,
    standard::StandardIntrinsic,
    types::AbiType,
};
const OUTPUT: usize = 0;
const ITERATOR: usize = 1;
const NEXT: usize = 2;
const ITEM: usize = 3;
const KEY: usize = 4;
const VALUE: usize = 5;
const LOOKUP: usize = 6;
pub(super) const SCRATCH_ROOTS: usize = LOOKUP + super::SCRATCH_ROOTS;
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
    Zero,
    One,
    ReadKey,
    ReadValue,
    Insert,
    Close,
    End,
    Move,
}
pub(super) struct Construction {
    scratch: usize,
    phase: Phase,
    map: bool,
    keys: KeySelection,
    source: SourceSelection,
    guarded: bool,
    present: bool,
    guard: Option<CollectionIteration>,
    lookup: Option<Lookup>,
}
impl Construction {
    pub(super) fn start(
        contract: &EngineNativeImport,
        arguments: &[Value],
    ) -> Result<Self, RuntimeError> {
        let source = SourceSelection::select(contract, &contract.signature.params[0], None, 0)?;
        Self::for_source(
            contract,
            source,
            &contract.signature.result,
            arguments.len(),
        )
    }
    pub(super) fn for_source(
        contract: &EngineNativeImport,
        source: SourceSelection,
        storage: &AbiType,
        scratch: usize,
    ) -> Result<Self, RuntimeError> {
        let map = match storage {
            AbiType::Map { .. } => true,
            AbiType::Set(..) => false,
            _ => return Err(invalid()),
        };
        Ok(Self {
            scratch,
            phase: Phase::WaitingIter,
            map,
            keys: KeySelection::for_storage(contract, storage)?,
            source,
            guarded: matches!(source.next(contract).receiver, AbiType::Iter(_)),
            present: false,
            guard: None,
            lookup: None,
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
    fn new_storage(
        &mut self,
        runtime: &Runtime,
        roots: &RootSet,
    ) -> Result<NativeAction, RuntimeError> {
        match runtime.invoke_standard_builtin(
            if self.map {
                StandardIntrinsic::LinkedHashMapNew
            } else {
                StandardIntrinsic::LinkedHashSetNew
            },
            &[],
        ) {
            Ok(value) => {
                self.set(runtime, roots, OUTPUT, value)?;
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
    // Identity Iterable has no conversion instruction; entry allocates its result.
    pub(super) fn initialize(
        &mut self,
        runtime: &Runtime,
        owner: &LoadedModule,
        contract: &EngineNativeImport,
        roots: &RootSet,
    ) -> Result<NativeAction, RuntimeError> {
        let value = roots.get(self.source.root).ok_or_else(invalid)?;
        let source = self.source.iterable(contract);
        let output = &self.source.next(contract).receiver;
        if source.implementation == NativeWitnessImplementation::Primitive
            && source.receiver == *output
        {
            self.set(runtime, roots, ITERATOR, value)?;
            return self.new_storage(runtime, roots);
        }
        let step = protocols::iter(runtime, owner, source, value, output)?;
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
            ProtocolStep::Value(value) => self.receive(runtime, owner, contract, roots, value),
            ProtocolStep::Call(request) => Ok(NativeAction::Callback(request)),
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
        if matches!(self.phase, Phase::Insert) {
            return self
                .lookup
                .as_mut()
                .ok_or_else(invalid)?
                .receive(runtime, owner, value);
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
    fn insert(&mut self) {
        self.lookup = Some(Lookup::start(
            if self.map {
                StandardIntrinsic::MapInsert
            } else {
                StandardIntrinsic::SetInsert
            },
            self.keys,
            Buffers {
                scratch: self.scratch + LOOKUP,
                receiver: self.scratch + OUTPUT,
                query: self.scratch + KEY,
            },
            self.map.then_some(self.scratch + VALUE),
        ));
        self.phase = Phase::Insert;
    }
    pub(super) fn advance(
        &mut self,
        runtime: &Runtime,
        owner: &LoadedModule,
        contract: &EngineNativeImport,
        roots: &RootSet,
    ) -> Result<NativeAction, RuntimeError> {
        match self.phase {
            Phase::New => return self.new_storage(runtime, roots),
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
                if self.map {
                    self.set(runtime, roots, ITEM, value)?;
                    self.phase = Phase::Zero;
                } else {
                    self.set(runtime, roots, KEY, value)?;
                    self.insert();
                }
            }
            Phase::Zero => self.phase = Phase::One,
            Phase::One => self.phase = Phase::ReadKey,
            Phase::ReadKey => {
                let Value::Tuple(values) = self.get(roots, ITEM)? else {
                    return Err(invalid());
                };
                self.set(
                    runtime,
                    roots,
                    KEY,
                    values.first().cloned().ok_or_else(invalid)?,
                )?;
                self.phase = Phase::ReadValue;
            }
            Phase::ReadValue => {
                let Value::Tuple(values) = self.get(roots, ITEM)? else {
                    return Err(invalid());
                };
                self.set(
                    runtime,
                    roots,
                    VALUE,
                    values.get(1).cloned().ok_or_else(invalid)?,
                )?;
                self.insert();
            }
            Phase::Insert => match self
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
            },
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
            Phase::Move => return self.get(roots, OUTPUT).map(NativeAction::Complete),
            Phase::WaitingIter | Phase::WaitingNext => return Err(invalid()),
        }
        Ok(NativeAction::Continue)
    }
}
