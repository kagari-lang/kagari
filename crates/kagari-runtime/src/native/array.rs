//! Resumable array construction preserves callback order and logical execution cost.
use crate::{
    error::RuntimeError,
    native::{NativeAction, NativeContext, NativeInvocationState},
    native_value::{
        NativeValue,
        continuation::{NativeContinuation, NativeFn},
    },
    value::Value,
};
use kagari_abi::types::AbiType;

fn invalid() -> RuntimeError {
    RuntimeError::module_validation("array native argument or result")
}

pub(super) fn from_fn<T: NativeValue, R: NativeValue>(
    count: usize,
    make: NativeFn<usize, T>,
) -> NativeContinuation<R> {
    NativeContinuation::new(FromFn {
        make,
        count: count as u64,
        index: 0,
        phase: Phase::Allocate,
        more: false,
    })
}

// Keep the predecessor's logical step sequence, including the first allocation.
enum Phase {
    Allocate,
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
struct FromFn<T: NativeValue> {
    make: NativeFn<usize, T>,
    count: u64,
    index: u64,
    phase: Phase,
    more: bool,
}
impl<T: NativeValue> NativeInvocationState for FromFn<T> {
    fn advance(&mut self, context: &mut NativeContext<'_>) -> Result<NativeAction, RuntimeError> {
        match self.phase {
            Phase::Allocate => {
                let array = Value::Array(context.heap().alloc_array(vec![])?);
                context.retain(0, array)?;
                self.phase = Phase::Zero;
            }
            Phase::Zero => self.phase = Phase::Jump,
            Phase::Jump => self.phase = Phase::Compare,
            Phase::Compare => {
                self.more = self.index < self.count;
                self.phase = Phase::Branch;
            }
            Phase::Branch => {
                if !self.more {
                    return context
                        .retained(0)
                        .map(NativeAction::Complete)
                        .ok_or_else(invalid);
                }
                self.phase = Phase::Invoke;
            }
            Phase::Invoke => {
                let request = self
                    .make
                    .request(context, usize::try_from(self.index).map_err(|_| invalid())?)?;
                self.phase = Phase::Waiting;
                return Ok(NativeAction::Callback(request));
            }
            Phase::Waiting => return Err(invalid()),
            Phase::Append => {
                let Some(Value::Array(id)) = context.retained(0) else {
                    return Err(invalid());
                };
                context
                    .heap()
                    .array_push(id, context.retained(1).ok_or_else(invalid)?)?;
                context.retain(1, Value::Unit)?;
                self.phase = Phase::One;
            }
            Phase::One => self.phase = Phase::Add,
            Phase::Add => {
                self.index = self.index.checked_add(1).ok_or_else(invalid)?;
                self.phase = Phase::Move;
            }
            Phase::Move => self.phase = Phase::Jump,
        }
        Ok(NativeAction::Continue)
    }
    fn receive(
        &mut self,
        context: &mut NativeContext<'_>,
        value: Value,
    ) -> Result<NativeAction, RuntimeError> {
        let AbiType::Array(item, _) = &context.signature().result else {
            return Err(invalid());
        };
        if !matches!(self.phase, Phase::Waiting) || !context.matches(&value, item) {
            return Err(invalid());
        }
        context.retain(1, value)?;
        self.phase = Phase::Append;
        Ok(NativeAction::Continue)
    }
}
