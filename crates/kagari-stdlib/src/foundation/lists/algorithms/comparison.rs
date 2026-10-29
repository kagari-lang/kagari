//! Comparator selections remain pinned throughout one collection algorithm.
use crate::foundation::{
    enumerations::value::{self as enums, EnumSymbols},
    lists::algorithms::{Algorithm, invalid},
};
use kagari_runtime::native::{
    binding::NativeResult,
    context::CallContext,
    function_handle::{Function, PreparedFunction},
    typed::NativeContext,
    value_handle::ScriptValue,
};
use std::{cmp::Ordering, sync::Arc};

pub(super) struct Comparison<'call, 'symbols> {
    symbols: &'symbols EnumSymbols,
    call: &'call CallContext<'call>,
    operation: Option<Arc<PreparedFunction>>,
    callback: Option<Arc<PreparedFunction>>,
    algorithm: Algorithm,
}

impl<'call, 'symbols> Comparison<'call, 'symbols> {
    pub(super) fn prepare(
        call: &'call CallContext<'call>,
        cx: &mut NativeContext<'_>,
        algorithm: Algorithm,
        symbols: &'symbols EnumSymbols,
    ) -> NativeResult<Self> {
        let operation = if algorithm.comparison() {
            Some(
                cx.selected_function::<(ScriptValue, ScriptValue), ScriptValue>(0)?
                    .prepare_call(cx.runtime())?,
            )
        } else {
            None
        };
        let callback = if matches!(
            algorithm,
            Algorithm::SortBy | Algorithm::SortByKey | Algorithm::Retain
        ) {
            let ty = call.argument_type_argument(1)?;
            // SAFETY: The call frame retains the callback until its owning Function is decoded.
            let value = unsafe { call.argument_transport(1) }?;
            if matches!(algorithm, Algorithm::SortBy) {
                Some(
                    // SAFETY: CallContext retains the callback in its argument snapshot.
                    unsafe {
                        cx.conversion()
                            .decode_transport::<Function<(ScriptValue, ScriptValue), ScriptValue>>(
                                &ty, &value,
                            )
                    }?
                    .prepare_call(cx.runtime())?,
                )
            } else {
                Some(
                    // SAFETY: CallContext retains the callback in its argument snapshot.
                    unsafe {
                        cx.conversion()
                            .decode_transport::<Function<(ScriptValue,), ScriptValue>>(&ty, &value)
                    }?
                    .prepare_call(cx.runtime())?,
                )
            }
        } else {
            None
        };
        Ok(Self {
            symbols,
            call,
            operation,
            callback,
            algorithm,
        })
    }

    pub(super) fn keep(
        &self,
        cx: &mut NativeContext<'_>,
        value: ScriptValue,
    ) -> NativeResult<bool> {
        self.callback
            .as_ref()
            .ok_or_else(invalid)?
            .call_with_options(cx, (value,), cx.runtime().execution_options())
    }

    pub(super) fn equal(
        &self,
        cx: &mut NativeContext<'_>,
        a: ScriptValue,
        b: ScriptValue,
    ) -> NativeResult<bool> {
        cx.poll()?;
        self.operation
            .as_ref()
            .ok_or_else(invalid)?
            .call_with_options(cx, (a, b), cx.runtime().execution_options())
    }

    pub(super) fn order(
        &self,
        cx: &mut NativeContext<'_>,
        a: ScriptValue,
        b: ScriptValue,
    ) -> NativeResult<Ordering> {
        cx.poll()?;
        let result: ScriptValue = if matches!(self.algorithm, Algorithm::SortBy) {
            self.callback
                .as_ref()
                .ok_or_else(invalid)?
                .call_retained_with_options(cx, &[&a, &b], cx.runtime().execution_options())?
        } else {
            let (a, b) = if matches!(self.algorithm, Algorithm::SortByKey) {
                let selector = self.callback.as_ref().ok_or_else(invalid)?;
                let a: ScriptValue = selector.call_retained_with_options(
                    cx,
                    &[&a],
                    cx.runtime().execution_options(),
                )?;
                let b: ScriptValue = selector.call_retained_with_options(
                    cx,
                    &[&b],
                    cx.runtime().execution_options(),
                )?;
                (a, b)
            } else {
                (a, b)
            };
            self.operation
                .as_ref()
                .ok_or_else(invalid)?
                .call_retained_with_options(cx, &[&a, &b], cx.runtime().execution_options())?
        };
        // The checked result already owns its root. Re-encoding into the outer
        // conversion context would retain every comparator result until sort exit.
        // SAFETY: result stays retained through the callback-free ordering read.
        let value = unsafe { result.value_transport(cx.runtime()) }?;
        enums::ordering(self.symbols, self.call, &value)
    }
}
