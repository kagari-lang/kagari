//! Lazy cursors retain source values and checked targets, never a native borrow.
use crate::foundation::{
    enumerations::value::{self as enums, EnumSymbols, option},
    iteration::operations::increment,
};
use kagari_runtime::native::storage::scan::{NativeScanKind, ScanBudget, ScanStatus};

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
    resource::HeapCharge,
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

unsafe impl NativePayload for Adapter {
    type ScanState = ();
    const SCAN_KIND: NativeScanKind = NativeScanKind::Bounded { max_work: 8 };

    fn begin_scan(&self) {}

    fn scan<'payload>(
        &'payload self,
        _: &mut Self::ScanState,
        budget: &mut ScanBudget,
        visit: &mut dyn FnMut(&'payload Value),
    ) -> ScanStatus {
        budget.charge(2);
        if let Some(callback) = &self.callback {
            callback.scan(&mut (), budget, visit);
        }
        visit(&self.source);
        if let Some(second) = &self.second {
            visit(second);
        }

        ScanStatus::Complete
    }

    fn charge(&self) -> HeapCharge {
        HeapCharge::new(5, 0)
    }
}

pub(crate) fn storage() -> NativeStorage {
    NativeStorage::payload::<Adapter>()
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

pub(crate) fn next_binding(symbols: EnumSymbols) -> NativeBinding {
    (move |cx: &mut CallContext<'_>, receiver: ValueHandle<'_>| next(&symbols, cx, receiver))
        .binding()
}

fn invalid() -> RuntimeError {
    RuntimeError::module_validation("Iterable adapter contract")
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
    // Preserve completed source callbacks before result admission. Their temporary
    // roots stay outside the builder through payload scanning and allocation.
    let payload = Adapter {
        source,
        second,
        operation,
        callback,
        remaining: Cell::new(remaining),
        switched: Cell::new(false),
    };
    cx.allocate_result_payload(if second.is_some() { &[1, 3] } else { &[1] }, |_, _| {
        Ok(payload)
    })
}

fn next(
    symbols: &EnumSymbols,
    cx: &mut CallContext<'_>,
    receiver: ValueHandle<'_>,
) -> NativeResult<Value> {
    let Some(_active) = receiver.try_enter_operation()? else {
        return Err(RuntimeError::new(
            RuntimeErrorKind::ScriptTrap,
            "recursive next on the same lazy iterator",
        ));
    };
    let (source, second, operation, callback, mut remaining, switched) = unsafe {
        receiver.with_payload::<Adapter, _>(Some(&_active), |adapter| {
            Ok((
                adapter.source,
                adapter.second,
                adapter.operation,
                adapter.callback.clone(),
                adapter.remaining.get(),
                adapter.switched.get(),
            ))
        })
    }?;
    if matches!(operation, Operation::Take) && remaining == 0 {
        return option(symbols, cx, None);
    }
    let (mut current, mut slot) = if switched {
        (second.ok_or_else(invalid)?, 1)
    } else {
        (source, 0)
    };
    loop {
        cx.poll()?;
        let result = cx.call_stored_values(0, slot, &[current])?;
        let Some(item) =
            enums::read_option(symbols, cx, &result.value(cx.heap()).ok_or_else(invalid)?)?
        else {
            if matches!(operation, Operation::Chain) && slot == 0 {
                unsafe {
                    receiver.with_payload::<Adapter, _>(Some(&_active), |adapter| {
                        adapter.switched.set(true);
                        Ok(())
                    })
                }?;
                current = second.ok_or_else(invalid)?;
                slot = 1;
                continue;
            }
            return option(symbols, cx, None);
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
                    return option(symbols, cx, Some(result));
                }
                if let Some(value) = enums::read_option(symbols, cx, &result)? {
                    return option(symbols, cx, Some(value));
                }
            }
            Operation::Filter => {
                if bool::decode(
                    callback
                        .as_ref()
                        .ok_or_else(invalid)?
                        .call_values(cx, &[item])?,
                )? {
                    return option(symbols, cx, Some(item));
                }
            }
            Operation::Take => {
                unsafe {
                    receiver.with_payload::<Adapter, _>(Some(&_active), |adapter| {
                        adapter.remaining.set(remaining - 1);
                        Ok(())
                    })
                }?;
                return option(symbols, cx, Some(item));
            }
            Operation::Skip if remaining > 0 => {
                remaining -= 1;
                unsafe {
                    receiver.with_payload::<Adapter, _>(Some(&_active), |adapter| {
                        adapter.remaining.set(remaining);
                        Ok(())
                    })
                }?;
            }
            Operation::Enumerate => {
                let following = increment(remaining)?;
                unsafe {
                    receiver.with_payload::<Adapter, _>(Some(&_active), |adapter| {
                        adapter.remaining.set(following);
                        Ok(())
                    })
                }?;
                let pair = cx.heap().alloc_tuple(vec![remaining.encode(), item])?;
                let _pair = cx.heap().root_value(pair).ok_or_else(invalid)?;
                return option(symbols, cx, Some(pair));
            }
            Operation::Zip => {
                let result = cx.call_stored_values(0, 1, &[second.ok_or_else(invalid)?])?;
                let Some(right) =
                    enums::read_option(symbols, cx, &result.value(cx.heap()).ok_or_else(invalid)?)?
                else {
                    return option(symbols, cx, None);
                };
                let _right = cx.heap().root_value(right).ok_or_else(invalid)?;
                let pair = cx.heap().alloc_tuple(vec![item, right])?;
                let _pair = cx.heap().root_value(pair).ok_or_else(invalid)?;
                return option(symbols, cx, Some(pair));
            }
            Operation::Skip | Operation::Chain => return option(symbols, cx, Some(item)),
        }
    }
}
