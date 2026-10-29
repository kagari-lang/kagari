//! Rooted argument views preserve complete packs, including scoped host capabilities.
use crate::{
    error::RuntimeError,
    frame::{types::arguments::TypeArgument, values::FrameSlots},
    gc::{GcHeap, roots::RootSet},
    module::execution::layout::Location,
    native::{
        binding::NativeResult, conversion::context::ConversionContext, value_handle::ScriptValue,
    },
    value::Value,
};
use std::{ops::Range, ptr};

#[derive(Clone, Copy)]
pub(crate) enum ArgumentSlots<'call> {
    Prepared(&'call [Location]),
    Contiguous {
        start: usize,
        count: usize,
    },
    /// Only sealed scalar argument packs can construct this unrooted view.
    Scalars(&'call [Value]),
    /// A private immutable snapshot; slicing only narrows its visible range.
    Snapshot {
        start: usize,
        count: usize,
    },
}

/// A checked argument pack whose managed entries remain protected by their owner.
/// Snapshots own an immutable root lease; frame views borrow the active call.
/// Retained extraction checks the runtime and declared type. Scoped host loans
/// remain scoped capabilities and cannot be extracted as long-lived ScriptValues.
#[derive(Clone)]
pub struct ArgumentView<'call> {
    pub(super) heap: &'call GcHeap,
    pub(super) frame: Option<FrameSlots>,
    pub(super) slots: ArgumentSlots<'call>,
    pub(super) narrowed: Option<RootSet>,
}

impl<'call> ArgumentView<'call> {
    /// Freeze a root pack before arbitrary argument conversion can run.
    /// The snapshot retains scoped capabilities without extending their host loan;
    /// every eventual access still validates the scope and declared capability.
    /// Foreign/expired roots reject, including invalid empty packs.
    pub fn from_roots(heap: &'call GcHeap, roots: &RootSet) -> NativeResult<Self> {
        let count = roots.slot_count(heap).ok_or_else(invalid)?;
        let mut values = Vec::new();
        values
            .try_reserve_exact(count)
            .map_err(|_| RuntimeError::resource_limit("argument snapshot capacity"))?;
        for index in 0..count {
            values.push(roots.get(heap, index).ok_or_else(invalid)?);
        }
        Self::from_values(heap, values)
    }

    pub(crate) fn from_values(heap: &'call GcHeap, values: Vec<Value>) -> NativeResult<Self> {
        let count = values.len();
        let roots = heap.root_execution_values(values).ok_or_else(invalid)?;
        Ok(Self {
            heap,
            frame: None,
            slots: ArgumentSlots::Snapshot { start: 0, count },
            narrowed: Some(roots),
        })
    }

    /// Freeze frame arguments, or share an already immutable snapshot.
    pub fn snapshot(&self) -> NativeResult<Self> {
        if matches!(self.slots, ArgumentSlots::Snapshot { .. }) {
            return Ok(self.clone());
        }
        let mut values = Vec::new();
        values
            .try_reserve_exact(self.len())
            .map_err(|_| RuntimeError::resource_limit("argument snapshot capacity"))?;
        for index in 0..self.len() {
            values.push(self.get(index).ok_or_else(invalid)?);
        }
        Self::from_values(self.heap, values)
    }

    /// Select a checked range without copying its values or registering more roots.
    pub fn slice(&self, range: Range<usize>) -> Option<Self> {
        if range.start > range.end || range.end > self.len() {
            return None;
        }
        let mut result = self.clone();
        let count = range.end - range.start;
        result.slots = if self.narrowed.is_some() {
            let offset = match self.slots {
                ArgumentSlots::Snapshot { start, .. } => start,
                _ => 0,
            };
            ArgumentSlots::Snapshot {
                start: offset.checked_add(range.start)?,
                count,
            }
        } else {
            match self.slots {
                ArgumentSlots::Prepared(slots) => ArgumentSlots::Prepared(slots.get(range)?),
                ArgumentSlots::Contiguous { start, .. } => ArgumentSlots::Contiguous {
                    start: start.checked_add(range.start)?,
                    count,
                },
                ArgumentSlots::Scalars(values) => ArgumentSlots::Scalars(values.get(range)?),
                ArgumentSlots::Snapshot { .. } => return None,
            }
        };
        Some(result)
    }

    /// Retain one storable argument against its exact declared type.
    /// Scoped host references cannot become long-lived owning ScriptValues.
    pub fn retain(
        &self,
        cx: &mut ConversionContext<'_>,
        expected: &TypeArgument,
        index: usize,
    ) -> NativeResult<ScriptValue> {
        self.check_heap(cx.runtime().gc())?;
        let value = self.get(index).ok_or_else(invalid)?;
        cx.retain_converted(expected, value)
    }

    pub(crate) fn check_heap(&self, heap: &GcHeap) -> NativeResult<()> {
        if !ptr::eq(self.heap, heap) {
            return Err(invalid());
        }
        Ok(())
    }

    /// Read transport for an internal/provider operation with explicit protection.
    ///
    /// # Safety
    /// Keep this view and its execution owner alive, and preserve the source slot
    /// until every copy is published or independently retained. A scoped host
    /// capability also requires its original host scope; retention cannot extend it.
    #[doc(hidden)]
    pub unsafe fn get_transport(&self, index: usize) -> Option<Value> {
        self.get(index)
    }

    pub(crate) fn frame(
        heap: &'call GcHeap,
        frame: FrameSlots,
        slots: ArgumentSlots<'call>,
    ) -> Self {
        Self {
            heap,
            frame: Some(frame),
            slots,
            narrowed: None,
        }
    }

    pub fn len(&self) -> usize {
        match self.slots {
            ArgumentSlots::Prepared(slots) => slots.len(),
            ArgumentSlots::Contiguous { count, .. } | ArgumentSlots::Snapshot { count, .. } => {
                count
            }
            ArgumentSlots::Scalars(values) => values.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub(crate) fn contains(&self, index: usize) -> bool {
        self.with_value(index, |_| ()).is_some()
    }

    /// Read rooted immutable slot data without an owning clone. The closure cannot
    /// retain a slot reference; it must not reenter while this borrow is active.
    pub(crate) fn with_value<R>(&self, index: usize, read: impl FnOnce(&Value) -> R) -> Option<R> {
        if index >= self.len() {
            return None;
        }
        if let Some(roots) = &self.narrowed {
            let offset = match self.slots {
                ArgumentSlots::Snapshot { start, .. } => start,
                _ => 0,
            };
            return roots.with_value(self.heap, offset.checked_add(index)?, read);
        }
        match self.slots {
            ArgumentSlots::Scalars(values) => values.get(index).map(read),
            ArgumentSlots::Prepared(slots) => {
                self.frame?
                    .with_location(self.heap, *slots.get(index)?, read)
            }
            ArgumentSlots::Contiguous { start, count } if index < count => {
                self.frame?
                    .with_value(self.heap, start.checked_add(index)?, read)
            }
            ArgumentSlots::Contiguous { .. } | ArgumentSlots::Snapshot { .. } => None,
        }
    }

    pub(crate) fn get(&self, index: usize) -> Option<Value> {
        self.with_value(index, Value::clone)
    }
}

fn invalid() -> RuntimeError {
    RuntimeError::module_validation("foreign, expired or missing argument snapshot")
}

#[cfg(test)]
mod tests {
    use super::ArgumentView;
    use crate::{
        Runtime, native::conversion::context::ConversionContext,
        test_support::layouts::allocation_owner, value::Value,
    };

    #[test]
    fn snapshots_preserve_ranges_ownership_and_independent_lifetime() {
        let mut runtime = Runtime::default();
        let owner = allocation_owner(&mut runtime);
        let mut cx = ConversionContext::new(&runtime, &owner).unwrap();
        let text_type = cx.type_for::<String>().unwrap();
        let text = runtime.gc().alloc_string("argument".to_owned()).unwrap();
        let roots = runtime
            .gc()
            .root_execution_values(vec![Value::I32(1), text, Value::I32(3)])
            .unwrap();
        let view = ArgumentView::from_roots(runtime.gc(), &roots).unwrap();
        let registered = runtime.gc().active_roots();
        let suffix = view.slice(1..3).unwrap();
        let selected = suffix.slice(0..1).unwrap().snapshot().unwrap();
        assert_eq!(runtime.gc().active_roots(), registered);
        assert_eq!(selected.len(), 1);
        assert!(view.slice(3..3).unwrap().is_empty());
        assert!(view.slice(0..4).is_none());
        let inverted = selected.len() + 1..selected.len();
        assert!(view.slice(inverted).is_none());
        roots.set(runtime.gc(), 1, Value::Unit).unwrap();
        drop(roots);
        runtime.collect_garbage().unwrap();
        assert!(selected.retain(&mut cx, &text_type, 1).is_err());
        let retained = selected.retain(&mut cx, &text_type, 0).unwrap();
        drop((view, suffix, selected));
        runtime.collect_garbage().unwrap();
        assert_eq!(
            retained
                .inspect(&runtime, |view| Ok(view.as_str()?.to_owned()))
                .unwrap(),
            "argument"
        );
        drop(retained);
        assert_eq!(runtime.collect_garbage().unwrap().live_objects, 0);

        let foreign = Runtime::default();
        let foreign_roots = foreign.gc().root_execution_values(vec![]).unwrap();
        assert!(ArgumentView::from_roots(runtime.gc(), &foreign_roots).is_err());
        let foreign_view = ArgumentView::from_roots(foreign.gc(), &foreign_roots).unwrap();
        assert!(foreign_view.retain(&mut cx, &text_type, 0).is_err());
        let empty = runtime.gc().root_execution_values(vec![]).unwrap();
        assert!(
            ArgumentView::from_roots(runtime.gc(), &empty)
                .unwrap()
                .is_empty()
        );
    }
}
