//! Once-only key selection, shared lookup, and ordered shallow groups.
use super::{
    Buffers, invalid,
    lookup::{KeyStep, Lookup},
    witness,
};
use crate::{
    LoadedModule, Runtime, RuntimeError,
    gc::{CollectionIteration, RootSet},
    native::{
        NativeAction, callback,
        protocols::{self, ProtocolStep},
    },
    value::{EnumTag, Value},
};
use kagari_abi::{
    native_import::{EngineNativeImport, NativeWitness, NativeWitnessImplementation},
    operations::IterOp,
    standard::{StandardIntrinsic, surface::StandardEnum, traits::StandardTrait},
    types::AbiType,
};
use kagari_common::identity::associated_type_id;

const OUTPUT: usize = 0;
const NEXT: usize = 1;
const ITEM: usize = 2;
const KEY: usize = 3;
const FOUND: usize = 4;
const GROUP: usize = 5;
const LOOKUP: usize = 6;
pub(super) const SCRATCH_ROOTS: usize = LOOKUP + super::SCRATCH_ROOTS;

#[derive(Clone, Copy)]
enum Phase {
    Begin,
    Jump,
    Next,
    WaitingNext,
    Test,
    Branch,
    Read,
    Key,
    WaitingKey,
    Get,
    GroupTest,
    GroupBranch,
    GroupRead,
    GroupNew,
    Append,
    Insert,
    Close,
    End,
}

pub(super) struct Grouping {
    scratch: usize,
    phase: Phase,
    custom: bool,
    guarded: bool,
    present: bool,
    fresh: bool,
    guard: Option<CollectionIteration>,
    lookup: Option<Lookup>,
}

impl Grouping {
    pub(super) fn start(
        contract: &EngineNativeImport,
        arguments: &[Value],
    ) -> Result<Self, RuntimeError> {
        let guarded = matches!(contract.signature.params.first(), Some(AbiType::Iter(_)));
        let equality = witness(contract, StandardTrait::PartialEq)?;
        Ok(Self {
            scratch: arguments.len(),
            phase: if guarded { Phase::Begin } else { Phase::Jump },
            custom: matches!(
                equality.implementation,
                NativeWitnessImplementation::Table(_) | NativeWitnessImplementation::Derived
            ),
            guarded,
            present: false,
            fresh: false,
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

    fn source<'a>(
        &self,
        contract: &'a EngineNativeImport,
    ) -> Result<&'a NativeWitness, RuntimeError> {
        contract
            .witnesses
            .iter()
            .find(|w| {
                w.receiver == contract.signature.params[0]
                    && StandardTrait::from_id(&w.interface.declaration)
                        == Some(StandardTrait::Iterator)
            })
            .ok_or_else(invalid)
    }

    fn optional(&self, contract: &EngineNativeImport) -> Result<AbiType, RuntimeError> {
        let source = self.source(contract)?;
        let item = source
            .interface
            .associated_types
            .get(&associated_type_id(&source.interface.declaration, "Item"))
            .ok_or_else(invalid)?;
        Ok(AbiType::StandardEnum {
            kind: StandardEnum::Option,
            args: vec![item.clone()],
        })
    }

    pub(super) fn initialize(
        &self,
        runtime: &Runtime,
        roots: &RootSet,
    ) -> Result<NativeAction, RuntimeError> {
        match runtime.invoke_standard_builtin(StandardIntrinsic::LinkedHashMapNew, &[]) {
            Ok(value) => {
                self.set(runtime, roots, OUTPUT, value)?;
                Ok(NativeAction::Continue)
            }
            Err(error) => Ok(NativeAction::BuiltinFailure(error)),
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
        if matches!(self.phase, Phase::Get | Phase::Insert)
            && let Some(lookup) = &mut self.lookup
        {
            return lookup.receive(runtime, owner, value);
        }
        let (slot, expected, next) = match self.phase {
            Phase::WaitingNext => (NEXT, self.optional(contract)?, Phase::Test),
            Phase::WaitingKey => {
                let AbiType::Map { key, .. } = &contract.signature.result else {
                    return Err(invalid());
                };
                (KEY, key.as_ref().clone(), Phase::Get)
            }
            _ => return Err(invalid()),
        };
        if !runtime.matches_interface_method_abi(&value, &expected, owner) {
            return Err(invalid());
        }
        self.set(runtime, roots, slot, value)?;
        self.phase = next;
        Ok(NativeAction::Continue)
    }

    fn lookup(
        &mut self,
        runtime: &Runtime,
        owner: &LoadedModule,
        contract: &EngineNativeImport,
        roots: &RootSet,
        insert: bool,
    ) -> Result<NativeAction, RuntimeError> {
        if self.lookup.is_none() {
            self.lookup = Some(Lookup::start(
                if insert {
                    StandardIntrinsic::MapInsert
                } else {
                    StandardIntrinsic::MapGet
                },
                self.custom,
                Buffers {
                    scratch: self.scratch + LOOKUP,
                    receiver: self.scratch + OUTPUT,
                    query: self.scratch + KEY,
                },
                insert.then_some(self.scratch + GROUP),
            ));
        }
        match self
            .lookup
            .as_mut()
            .ok_or_else(invalid)?
            .advance(runtime, owner, contract, roots)?
        {
            KeyStep::Action(action) => Ok(action),
            KeyStep::Ready(value) => {
                self.lookup = None;
                if insert {
                    self.phase = Phase::Jump;
                } else {
                    self.set(runtime, roots, FOUND, value)?;
                    self.phase = Phase::GroupTest;
                }
                Ok(NativeAction::Continue)
            }
        }
    }

    fn field(
        &self,
        runtime: &Runtime,
        roots: &RootSet,
        slot: usize,
    ) -> Result<Value, RuntimeError> {
        let Value::Enum(id) = self.get(roots, slot)? else {
            return Err(invalid());
        };
        runtime
            .gc()
            .enum_snapshot(id)
            .ok_or_else(invalid)?
            .fields
            .into_iter()
            .next()
            .ok_or_else(invalid)
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
                self.phase = Phase::Jump;
            }
            Phase::Jump => self.phase = Phase::Next,
            Phase::Next => {
                let step = protocols::next(
                    runtime,
                    owner,
                    self.source(contract)?,
                    roots.get(0).ok_or_else(invalid)?,
                    &self.optional(contract)?,
                )?;
                self.phase = Phase::WaitingNext;
                return match step {
                    ProtocolStep::Call(request) => Ok(NativeAction::Callback(request)),
                    ProtocolStep::Value(value) => {
                        self.receive(runtime, owner, contract, roots, value)
                    }
                    ProtocolStep::BuiltinFailure(error) => Ok(NativeAction::BuiltinFailure(error)),
                };
            }
            Phase::Test | Phase::GroupTest => {
                let group = matches!(self.phase, Phase::GroupTest);
                let Value::Enum(id) = self.get(roots, if group { FOUND } else { NEXT })? else {
                    return Err(invalid());
                };
                self.present =
                    runtime.gc().enum_snapshot(id).ok_or_else(invalid)?.tag == EnumTag::OptionSome;
                self.phase = if group {
                    Phase::GroupBranch
                } else {
                    Phase::Branch
                };
            }
            Phase::Branch => {
                if self.present {
                    self.phase = Phase::Read;
                } else if self.guarded {
                    self.phase = Phase::Close;
                } else {
                    return self.get(roots, OUTPUT).map(NativeAction::Complete);
                }
            }
            Phase::Read => {
                self.set(runtime, roots, ITEM, self.field(runtime, roots, NEXT)?)?;
                self.phase = Phase::Key;
            }
            Phase::Key => {
                self.phase = Phase::WaitingKey;
                return callback(
                    runtime,
                    &roots.get(1).ok_or_else(invalid)?,
                    &contract.signature.params[1],
                    vec![self.get(roots, ITEM)?],
                )
                .map(NativeAction::Callback);
            }
            Phase::Get => return self.lookup(runtime, owner, contract, roots, false),
            Phase::GroupBranch => {
                self.fresh = !self.present;
                self.phase = if self.present {
                    Phase::GroupRead
                } else {
                    Phase::GroupNew
                };
            }
            Phase::GroupRead => {
                self.set(runtime, roots, GROUP, self.field(runtime, roots, FOUND)?)?;
                self.phase = Phase::Append;
            }
            Phase::GroupNew => {
                match runtime.invoke_standard_builtin(StandardIntrinsic::ArrayListNew, &[]) {
                    Ok(value) => self.set(runtime, roots, GROUP, value)?,
                    Err(error) => return Ok(NativeAction::BuiltinFailure(error)),
                }
                self.phase = Phase::Append;
            }
            Phase::Append => {
                if let Err(error) = runtime.invoke_standard_builtin(
                    StandardIntrinsic::ArrayPush,
                    &[self.get(roots, GROUP)?, self.get(roots, ITEM)?],
                ) {
                    return Ok(NativeAction::BuiltinFailure(error));
                }
                self.phase = if self.fresh {
                    Phase::Insert
                } else {
                    Phase::Jump
                };
            }
            Phase::Insert => return self.lookup(runtime, owner, contract, roots, true),
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
                return self.get(roots, OUTPUT).map(NativeAction::Complete);
            }
            Phase::WaitingNext | Phase::WaitingKey => return Err(invalid()),
        }
        Ok(NativeAction::Continue)
    }
}
