//! Lazy cursors retain source values and checked targets, never a native borrow.
use crate::foundation::{
    enumerations::value::{self as enums, EnumSymbols, option},
    iteration::operations::increment,
};

use kagari_runtime::{
    error::{RuntimeError, RuntimeErrorKind},
    native::{
        binding::{Codec, NativeBinding, NativeResult},
        callable::StoredCallable,
        context::CallContext,
        functions::NativeFunction,
        scalar::NativeScalar,
        storage::{
            NativePayload, NativeStorage,
            scan::{NativeScanKind, ScanBudget, ScanStatus},
            value::StoredValue,
        },
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
    source: StoredValue,
    second: Option<StoredValue>,
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
        visit: &mut dyn FnMut(&'payload StoredValue),
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
    unsafe {
        NativeBinding::new_transport(vec![Codec::Value; arity], Codec::Value, move |cx| {
            construct(cx, operation)
        })
    }
}

pub(crate) fn next_binding(symbols: EnumSymbols) -> NativeBinding {
    // SAFETY: The cursor algorithm retains extracted children before callbacks
    // and returns the completed carrier directly to checked result publication.
    unsafe {
        (move |cx: &mut CallContext<'_>, receiver: ValueHandle<'_>| next(&symbols, cx, receiver))
            .binding_transport()
    }
}

fn invalid() -> RuntimeError {
    RuntimeError::module_validation("Iterable adapter contract")
}

fn construct(cx: &mut CallContext<'_>, operation: Operation) -> NativeResult<Value> {
    // SAFETY: The call frame retains both input sources and the callback through adapter publication.
    let receiver = unsafe { cx.argument_transport(0) }?;
    let callback = if matches!(
        operation,
        Operation::Map | Operation::Filter | Operation::FilterMap
    ) {
        // SAFETY: The call/receiver roots retain source and callback edges; item/result working
        // roots cover reentry.
        Some(unsafe { cx.callable(1)?.store_transport() })
    } else {
        None
    };
    let remaining = if matches!(operation, Operation::Take | Operation::Skip) {
        // SAFETY: The call frame retains both input sources and the callback through adapter publication.
        u64::decode(unsafe { cx.argument_transport(1) }?)?
    } else {
        0
    };
    // SAFETY: The call/receiver roots retain source and callback edges; item/result working roots
    // cover reentry.
    let source = unsafe { cx.call_values_transport(cx.selected_at(0)?, &[receiver]) }?;
    let source_root = unsafe {
        // SAFETY: Source/result roots protect extracted children; fresh callback or tuple results are rooted before the next callback.
        cx.heap().root_value_transport(source)
    }
    .ok_or_else(invalid)?;
    let second = if matches!(operation, Operation::Zip | Operation::Chain) {
        // SAFETY: The call frame retains both input sources and the callback through adapter publication.
        Some(unsafe { cx.call_values_transport(cx.selected_at(2)?, &[cx.argument_transport(1)?]) }?)
    } else {
        None
    };
    let second_root = second
        .map(|value| {
            unsafe {
                // SAFETY: Source/result roots protect extracted children; fresh callback or tuple results are rooted before the next callback.
                cx.heap().root_value_transport(value)
            }
            .ok_or_else(invalid)
        })
        .transpose()?;
    // Preserve completed source callbacks before result admission. Their temporary
    // roots stay outside the builder through payload scanning and allocation.
    // SAFETY: The call/receiver roots retain source and callback edges; item/result working roots
    // cover reentry.
    unsafe {
        cx.allocate_result_payload_transport(
            if second.is_some() { &[1, 3] } else { &[1] },
            |storage, _| {
                Ok(Adapter {
                    source: storage.store(&source_root)?,
                    second: second_root
                        .as_ref()
                        .map(|root| storage.store(root))
                        .transpose()?,
                    operation,
                    callback,
                    remaining: Cell::new(remaining),
                    switched: Cell::new(false),
                })
            },
        )
    }
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
                adapter.source.value_transport(),
                adapter.second.map(|value| value.value_transport()),
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
        // SAFETY: The call/receiver roots retain source and callback edges; item/result working
        // roots cover reentry.
        let result = unsafe { cx.call_stored_values_transport(0, slot, &[current]) }?;
        // SAFETY: result protects the Option until item is independently rooted below.
        let Some(item) = enums::read_option(
            symbols,
            cx,
            &unsafe { result.value_transport(cx.heap()) }.ok_or_else(invalid)?,
        )?
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
        let _item = unsafe {
            // SAFETY: Source/result roots protect extracted children; fresh callback or tuple results are rooted before the next callback.
            cx.heap().root_value_transport(item)
        }
        .ok_or_else(invalid)?;
        match operation {
            Operation::Map | Operation::FilterMap => {
                // SAFETY: The call/receiver roots retain source and callback edges; item/result
                // working roots cover reentry.
                let result = unsafe {
                    callback
                        .as_ref()
                        .ok_or_else(invalid)?
                        .call_values_transport(cx, &[item])
                }?;
                let _result = unsafe {
                    // SAFETY: Source/result roots protect extracted children; fresh callback or tuple results are rooted before the next callback.
                    cx.heap().root_value_transport(result)
                }
                .ok_or_else(invalid)?;
                if matches!(operation, Operation::Map) {
                    return option(symbols, cx, Some(result));
                }
                if let Some(value) = enums::read_option(symbols, cx, &result)? {
                    return option(symbols, cx, Some(value));
                }
            }
            Operation::Filter => {
                // SAFETY: The call/receiver roots retain source and callback edges; item/result
                // working roots cover reentry.
                if bool::decode(unsafe {
                    callback
                        .as_ref()
                        .ok_or_else(invalid)?
                        .call_values_transport(cx, &[item])
                }?)? {
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
                let pair = unsafe {
                    cx.heap()
                        .alloc_tuple_transport(vec![remaining.encode(), item])
                }?;
                let _pair = unsafe {
                    // SAFETY: Source/result roots protect extracted children; fresh callback or tuple results are rooted before the next callback.
                    cx.heap().root_value_transport(pair)
                }
                .ok_or_else(invalid)?;
                return option(symbols, cx, Some(pair));
            }
            Operation::Zip => {
                // SAFETY: The call/receiver roots retain source and callback edges; item/result
                // working roots cover reentry.
                let result = unsafe {
                    cx.call_stored_values_transport(0, 1, &[second.ok_or_else(invalid)?])
                }?;
                // SAFETY: result remains live through right publication in the pair.
                let Some(right) = enums::read_option(
                    symbols,
                    cx,
                    &unsafe { result.value_transport(cx.heap()) }.ok_or_else(invalid)?,
                )?
                else {
                    return option(symbols, cx, None);
                };
                let _right = unsafe {
                    // SAFETY: Source/result roots protect extracted children; fresh callback or tuple results are rooted before the next callback.
                    cx.heap().root_value_transport(right)
                }
                .ok_or_else(invalid)?;
                let pair = unsafe { cx.heap().alloc_tuple_transport(vec![item, right]) }?;
                let _pair = unsafe {
                    // SAFETY: Source/result roots protect extracted children; fresh callback or tuple results are rooted before the next callback.
                    cx.heap().root_value_transport(pair)
                }
                .ok_or_else(invalid)?;
                return option(symbols, cx, Some(pair));
            }
            Operation::Skip | Operation::Chain => return option(symbols, cx, Some(item)),
        }
    }
}
