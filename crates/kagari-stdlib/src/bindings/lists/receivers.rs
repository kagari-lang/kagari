//! Prepare custom receiver operations once and traverse through its iterator.
use crate::bindings::enums;
use std::slice;
use {
    crate::bindings::lists::invalid,
    kagari_runtime::{
        gc::HeapObjectId,
        native::{
            binding::NativeResult,
            context::{CallContext, LinkedCallable},
        },
        value::Value,
    },
};

pub(super) struct ReceiverCalls<'call> {
    iter: &'call LinkedCallable,
    next: &'call LinkedCallable,
    write: Option<&'call LinkedCallable>,
}

impl<'call> ReceiverCalls<'call> {
    pub(super) fn prepare(
        cx: &CallContext<'call>,
        start: usize,
        mutable: bool,
    ) -> NativeResult<Self> {
        Ok(Self {
            iter: cx.selected_at(start)?,
            next: cx.selected_at(start + 1)?,
            write: mutable.then(|| cx.selected_at(start + 2)).transpose()?,
        })
    }

    pub(super) fn snapshot(&self, cx: &mut CallContext<'_>) -> NativeResult<HeapObjectId> {
        let item = cx.selected_result_parameter(self.next, 0)?;
        let result = cx.allocate_vec(item, vec![])?;
        let _result = cx.heap().root_value(result).ok_or_else(invalid)?;
        let Value::GcHandle(id) = result else {
            return Err(invalid());
        };
        let cursor = cx.call_values(self.iter, &[cx.argument(0)?])?;
        let _cursor = cx.heap().root_value(cursor).ok_or_else(invalid)?;
        loop {
            cx.poll()?;
            let value = cx.call_values(self.next, slice::from_ref(&cursor))?;
            let Some(value) = enums::option(cx, &value)? else {
                return Ok(id);
            };
            let _item = cx.heap().root_value(value).ok_or_else(invalid)?;
            cx.heap().sequence_push(id, value)?;
        }
    }

    pub(super) fn set(
        &self,
        cx: &mut CallContext<'_>,
        index: usize,
        value: Value,
    ) -> NativeResult<()> {
        cx.call_values(
            self.write.ok_or_else(invalid)?,
            &[cx.argument(0)?, Value::U64(index as u64), value],
        )?;
        Ok(())
    }

    pub(super) fn remove(&self, cx: &mut CallContext<'_>, index: usize) -> NativeResult<()> {
        cx.call_values(
            self.write.ok_or_else(invalid)?,
            &[cx.argument(0)?, Value::U64(index as u64)],
        )?;
        Ok(())
    }
}
