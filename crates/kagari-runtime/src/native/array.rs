//! Array implementation policy belongs to this provider, never the generic driver.
use crate::{
    RuntimeError,
    native::{
        NativeAction, NativeContext, NativeInvocationState, NativeRegistration, NativeRegistry,
    },
    value::Value,
};
use kagari_abi::types::AbiType;
use kagari_stdlib_provider::contracts;

fn invalid() -> RuntimeError {
    RuntimeError::module_validation("array provider contract")
}
struct ReturnValue {
    wait: bool,
}
impl NativeInvocationState for ReturnValue {
    fn advance(&mut self, context: &mut NativeContext<'_>) -> Result<NativeAction, RuntimeError> {
        if self.wait {
            self.wait = false;
            return Ok(NativeAction::Continue);
        }
        context
            .retained(0)
            .map(NativeAction::Complete)
            .ok_or_else(invalid)
    }
}
fn direct(
    context: &mut NativeContext<'_>,
    value: Value,
) -> Result<Box<dyn NativeInvocationState>, RuntimeError> {
    context.retain(0, value)?;
    Ok(Box::new(ReturnValue { wait: false }))
}
pub(crate) fn install(registry: &mut NativeRegistry) {
    for (name, contract) in contracts() {
        let entry = match name {
            "array_new" => NativeRegistration::new(contract, 1, |context| {
                let array = Value::Array(context.heap().alloc_array(vec![])?);
                direct(context, array)
            }),
            "array_len" => NativeRegistration::new(contract, 1, |context| {
                let Some(Value::Array(id)) = context.argument(0) else {
                    return Err(invalid());
                };
                let length = context.heap().array_len(id).ok_or_else(invalid)?;
                direct(context, Value::U64(length as u64))
            }),
            "array_push" => NativeRegistration::new(contract, 1, |context| {
                let Some(Value::Array(id)) = context.argument(0) else {
                    return Err(invalid());
                };
                context
                    .heap()
                    .array_push(id, context.argument(1).ok_or_else(invalid)?)?;
                context.retain(0, Value::Unit)?;
                Ok(Box::new(ReturnValue { wait: true }))
            }),
            "array_from_fn" => NativeRegistration::new(contract, 2, |context| {
                let Some(Value::U64(count)) = context.argument(0) else {
                    return Err(invalid());
                };
                Ok(Box::new(FromFn {
                    count,
                    index: 0,
                    phase: Phase::Allocate,
                    more: false,
                }))
            }),
            _ => unreachable!("standard provider descriptor has no implementation"),
        };
        registry
            .install(entry)
            .expect("unique bundled native registration");
    }
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
struct FromFn {
    count: u64,
    index: u64,
    phase: Phase,
    more: bool,
}
impl NativeInvocationState for FromFn {
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
                let signature = &context.signature().params[1];
                let request = context.callback(
                    &context.argument(1).ok_or_else(invalid)?,
                    signature,
                    vec![Value::U64(self.index)],
                )?;
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
