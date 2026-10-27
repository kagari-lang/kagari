//! Custom lists retain their declared iteration and per-write failure behavior.
use crate::collections::vector::ScriptVec;
use kagari_runtime::{
    frame::types::arguments::TypeArgument,
    native::{
        binding::NativeResult, context::CallContext, function_handle::PreparedFunction,
        typed::NativeContext, value_handle::ScriptValue,
    },
};
use std::sync::Arc;

pub(super) struct ReceiverCalls {
    iter: Arc<PreparedFunction>,
    next: Arc<PreparedFunction>,
    write: Option<Arc<PreparedFunction>>,
    item: TypeArgument,
    source: ScriptValue,
}

impl ReceiverCalls {
    pub(super) fn prepare(
        call: &CallContext<'_>,
        cx: &mut NativeContext<'_>,
        start: usize,
        mutable: bool,
    ) -> NativeResult<Self> {
        let source = cx
            .conversion()
            .decode_value(&call.argument_type_argument(0)?, &call.argument(0)?)?;
        Ok(Self {
            iter: cx
                .selected_function::<(ScriptValue,), ScriptValue>(start)?
                .prepare_call(cx.runtime())?,
            next: cx
                .selected_function::<(ScriptValue,), Option<ScriptValue>>(start + 1)?
                .prepare_call(cx.runtime())?,
            write: if mutable {
                // Mutating defaults declare either remove(receiver, index) or set(receiver, index, item).
                let selected = call.selected_at(start + 2)?;
                Some(if selected.parameters().len() == 2 {
                    cx.selected_function::<(ScriptValue, u64), ScriptValue>(start + 2)?
                        .prepare_call(cx.runtime())?
                } else {
                    cx.selected_function::<(ScriptValue, u64, ScriptValue), ScriptValue>(start + 2)?
                        .prepare_call(cx.runtime())?
                })
            } else {
                None
            },
            item: call.selected_result_parameter(call.selected_at(start + 1)?, 0)?,
            source,
        })
    }

    pub(super) fn snapshot(
        &self,
        cx: &mut NativeContext<'_>,
    ) -> NativeResult<ScriptVec<ScriptValue>> {
        let result = ScriptVec::create_with_type(cx, self.item.clone(), vec![])?;
        let cursor: ScriptValue = self.iter.call_with_options(
            cx,
            (self.source.clone(),),
            cx.runtime().execution_options(),
        )?;
        loop {
            cx.poll()?;
            let value: Option<ScriptValue> = self.next.call_with_options(
                cx,
                (cursor.clone(),),
                cx.runtime().execution_options(),
            )?;
            let Some(value) = value else {
                return Ok(result);
            };
            result.push(cx, value)?;
        }
    }

    pub(super) fn set(
        &self,
        cx: &mut NativeContext<'_>,
        index: usize,
        value: ScriptValue,
    ) -> NativeResult<()> {
        let _: ScriptValue = self
            .write
            .as_ref()
            .expect("mutable default")
            .call_with_options(
                cx,
                (self.source.clone(), index as u64, value),
                cx.runtime().execution_options(),
            )?;
        Ok(())
    }

    pub(super) fn remove(&self, cx: &mut NativeContext<'_>, index: usize) -> NativeResult<()> {
        let _: ScriptValue = self
            .write
            .as_ref()
            .expect("mutable default")
            .call_with_options(
                cx,
                (self.source.clone(), index as u64),
                cx.runtime().execution_options(),
            )?;
        Ok(())
    }
}
