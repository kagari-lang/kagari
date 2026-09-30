//! Concrete array preparation followed by one existing atomic storage commit.
mod dedup;
mod sorting;
use crate::{
    LoadedModule, Runtime, RuntimeError,
    gc::{CollectionIteration, RootSet},
    native::{
        NativeAction,
        prepared_arrays::{dedup::Dedup, sorting::Sorting},
    },
    value::Value,
};
use kagari_abi::{
    native_import::{EngineNativeImport, NativeWitness},
    standard::{StandardIntrinsic, traits::StandardTrait},
    types::AbiType,
};

const VALUES: usize = 0;
const OUTPUT: usize = 1;
const ITERATOR: usize = 2;
const LEFT: usize = 3;
const RIGHT: usize = 4;
const RESULT: usize = 5;
pub(super) const SCRATCH_ROOTS: usize = 6;

fn invalid() -> RuntimeError {
    RuntimeError::module_validation("native prepared array contract mismatch")
}

#[derive(Clone, Copy)]
struct Buffers {
    scratch: usize,
}
impl Buffers {
    fn get(self, roots: &RootSet, slot: usize) -> Result<Value, RuntimeError> {
        roots.get(self.scratch + slot).ok_or_else(invalid)
    }
    fn set(
        self,
        runtime: &Runtime,
        roots: &RootSet,
        slot: usize,
        value: Value,
    ) -> Result<(), RuntimeError> {
        roots
            .set(runtime.gc(), self.scratch + slot, value)
            .ok_or_else(invalid)
    }
    fn read(
        self,
        runtime: &Runtime,
        roots: &RootSet,
        source: Value,
        index: u64,
        slot: usize,
    ) -> Result<(), RuntimeError> {
        let Value::Array(id) = source else {
            return Err(invalid());
        };
        let value = runtime
            .gc()
            .array_get(id, usize::try_from(index).map_err(|_| invalid())?)
            .ok_or_else(invalid)?;
        self.set(runtime, roots, slot, value)
    }
    fn new_array(
        self,
        runtime: &Runtime,
        roots: &RootSet,
        slot: usize,
    ) -> Result<Option<NativeAction>, RuntimeError> {
        match runtime.invoke_standard_builtin(StandardIntrinsic::ArrayListNew, &[]) {
            Ok(value) => {
                self.set(runtime, roots, slot, value)?;
                Ok(None)
            }
            Err(error) => Ok(Some(NativeAction::BuiltinFailure(error))),
        }
    }
    fn push(
        self,
        runtime: &Runtime,
        roots: &RootSet,
        slot: usize,
        value: Value,
    ) -> Result<Option<NativeAction>, RuntimeError> {
        Ok(runtime
            .invoke_standard_builtin(
                StandardIntrinsic::ArrayPush,
                &[self.get(roots, slot)?, value],
            )
            .err()
            .map(NativeAction::BuiltinFailure))
    }
    fn field(
        self,
        runtime: &Runtime,
        roots: &RootSet,
        slot: usize,
        field: usize,
    ) -> Result<(), RuntimeError> {
        let Value::Tuple(values) = self.get(roots, slot)? else {
            return Err(invalid());
        };
        self.set(
            runtime,
            roots,
            slot,
            values.get(field).cloned().ok_or_else(invalid)?,
        )
    }
}

fn witness<'a>(
    contract: &'a EngineNativeImport,
    ty: &AbiType,
    protocol: StandardTrait,
) -> Result<&'a NativeWitness, RuntimeError> {
    contract
        .witnesses
        .iter()
        .find(|witness| {
            &witness.receiver == ty
                && StandardTrait::from_id(&witness.interface.declaration) == Some(protocol)
        })
        .ok_or_else(invalid)
}

enum PreparationStep {
    Action(NativeAction),
    Ready(usize),
}
enum Preparation {
    Sorting(Sorting),
    Dedup(Dedup),
}
enum Phase {
    Prepare,
    EndMutation,
    Commit,
}
pub(super) struct PreparedArray {
    operation: StandardIntrinsic,
    buffers: Buffers,
    preparation: Preparation,
    mutation: Option<CollectionIteration>,
    phase: Phase,
    output: usize,
}
impl PreparedArray {
    pub(super) fn start(
        operation: StandardIntrinsic,
        arguments: &[Value],
    ) -> Result<Self, RuntimeError> {
        let preparation = match operation {
            StandardIntrinsic::ArraySort
            | StandardIntrinsic::ArraySortBy
            | StandardIntrinsic::ArraySortByKey => Preparation::Sorting(Sorting::start(operation)),
            StandardIntrinsic::ArrayDedup => Preparation::Dedup(Dedup::start()),
            _ => return Err(invalid()),
        };
        Ok(Self {
            operation,
            buffers: Buffers {
                scratch: arguments.len(),
            },
            preparation,
            mutation: None,
            phase: Phase::Prepare,
            output: VALUES,
        })
    }
    /// Entry already charged the old guard acquisition; root arguments first.
    pub(super) fn initialize(
        &mut self,
        runtime: &Runtime,
        roots: &RootSet,
    ) -> Result<(), RuntimeError> {
        self.mutation = Some(
            runtime
                .gc()
                .begin_collection_mutation(&roots.get(0).ok_or_else(invalid)?)?,
        );
        Ok(())
    }
    pub(super) fn receive(
        &mut self,
        runtime: &Runtime,
        owner: &LoadedModule,
        contract: &EngineNativeImport,
        roots: &RootSet,
        value: Value,
    ) -> Result<NativeAction, RuntimeError> {
        if !matches!(self.phase, Phase::Prepare) {
            return Err(invalid());
        }
        match &mut self.preparation {
            Preparation::Sorting(state) => {
                state.receive(runtime, owner, contract, roots, self.buffers, value)
            }
            Preparation::Dedup(state) => state.receive(runtime, owner, value),
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
            Phase::Prepare => {
                let step = match &mut self.preparation {
                    Preparation::Sorting(state) => {
                        state.advance(runtime, owner, contract, roots, self.buffers)?
                    }
                    Preparation::Dedup(state) => {
                        state.advance(runtime, owner, contract, roots, self.buffers)?
                    }
                };
                match step {
                    PreparationStep::Action(action) => return Ok(action),
                    PreparationStep::Ready(slot) => {
                        self.output = slot;
                        self.phase = Phase::EndMutation;
                    }
                }
            }
            Phase::EndMutation => {
                self.mutation.take();
                self.phase = Phase::Commit;
            }
            Phase::Commit => {
                return Ok(
                    match runtime.invoke_standard_builtin(
                        if self.operation == StandardIntrinsic::ArrayDedup {
                            StandardIntrinsic::CollectionRetainStorage
                        } else {
                            StandardIntrinsic::ArrayReplaceStorage
                        },
                        &[
                            roots.get(0).ok_or_else(invalid)?,
                            self.buffers.get(roots, self.output)?,
                        ],
                    ) {
                        Ok(value) => NativeAction::Complete(value),
                        Err(error) => NativeAction::BuiltinFailure(error),
                    },
                );
            }
        }
        Ok(NativeAction::Continue)
    }
}
