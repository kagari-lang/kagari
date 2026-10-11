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
enum Operation {
    Map,
    Filter,
    Take,
}

#[derive(Debug)]
struct Adapter {
    source: Value,
    operation: Operation,
    callback: Option<StoredCallable>,
    remaining: Cell<usize>,
}

impl NativePayload for Adapter {
    fn trace<'payload>(&'payload self, visit: &mut dyn FnMut(&'payload Value)) {
        if let Some(callback) = &self.callback {
            callback.trace(visit);
        }
    }

    fn iteration_sources<'payload>(&'payload self, visit: &mut dyn FnMut(&'payload Value)) {
        visit(&self.source);
    }

    fn units(&self) -> usize {
        3
    }
}

pub(super) fn storage() -> NativeStorage {
    NativeStorage::payload::<Adapter>()
}

pub(super) fn binding(name: &str) -> Option<NativeBinding> {
    let operation = match name {
        "__default_Iterable_collect" | "__default_Iterable_sum" | "__default_Iterable_product" => {
            return Some(NativeBinding::new(
                vec![Codec::Value],
                Codec::Value,
                destination,
            ));
        }
        "__default_Iterable_fold" => {
            return Some(NativeBinding::new(
                vec![Codec::Value; 3],
                Codec::Value,
                fold,
            ));
        }
        "$foundation_MapIterator_next"
        | "$foundation_FilterIterator_next"
        | "$foundation_TakeIterator_next" => return Some(next.binding()),
        "__default_Iterable_map" => Operation::Map,
        "__default_Iterable_filter" => Operation::Filter,
        "__default_Iterable_take" => Operation::Take,
        _ => return None,
    };
    Some(NativeBinding::new(
        vec![Codec::Value; 2],
        Codec::Value,
        move |cx| construct(cx, operation),
    ))
}

fn destination(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    cx.call_values(cx.selected_at(0)?, &[cx.argument(0)?])
}

fn fold(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    let callback = cx.callable(2)?.store();
    let mut accumulator = cx.argument(1)?;
    let root = cx.heap().root_value(accumulator).ok_or_else(invalid)?;
    let cursor = cx.call_values(cx.selected_at(0)?, &[cx.argument(0)?])?;
    let _cursor = cx.heap().root_value(cursor).ok_or_else(invalid)?;
    let _scope = cx.heap().begin_collection_iteration(&cursor)?;
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

fn invalid() -> RuntimeError {
    RuntimeError::module_validation("Iterable adapter contract")
}

fn construct(cx: &mut CallContext<'_>, operation: Operation) -> NativeResult<Value> {
    let receiver = cx.argument(0)?;
    let callback = if matches!(operation, Operation::Take) {
        None
    } else {
        Some(cx.callable(1)?.store())
    };
    let remaining = if matches!(operation, Operation::Take) {
        usize::decode(cx.argument(1)?)?
    } else {
        0
    };
    let source = cx.call_values(cx.selected_at(0)?, &[receiver])?;
    let _source = cx.heap().root_value(source).ok_or_else(invalid)?;
    cx.allocate_result_payload(
        Adapter {
            source,
            operation,
            callback,
            remaining: Cell::new(remaining),
        },
        &[1],
    )
}

fn next(cx: &mut CallContext<'_>, receiver: ValueHandle<'_>) -> NativeResult<Value> {
    let Some(_active) = receiver.try_enter_operation()? else {
        return Err(RuntimeError::new(
            RuntimeErrorKind::ScriptTrap,
            "recursive next on the same lazy iterator",
        ));
    };
    let (source, operation, callback, remaining) =
        receiver.with_payload::<Adapter, _>(|adapter| {
            Ok((
                adapter.source,
                adapter.operation,
                adapter.callback.clone(),
                adapter.remaining.get(),
            ))
        })?;
    if matches!(operation, Operation::Take) && remaining == 0 {
        return option(cx, None);
    }
    loop {
        cx.poll()?;
        let result = cx.call_stored_values(0, 0, &[source])?;
        let Some(item) = enums::option(cx, &result)? else {
            return option(cx, None);
        };
        let _item = cx.heap().root_value(item).ok_or_else(invalid)?;
        match operation {
            Operation::Map => {
                let result = callback
                    .as_ref()
                    .ok_or_else(invalid)?
                    .call_values(cx, &[item])?;
                let _result = cx.heap().root_value(result).ok_or_else(invalid)?;
                return option(cx, Some(result));
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
        }
    }
}
