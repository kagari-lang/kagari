//! Typed callbacks share the checked exclusive buffer and its unwind restoration.
use crate::native::{
    binding::NativeResult, collections::vector::ScriptVec, conversion::FromKagari,
    typed::NativeContext,
};

impl<T: FromKagari> ScriptVec<T> {
    /// Keep matching elements in order. Completed removals survive a later
    /// callback error; the exclusive storage lease rejects alias mutations.
    pub fn retain(
        &self,
        cx: &mut NativeContext<'_>,
        mut keep: impl FnMut(&mut NativeContext<'_>, T) -> NativeResult<bool>,
    ) -> NativeResult<()> {
        let id = self.id(cx, true)?;
        let heap = cx.runtime().gc();
        heap.edit_sequence(id, |mut buffer| {
            buffer.retain(|value| {
                cx.poll()?;
                let value = cx.conversion.decode_prepared(&self.element, &value)?;
                let keep = keep(cx, value)?;
                cx.poll()?;
                Ok(keep)
            })
        })
    }

    /// Remove consecutive equal elements, preserving the first in each run.
    /// Both compared elements stay retained across conversion and callback GC.
    pub fn dedup_by(
        &self,
        cx: &mut NativeContext<'_>,
        mut equal: impl FnMut(&mut NativeContext<'_>, T, T) -> NativeResult<bool>,
    ) -> NativeResult<()> {
        let id = self.id(cx, true)?;
        let heap = cx.runtime().gc();
        heap.edit_sequence(id, |mut buffer| {
            buffer.dedup_by(|left, right| {
                cx.poll()?;
                let left = cx.conversion.decode_prepared(&self.element, &left)?;
                let right = cx.conversion.decode_prepared(&self.element, &right)?;
                let equal = equal(cx, left, right)?;
                cx.poll()?;
                Ok(equal)
            })
        })
    }
}
