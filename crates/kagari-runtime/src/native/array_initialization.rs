//! Per-index ArrayList construction with rooted callbacks and exact logical steps.
use crate::{
    LoadedModule, Runtime, RuntimeError,
    builtin::BuiltinError,
    gc::RootSet,
    native::{NativeAction, callback},
    value::Value,
};
use kagari_abi::{native_import::NativeSignature, standard::StandardIntrinsic, types::AbiType};

const ARRAY: usize = 0;
const ITEM: usize = 1;
pub(super) const SCRATCH_ROOTS: usize = 2;

#[derive(Clone, Copy)]
enum Phase {
    Zero,
    Jump,
    Compare,
    Branch,
    Invoke,
    Waiting,
    Append,
    One,
    Add,
    Move,
}

pub(super) struct ArrayInitialization {
    phase: Phase,
    count: u64,
    index: u64,
    more: bool,
    scratch: usize,
}

fn invalid() -> RuntimeError {
    RuntimeError::module_validation("native ArrayList initializer contract mismatch")
}

impl ArrayInitialization {
    pub(super) fn start(arguments: &[Value]) -> Result<Self, RuntimeError> {
        let Some(Value::U64(count)) = arguments.first() else {
            return Err(invalid());
        };
        Ok(Self {
            phase: Phase::Zero,
            count: *count,
            index: 0,
            more: false,
            scratch: arguments.len(),
        })
    }

    /// Allocation is the charged entry operation; roots must precede a resource failure.
    pub(super) fn initialize(
        &self,
        runtime: &Runtime,
        roots: &RootSet,
    ) -> Result<Option<BuiltinError>, RuntimeError> {
        match runtime.invoke_standard_builtin(StandardIntrinsic::ArrayListNew, &[]) {
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

    pub(super) fn advance(
        &mut self,
        runtime: &Runtime,
        signature: &NativeSignature,
        roots: &RootSet,
    ) -> Result<NativeAction, RuntimeError> {
        match self.phase {
            Phase::Zero => self.phase = Phase::Jump,
            Phase::Jump => self.phase = Phase::Compare,
            Phase::Compare => {
                self.more = self.index < self.count;
                self.phase = Phase::Branch;
            }
            Phase::Branch => {
                if !self.more {
                    return self.get(roots, ARRAY).map(NativeAction::Complete);
                }
                self.phase = Phase::Invoke;
            }
            Phase::Invoke => {
                let request = callback(
                    runtime,
                    &roots.get(1).ok_or_else(invalid)?,
                    &signature.params[1],
                    vec![Value::U64(self.index)],
                )?;
                self.phase = Phase::Waiting;
                return Ok(NativeAction::Callback(request));
            }
            Phase::Waiting => return Err(invalid()),
            Phase::Append => {
                if let Err(error) = runtime.invoke_standard_builtin(
                    StandardIntrinsic::ArrayPush,
                    &[self.get(roots, ARRAY)?, self.get(roots, ITEM)?],
                ) {
                    return Ok(NativeAction::BuiltinFailure(error));
                }
                self.phase = Phase::One;
            }
            Phase::One => self.phase = Phase::Add,
            Phase::Add => {
                // Invocations occur only below count, including count == usize::MAX.
                self.index = self.index.checked_add(1).ok_or_else(invalid)?;
                self.phase = Phase::Move;
            }
            Phase::Move => self.phase = Phase::Jump,
        }
        Ok(NativeAction::Continue)
    }

    pub(super) fn receive(
        &mut self,
        runtime: &Runtime,
        owner: &LoadedModule,
        signature: &NativeSignature,
        roots: &RootSet,
        value: Value,
    ) -> Result<NativeAction, RuntimeError> {
        let AbiType::Array(item, _) = &signature.result else {
            return Err(invalid());
        };
        if !matches!(self.phase, Phase::Waiting)
            || !runtime.matches_interface_method_abi(&value, item, owner)
        {
            return Err(invalid());
        }
        self.set(runtime, roots, ITEM, value)?;
        self.phase = Phase::Append;
        Ok(NativeAction::Continue)
    }
}
