//! Lazy cursors retain source values and checked targets, never a native borrow.
use crate::foundation::{
    enumerations::value::{self as enums, EnumSymbols, option},
    iteration::operations::increment,
};
use kagari_runtime::{
    error::{RuntimeError, trap::TrapReason},
    native::{
        binding::{Codec, NativeBinding, NativeResult},
        callable::StoredCallable,
        context::NativeCall,
        scalar::NativeScalar,
        storage::{NativePayload, NativeStorage},
        views::ValueHandle,
    },
    value::{carriers::StoredValue, view::ValueView},
};

use kagari_gc::{
    error::GcError,
    object::{Edge, GcObject, ObjectCharge},
    scan::{EdgeVisitor, ScanBudget, ScanKind, ScanStatus},
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
    callback: Option<Edge<StoredCallable>>,
    remaining: Cell<u64>,
    switched: Cell<bool>,
}

impl NativePayload for Adapter {
    fn registration_storage() -> NativeStorage {
        storage()
    }
}

// SAFETY: all script fields are initialized inside NativeInit and never replaced.
// The scanner visits the two stored values and optional callback edge. Mutable
// counters are scalar Cells; Drop owns no peer access or indirect allocation.
unsafe impl GcObject for Adapter {
    type ScanState = usize;
    const SCAN_KIND: ScanKind = ScanKind::Resumable;

    fn begin_scan(&self) -> usize {
        0
    }

    fn scan(
        &self,
        next: &mut usize,
        budget: &mut ScanBudget,
        visitor: &mut EdgeVisitor<'_>,
    ) -> Result<ScanStatus, GcError> {
        while *next < 3 {
            if !budget.take(1) {
                return Ok(ScanStatus::Pending);
            }
            // SAFETY: these are this admitted provider's initialized immutable
            // fields. Pending visits retry the same field before advancing.
            let status = unsafe {
                match *next {
                    0 => self.source.scan(budget, visitor)?,
                    1 => match &self.second {
                        Some(value) => value.scan(budget, visitor)?,
                        None => ScanStatus::Complete,
                    },
                    _ => match &self.callback {
                        Some(callback) => visitor.try_visit_stored(callback, budget)?,
                        None => ScanStatus::Complete,
                    },
                }
            };
            if status == ScanStatus::Pending {
                return Ok(status);
            }
            *next += 1;
        }
        Ok(ScanStatus::Complete)
    }

    fn charge(&self) -> ObjectCharge {
        ObjectCharge { bytes: 0, units: 5 }
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
    NativeBinding::new(vec![Codec::Value], Codec::Value, move |cx| {
        let receiver = cx.value_handle(0)?;
        next(&symbols, cx, receiver)
    })
}

fn invalid() -> RuntimeError {
    RuntimeError::module_validation("Iterable adapter contract")
}

fn construct<'call>(
    cx: &mut NativeCall<'call>,
    operation: Operation,
) -> NativeResult<ValueView<'call>> {
    let receiver = cx.argument(0)?;
    let callback = if matches!(
        operation,
        Operation::Map | Operation::Filter | Operation::FilterMap
    ) {
        Some(cx.callable(1)?)
    } else {
        None
    };
    let remaining = if matches!(operation, Operation::Take | Operation::Skip) {
        u64::decode(cx.argument(1)?)?
    } else {
        0
    };
    let source = cx.call_values(cx.selected_at(0)?, &[receiver])?;
    let second = if matches!(operation, Operation::Zip | Operation::Chain) {
        {
            let argument = cx.argument(1)?;
            Some(cx.call_values(cx.selected_at(2)?, &[argument])?)
        }
    } else {
        None
    };
    let slots = if second.is_some() {
        &[1, 3][..]
    } else {
        &[1][..]
    };
    cx.allocate_result_with(slots, move |init| {
        let source = init.value(&source)?;
        let second = second.as_ref().map(|value| init.value(value)).transpose()?;
        let callback = callback
            .as_ref()
            .map(|callback| callback.store(init))
            .transpose()?;
        // SAFETY: every admitted source/callback is stored in its matching field
        // and scanned by Adapter before the initialized provider is published.
        unsafe {
            init.scope().alloc_with(|mut destination| {
                Ok(Adapter {
                    source: StoredValue::initialize(&mut destination, source)?,
                    second: second
                        .map(|value| StoredValue::initialize(&mut destination, value))
                        .transpose()?,
                    callback: callback
                        .map(|value| destination.edge_scoped(value))
                        .transpose()?,
                    operation,
                    remaining: Cell::new(remaining),
                    switched: Cell::new(false),
                })
            })
        }
    })
}

fn next<'call>(
    symbols: &EnumSymbols,
    cx: &mut NativeCall<'call>,
    receiver: ValueHandle<'_>,
) -> NativeResult<ValueView<'call>> {
    receiver
        .with_operation(cx, |cx| next_active(symbols, cx, &receiver))?
        .ok_or_else(|| {
            RuntimeError::from(TrapReason::Provider(
                "recursive next on the same lazy iterator".into(),
            ))
        })
}

fn next_active<'call>(
    symbols: &EnumSymbols,
    cx: &mut NativeCall<'call>,
    receiver: &ValueHandle<'_>,
) -> NativeResult<ValueView<'call>> {
    let (operation, mut remaining, switched, has_second) =
        receiver.with_payload::<Adapter, _>(cx, |adapter| {
            Ok((
                adapter.operation,
                adapter.remaining.get(),
                adapter.switched.get(),
                adapter.second.is_some(),
            ))
        })?;
    let value = receiver.value(cx)?;
    let source = cx.value_payload::<Adapter>(&value, |cx, adapter| {
        StoredValue::get_field(cx, adapter, |adapter| Some(&adapter.source))
    })?;
    let second = if has_second {
        Some(cx.value_payload::<Adapter>(&value, |cx, adapter| {
            StoredValue::get_field(cx, adapter, |adapter| adapter.second.as_ref())
        })?)
    } else {
        None
    };
    let callback = cx.callable_payload::<Adapter>(&value, |cx, adapter| {
        cx.follow(adapter, |adapter| adapter.callback.as_ref())
    })?;
    if matches!(operation, Operation::Take) && remaining == 0 {
        return option(symbols, cx, None);
    }
    let (mut current, mut slot) = if switched {
        (second.ok_or_else(invalid)?, 1)
    } else {
        (source, 0)
    };
    loop {
        let completed = cx.with_optional_value_temporaries(|cx| {
            let result = cx.call_stored_values(0, slot, &[current])?;
            let Some(item) = enums::read_option(symbols, cx, &result)? else {
                if matches!(operation, Operation::Chain) && slot == 0 {
                    receiver.with_payload::<Adapter, _>(cx, |adapter| {
                        adapter.switched.set(true);
                        Ok(())
                    })?;
                    current = second.ok_or_else(invalid)?;
                    slot = 1;
                    return Ok(None);
                }
                return option(symbols, cx, None).map(Some);
            };
            match operation {
                Operation::Map | Operation::FilterMap => {
                    let result = callback
                        .as_ref()
                        .ok_or_else(invalid)?
                        .call_values(cx, &[item])?;
                    if matches!(operation, Operation::Map) {
                        return option(symbols, cx, Some(result)).map(Some);
                    }
                    if let Some(value) = enums::read_option(symbols, cx, &result)? {
                        return option(symbols, cx, Some(value)).map(Some);
                    }
                }
                Operation::Filter => {
                    if bool::decode(
                        callback
                            .as_ref()
                            .ok_or_else(invalid)?
                            .call_values(cx, &[item])?,
                    )? {
                        return option(symbols, cx, Some(item)).map(Some);
                    }
                }
                Operation::Take => {
                    receiver.with_payload::<Adapter, _>(cx, |adapter| {
                        adapter.remaining.set(remaining - 1);
                        Ok(())
                    })?;
                    return option(symbols, cx, Some(item)).map(Some);
                }
                Operation::Skip if remaining > 0 => {
                    remaining -= 1;
                    receiver.with_payload::<Adapter, _>(cx, |adapter| {
                        adapter.remaining.set(remaining);
                        Ok(())
                    })?;
                }
                Operation::Enumerate => {
                    let following = increment(remaining)?;
                    receiver.with_payload::<Adapter, _>(cx, |adapter| {
                        adapter.remaining.set(following);
                        Ok(())
                    })?;
                    let pair = cx.allocate_tuple(&[remaining.encode(), item])?;
                    return option(symbols, cx, Some(pair)).map(Some);
                }
                Operation::Zip => {
                    let result = cx.call_stored_values(0, 1, &[second.ok_or_else(invalid)?])?;
                    let Some(right) = enums::read_option(symbols, cx, &result)? else {
                        return option(symbols, cx, None).map(Some);
                    };
                    let pair = cx.allocate_tuple(&[item, right])?;
                    return option(symbols, cx, Some(pair)).map(Some);
                }
                Operation::Skip | Operation::Chain => {
                    return option(symbols, cx, Some(item)).map(Some);
                }
            }
            Ok(None)
        })?;
        if let Some(value) = completed {
            return Ok(value);
        }
    }
}
