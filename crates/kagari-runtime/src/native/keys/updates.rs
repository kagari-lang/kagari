//! Map callbacks preserve both lookups and publish only after checked insertion.
use super::{
    Buffers, KeySelection, RESULT, invalid,
    lookup::{KeyStep, Lookup},
};
use crate::{
    LoadedModule, Runtime, RuntimeError,
    gc::{CollectionIteration, RootSet},
    native::{NativeAction, callback},
    value::{EnumTag, Value},
};
use kagari_abi::{native_import::EngineNativeImport, standard::StandardIntrinsic};
enum Phase {
    Get,
    Test,
    Branch,
    EndPresent,
    ReadPresent,
    Callback,
    Waiting,
    EndMutation,
    Insert,
    Unit,
    Move,
    Jump,
}
pub(super) struct MapUpdate {
    operation: StandardIntrinsic,
    keys: KeySelection,
    buffers: Buffers,
    lookup: Lookup,
    phase: Phase,
    mutation: Option<CollectionIteration>,
    present: bool,
}
impl MapUpdate {
    pub(super) fn start(
        operation: StandardIntrinsic,
        keys: KeySelection,
        buffers: Buffers,
    ) -> Self {
        Self {
            operation,
            keys,
            buffers,
            lookup: Lookup::start(StandardIntrinsic::MapGet, keys, buffers, None),
            phase: Phase::Get,
            mutation: None,
            present: false,
        }
    }
    pub(super) fn initialize(
        &mut self,
        runtime: &Runtime,
        roots: &RootSet,
    ) -> Result<NativeAction, RuntimeError> {
        self.mutation = Some(
            runtime
                .gc()
                .begin_collection_mutation(&self.buffers.receiver(roots)?)?,
        );
        Ok(NativeAction::Continue)
    }
    pub(super) fn receive(
        &mut self,
        runtime: &Runtime,
        owner: &LoadedModule,
        contract: &EngineNativeImport,
        roots: &RootSet,
        value: Value,
    ) -> Result<NativeAction, RuntimeError> {
        if matches!(self.phase, Phase::Get | Phase::Insert) {
            return self.lookup.receive(runtime, owner, value);
        }
        if !matches!(self.phase, Phase::Waiting)
            || !runtime.matches_interface_method_abi(&value, &contract.signature.result, owner)
        {
            return Err(invalid());
        }
        self.buffers.set(runtime, roots, RESULT, value)?;
        self.phase = Phase::EndMutation;
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
            Phase::Get => match self.lookup.advance(runtime, owner, contract, roots)? {
                KeyStep::Action(action) => return Ok(action),
                KeyStep::Ready(value) => {
                    self.buffers.set(runtime, roots, RESULT, value)?;
                    self.phase = if self.operation == StandardIntrinsic::MapUpdate {
                        Phase::Callback
                    } else {
                        Phase::Test
                    };
                }
            },
            Phase::Test => {
                let Value::Enum(id) = self.buffers.get(roots, RESULT)? else {
                    return Err(invalid());
                };
                self.present =
                    runtime.gc().enum_snapshot(id).ok_or_else(invalid)?.tag == EnumTag::OptionSome;
                self.phase = Phase::Branch;
            }
            Phase::Branch => {
                self.phase = if self.present {
                    Phase::EndPresent
                } else {
                    Phase::Callback
                }
            }
            Phase::EndPresent => {
                self.mutation.take();
                self.phase = Phase::ReadPresent;
            }
            Phase::ReadPresent => {
                let Value::Enum(id) = self.buffers.get(roots, RESULT)? else {
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
                self.buffers.set(runtime, roots, RESULT, value)?;
                self.phase = Phase::Move;
            }
            Phase::Callback => {
                let args = if self.operation == StandardIntrinsic::MapUpdate {
                    vec![self.buffers.get(roots, RESULT)?]
                } else {
                    vec![]
                };
                let request = callback(
                    runtime,
                    &roots.get(2).ok_or_else(invalid)?,
                    &contract.signature.params[2],
                    args,
                )?;
                self.phase = Phase::Waiting;
                return Ok(NativeAction::Callback(request));
            }
            Phase::EndMutation => {
                self.mutation.take();
                self.lookup = Lookup::start(
                    StandardIntrinsic::MapInsert,
                    self.keys,
                    self.buffers,
                    Some(self.buffers.scratch + RESULT),
                );
                self.phase = Phase::Insert;
            }
            Phase::Insert => match self.lookup.advance(runtime, owner, contract, roots)? {
                KeyStep::Action(action) => return Ok(action),
                KeyStep::Ready(_) => self.phase = Phase::Unit,
            },
            Phase::Unit => {
                if self.operation == StandardIntrinsic::MapUpdate {
                    return Ok(NativeAction::Complete(self.buffers.get(roots, RESULT)?));
                }
                self.phase = Phase::Move;
            }
            Phase::Move => self.phase = Phase::Jump,
            Phase::Jump => return Ok(NativeAction::Complete(self.buffers.get(roots, RESULT)?)),
            Phase::Waiting => return Err(invalid()),
        }
        Ok(NativeAction::Continue)
    }
}
