//! Ordered shallow Map snapshots and their checked readonly List representation.
use crate::{
    LoadedModule, Runtime, RuntimeError,
    builtin::BuiltinError,
    gc::{CollectionIteration, RootSet},
    native::{
        NativeAction,
        protocols::{self, ProtocolStep},
        results,
    },
    value::{EnumTag, Value},
};
use kagari_abi::{
    callable::EngineNativeBinding,
    native_import::{EngineNativeImport, NativeWitness},
    operations::IterOp,
    standard::{
        StandardIntrinsic, bindings::NativeDefaultMethod, surface::StandardEnum,
        traits::StandardTrait,
    },
    types::AbiType,
};
use kagari_common::identity::associated_type_id;
const ARRAY: usize = 0;
const ITERATOR: usize = 1;
const NEXT: usize = 2;
const ITEM: usize = 3;
#[derive(Clone, Copy)]
enum Phase {
    Iter,
    WaitingIter,
    Begin,
    Jump,
    Next,
    WaitingNext,
    Test,
    Branch,
    Read,
    Index,
    Select,
    Append,
    Close,
    End,
    Interface,
}
pub(super) struct SnapshotInvocation {
    entry_operation: StandardIntrinsic,
    phase: Phase,
    field: Option<usize>,
    scratch: usize,
    present: bool,
    guard: Option<CollectionIteration>,
    pub(super) initial: Option<Vec<Value>>,
}
fn invalid() -> RuntimeError {
    RuntimeError::module_validation("native Map snapshot contract mismatch")
}
impl SnapshotInvocation {
    pub(super) fn start(
        contract: &EngineNativeImport,
        arguments: &[Value],
    ) -> Result<Self, RuntimeError> {
        let (operation, field, direct) = match contract.binding {
            EngineNativeBinding::Intrinsic(StandardIntrinsic::MapKeys) => {
                (StandardIntrinsic::MapKeysStorage, Some(0), true)
            }
            EngineNativeBinding::Intrinsic(StandardIntrinsic::MapValues) => {
                (StandardIntrinsic::MapValuesStorage, Some(1), true)
            }
            EngineNativeBinding::Intrinsic(StandardIntrinsic::MapEntries) => {
                (StandardIntrinsic::MapEntriesStorage, None, true)
            }
            EngineNativeBinding::TraitDefault(NativeDefaultMethod::MapKeysView) => {
                (StandardIntrinsic::ArrayListNew, Some(0), false)
            }
            EngineNativeBinding::TraitDefault(NativeDefaultMethod::MapValuesView) => {
                (StandardIntrinsic::ArrayListNew, Some(1), false)
            }
            EngineNativeBinding::TraitDefault(NativeDefaultMethod::MapEntriesView) => {
                (StandardIntrinsic::ArrayListNew, None, false)
            }
            _ => return Err(invalid()),
        };
        Ok(Self {
            entry_operation: operation,
            phase: if direct {
                Phase::Interface
            } else {
                Phase::Iter
            },
            field,
            scratch: arguments.len(),
            present: false,
            guard: None,
            initial: Some(vec![Value::Unit; 4]),
        })
    }
    /// Run the already charged entry after argument/scratch roots are registered.
    /// A resource failure can prohibit registering any further execution roots.
    pub(super) fn initialize(
        &self,
        runtime: &Runtime,
        arguments: &[Value],
        roots: &RootSet,
    ) -> Result<Option<BuiltinError>, RuntimeError> {
        match runtime.invoke_standard_builtin(
            self.entry_operation,
            if matches!(self.phase, Phase::Interface) {
                arguments
            } else {
                &[]
            },
        ) {
            Ok(array) => {
                self.set(runtime, roots, ARRAY, array)?;
                Ok(None)
            }
            Err(error) => Ok(Some(error)),
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
                    && (protocol == StandardTrait::Iterator
                        || contract.signature.params.first() == Some(&witness.receiver))
            })
            .ok_or_else(invalid)
    }
    fn iterator(&self, contract: &EngineNativeImport) -> Result<AbiType, RuntimeError> {
        let witness = self.witness(contract, StandardTrait::Iterable)?;
        witness
            .interface
            .associated_types
            .get(&associated_type_id(&witness.interface.declaration, "Iter"))
            .cloned()
            .ok_or_else(invalid)
    }
    fn optional(&self, contract: &EngineNativeImport) -> Result<AbiType, RuntimeError> {
        let witness = self.witness(contract, StandardTrait::Iterator)?;
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
    fn request(
        &mut self,
        runtime: &Runtime,
        owner: &LoadedModule,
        contract: &EngineNativeImport,
        roots: &RootSet,
        step: ProtocolStep,
    ) -> Result<NativeAction, RuntimeError> {
        match step {
            ProtocolStep::Call(request) => {
                self.phase = if matches!(self.phase, Phase::Iter) {
                    Phase::WaitingIter
                } else {
                    Phase::WaitingNext
                };
                Ok(NativeAction::Callback(request))
            }
            ProtocolStep::Value(value) => {
                self.phase = if matches!(self.phase, Phase::Iter) {
                    Phase::WaitingIter
                } else {
                    Phase::WaitingNext
                };
                self.receive(runtime, owner, contract, roots, value)
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
        let (slot, ty, next) = match self.phase {
            Phase::WaitingIter => (ITERATOR, self.iterator(contract)?, Phase::Begin),
            Phase::WaitingNext => (NEXT, self.optional(contract)?, Phase::Test),
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
            Phase::Iter => {
                let step = protocols::iter(
                    runtime,
                    owner,
                    self.witness(contract, StandardTrait::Iterable)?,
                    roots.get(0).ok_or_else(invalid)?,
                    &self.iterator(contract)?,
                )?;
                return self.request(runtime, owner, contract, roots, step);
            }
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
                    self.witness(contract, StandardTrait::Iterator)?,
                    self.get(roots, ITERATOR)?,
                    &self.optional(contract)?,
                )?;
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
                } else {
                    Phase::Close
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
                self.phase = if self.field.is_some() {
                    Phase::Index
                } else {
                    Phase::Append
                };
            }
            Phase::Index => self.phase = Phase::Select,
            Phase::Select => {
                let Value::Tuple(fields) = self.get(roots, ITEM)? else {
                    return Err(invalid());
                };
                let value = fields
                    .get(self.field.ok_or_else(invalid)?)
                    .cloned()
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
                    &self.iterator(contract)?,
                    IterOp::Close,
                )?;
                self.phase = Phase::End;
            }
            Phase::End => {
                self.guard.take();
                self.phase = Phase::Interface;
            }
            Phase::Interface => {
                return results::readonly_list(runtime, owner, contract, self.get(roots, ARRAY)?)
                    .map(NativeAction::Complete);
            }
            Phase::WaitingIter | Phase::WaitingNext => return Err(invalid()),
        }
        Ok(NativeAction::Continue)
    }
}
