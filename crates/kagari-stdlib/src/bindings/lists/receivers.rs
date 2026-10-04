//! Prepare custom receiver operations once and traverse through its iterator.
use std::slice;
use {
    crate::bindings::lists::invalid,
    kagari_runtime::{
        gc::HeapObjectId,
        native::{
            binding::NativeResult,
            context::{CallContext, LinkedCallable},
        },
        value::{EnumTag, Value},
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
        let result = cx.allocate_sequence(item, vec![])?;
        let _result = cx.heap().root_value(result.clone()).ok_or_else(invalid)?;
        let Value::Array(id) = result else {
            return Err(invalid());
        };
        let cursor = cx.call_values(self.iter, &[cx.argument(0)?])?;
        let _cursor = cx.heap().root_value(cursor.clone()).ok_or_else(invalid)?;
        loop {
            cx.poll()?;
            let Value::Enum(option) = cx.call_values(self.next, slice::from_ref(&cursor))? else {
                return Err(invalid());
            };
            let option = cx.heap().enum_snapshot(option).ok_or_else(invalid)?;
            match option.tag {
                EnumTag::OptionNone => return Ok(id),
                EnumTag::OptionSome => {
                    let [value] = option.fields.as_slice() else {
                        return Err(invalid());
                    };
                    let _item = cx.heap().root_value(value.clone()).ok_or_else(invalid)?;
                    cx.heap().array_push(id, value.clone())?;
                }
                _ => return Err(invalid()),
            }
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
