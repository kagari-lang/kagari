//! Lazy cursors retain source values and checked targets, never a native borrow.
use crate::bindings::{enums, option};
use kagari_runtime::{
    error::{RuntimeError, RuntimeErrorKind},
    native::{
        binding::{Codec, NativeBinding, NativeResult},
        callable::StoredCallable,
        context::CallContext,
        functions::NativeFunction,
        scalar::NativeScalar,
        storage::{NativePayload, NativeStorage},
        views::ValueHandle,
    },
    value::Value,
};
use std::cell::Cell;

#[derive(Debug, Clone, Copy)]
pub(crate) enum Operation {
    Map,
    Filter,
    Take,
    FilterMap,
    Skip,
    Enumerate,
    Zip,
    Chain,
}

#[derive(Debug)]
struct Adapter {
    source: Value,
    second: Option<Value>,
    operation: Operation,
    callback: Option<StoredCallable>,
    remaining: Cell<u64>,
    switched: Cell<bool>,
}

impl NativePayload for Adapter {
    fn trace<'payload>(&'payload self, visit: &mut dyn FnMut(&'payload Value)) {
        if let Some(callback) = &self.callback {
            callback.trace(visit);
        }
        visit(&self.source);
        if let Some(second) = &self.second {
            visit(second);
        }
    }

    fn units(&self) -> usize {
        5
    }
}

pub(crate) fn storage() -> NativeStorage {
    NativeStorage::payload::<Adapter>()
}

#[derive(Clone, Copy)]
pub(crate) enum Terminal {
    Count,
    Find,
    Any,
    All,
    ForEach,
}

pub(crate) fn adapter_binding(operation: Operation) -> NativeBinding {
    let arity = if matches!(operation, Operation::Enumerate) {
        1
    } else {
        2
    };
    NativeBinding::new(vec![Codec::Value; arity], Codec::Value, move |cx| {
        construct(cx, operation)
    })
}

pub(crate) fn terminal_binding(terminal: Terminal) -> NativeBinding {
    let arity = if matches!(terminal, Terminal::Count) {
        1
    } else {
        2
    };
    NativeBinding::new(vec![Codec::Value; arity], Codec::Value, move |cx| {
        consume(cx, terminal)
    })
}

pub(crate) fn next_binding() -> NativeBinding {
    next.binding()
}

pub(crate) fn identity(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    cx.argument(0)
}

pub(crate) fn destination(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    cx.call_values(cx.selected_at(0)?, &[cx.argument(0)?])
}

pub(crate) fn fold(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    let callback = cx.callable(2)?.store();
    let mut accumulator = cx.argument(1)?;
    let root = cx.heap().root_value(accumulator).ok_or_else(invalid)?;
    let cursor = cx.call_values(cx.selected_at(0)?, &[cx.argument(0)?])?;
    let _cursor = cx.heap().root_value(cursor).ok_or_else(invalid)?;
    loop {
        cx.poll()?;
        let result = cx.call_values(cx.selected_at(1)?, &[cursor])?;
        let Some(item) = enums::option(cx, &result)? else {
            return Ok(accumulator);
        };
        let _item = cx.heap().root_value(item).ok_or_else(invalid)?;
        accumulator = callback.call_values(cx, &[accumulator, item])?;
        root.set(cx.heap(), accumulator).ok_or_else(invalid)?;
    }
}

fn consume(cx: &mut CallContext<'_>, terminal: Terminal) -> NativeResult<Value> {
    let callback = if matches!(terminal, Terminal::Count) {
        None
    } else {
        Some(cx.callable(1)?.store())
    };
    let cursor = cx.call_values(cx.selected_at(0)?, &[cx.argument(0)?])?;
    let _cursor = cx.heap().root_value(cursor).ok_or_else(invalid)?;
    let mut count = 0u64;
    loop {
        cx.poll()?;
        let result = cx.call_values(cx.selected_at(1)?, &[cursor])?;
        let Some(item) = enums::option(cx, &result)? else {
            return match terminal {
                Terminal::Count => Ok(count.encode()),
                Terminal::Find => option(cx, None),
                Terminal::Any => Ok(false.encode()),
                Terminal::All => Ok(true.encode()),
                Terminal::ForEach => Ok(Value::Unit),
            };
        };
        let _item = cx.heap().root_value(item).ok_or_else(invalid)?;
        if matches!(terminal, Terminal::Count) {
            count = increment(count)?;
            continue;
        }
        let result = callback
            .as_ref()
            .ok_or_else(invalid)?
            .call_values(cx, &[item])?;
        match terminal {
            Terminal::Find if bool::decode(result)? => return option(cx, Some(item)),
            Terminal::Any if bool::decode(result)? => return Ok(true.encode()),
            Terminal::All if !bool::decode(result)? => return Ok(false.encode()),
            _ => {}
        }
    }
}

fn invalid() -> RuntimeError {
    RuntimeError::module_validation("Iterable adapter contract")
}

fn increment(value: u64) -> NativeResult<u64> {
    value
        .checked_add(1)
        .ok_or_else(|| RuntimeError::new(RuntimeErrorKind::ScriptTrap, "integer overflow"))
}

fn construct(cx: &mut CallContext<'_>, operation: Operation) -> NativeResult<Value> {
    let receiver = cx.argument(0)?;
    let callback = if matches!(
        operation,
        Operation::Map | Operation::Filter | Operation::FilterMap
    ) {
        Some(cx.callable(1)?.store())
    } else {
        None
    };
    let remaining = if matches!(operation, Operation::Take | Operation::Skip) {
        u64::decode(cx.argument(1)?)?
    } else {
        0
    };
    let source = cx.call_values(cx.selected_at(0)?, &[receiver])?;
    let _source = cx.heap().root_value(source).ok_or_else(invalid)?;
    let second = if matches!(operation, Operation::Zip | Operation::Chain) {
        Some(cx.call_values(cx.selected_at(2)?, &[cx.argument(1)?])?)
    } else {
        None
    };
    let _second = second
        .map(|value| cx.heap().root_value(value).ok_or_else(invalid))
        .transpose()?;
    cx.allocate_result_payload(
        Adapter {
            source,
            second,
            operation,
            callback,
            remaining: Cell::new(remaining),
            switched: Cell::new(false),
        },
        if second.is_some() { &[1, 3] } else { &[1] },
    )
}

fn next(cx: &mut CallContext<'_>, receiver: ValueHandle<'_>) -> NativeResult<Value> {
    let Some(_active) = receiver.try_enter_operation()? else {
        return Err(RuntimeError::new(
            RuntimeErrorKind::ScriptTrap,
            "recursive next on the same lazy iterator",
        ));
    };
    let (source, second, operation, callback, mut remaining, switched) = receiver
        .with_payload::<Adapter, _>(|adapter| {
            Ok((
                adapter.source,
                adapter.second,
                adapter.operation,
                adapter.callback.clone(),
                adapter.remaining.get(),
                adapter.switched.get(),
            ))
        })?;
    if matches!(operation, Operation::Take) && remaining == 0 {
        return option(cx, None);
    }
    let (mut current, mut slot) = if switched {
        (second.ok_or_else(invalid)?, 1)
    } else {
        (source, 0)
    };
    loop {
        cx.poll()?;
        let result = cx.call_stored_values(0, slot, &[current])?;
        let Some(item) = enums::option(cx, &result)? else {
            if matches!(operation, Operation::Chain) && slot == 0 {
                receiver.with_payload::<Adapter, _>(|adapter| {
                    adapter.switched.set(true);
                    Ok(())
                })?;
                current = second.ok_or_else(invalid)?;
                slot = 1;
                continue;
            }
            return option(cx, None);
        };
        let _item = cx.heap().root_value(item).ok_or_else(invalid)?;
        match operation {
            Operation::Map | Operation::FilterMap => {
                let result = callback
                    .as_ref()
                    .ok_or_else(invalid)?
                    .call_values(cx, &[item])?;
                let _result = cx.heap().root_value(result).ok_or_else(invalid)?;
                if matches!(operation, Operation::Map) {
                    return option(cx, Some(result));
                }
                if let Some(value) = enums::option(cx, &result)? {
                    return option(cx, Some(value));
                }
            }
            Operation::Filter => {
                if bool::decode(
                    callback
                        .as_ref()
                        .ok_or_else(invalid)?
                        .call_values(cx, &[item])?,
                )? {
                    return option(cx, Some(item));
                }
            }
            Operation::Take => {
                receiver.with_payload::<Adapter, _>(|adapter| {
                    adapter.remaining.set(remaining - 1);
                    Ok(())
                })?;
                return option(cx, Some(item));
            }
            Operation::Skip if remaining > 0 => {
                remaining -= 1;
                receiver.with_payload::<Adapter, _>(|adapter| {
                    adapter.remaining.set(remaining);
                    Ok(())
                })?;
            }
            Operation::Enumerate => {
                let following = increment(remaining)?;
                receiver.with_payload::<Adapter, _>(|adapter| {
                    adapter.remaining.set(following);
                    Ok(())
                })?;
                let pair = cx.heap().alloc_tuple(vec![remaining.encode(), item])?;
                let _pair = cx.heap().root_value(pair).ok_or_else(invalid)?;
                return option(cx, Some(pair));
            }
            Operation::Zip => {
                let result = cx.call_stored_values(0, 1, &[second.ok_or_else(invalid)?])?;
                let Some(right) = enums::option(cx, &result)? else {
                    return option(cx, None);
                };
                let _right = cx.heap().root_value(right).ok_or_else(invalid)?;
                let pair = cx.heap().alloc_tuple(vec![item, right])?;
                let _pair = cx.heap().root_value(pair).ok_or_else(invalid)?;
                return option(cx, Some(pair));
            }
            Operation::Skip | Operation::Chain => return option(cx, Some(item)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::increment;
    use kagari_runtime::error::RuntimeErrorKind;

    #[test]
    fn logical_counts_use_the_full_u64_domain() {
        assert_eq!(increment(u64::from(u32::MAX)).unwrap(), 1u64 << 32);
        assert_eq!(increment(u64::MAX - 1).unwrap(), u64::MAX);
        assert_eq!(
            increment(u64::MAX).unwrap_err().kind(),
            RuntimeErrorKind::ScriptTrap
        );
    }
}
