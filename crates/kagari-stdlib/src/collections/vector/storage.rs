//! Compact collection-owned storage, including all callback-visible work state.
#[cfg(feature = "execution-diagnostics")]
use crate::collections::diagnostics;
use crate::collections::vector::{VecStorage, buffer::SequenceStorage, index_error};
use kagari_runtime::{
    error::RuntimeError,
    frame::types::arguments::TypeArgument,
    gc::HeapObjectId,
    module::ExecutableModule,
    native::{
        binding::NativeResult,
        context::CallContext,
        payload::NativeType,
        storage::{
            NativePayload,
            access::StorageAccess,
            scan::{NativeScanKind, ScanBudget, ScanStatus},
        },
        storage_type::StorageType,
        typed::NativeContext,
    },
    resource::HeapCharge,
    value::Value,
};

#[derive(Debug, Clone, Copy)]
pub(super) enum Work {
    Merge {
        scratch: usize,
        other: usize,
        output: usize,
        end: usize,
    },
    Compact {
        read: usize,
        write: usize,
    },
}

impl VecStorage {
    pub(crate) fn len(&self) -> usize {
        self.values.len()
    }

    pub(crate) fn get(&self, index: usize) -> Option<Value> {
        self.values.get(index)
    }

    pub(super) fn truncate(&mut self, access: &mut StorageAccess<'_>, length: usize) {
        if length < self.len() {
            access.commit_structure();
            self.values.truncate(length);
        }
    }

    /// The caller publishes the imported value before changing a slot.
    pub(super) fn set(&mut self, index: usize, value: Value) -> NativeResult<()> {
        if index >= self.len() {
            return Err(index_error());
        }
        self.values.set(index, value)
    }

    /// The caller publishes the imported value before changing any slots.
    pub(super) fn insert(
        &mut self,
        access: &mut StorageAccess<'_>,
        index: usize,
        value: Value,
    ) -> NativeResult<()> {
        if index > self.len() {
            return Err(index_error());
        }
        self.values
            .try_reserve(1)
            .map_err(|_| RuntimeError::resource_limit("Vec capacity"))?;
        for value in self.values.traced().iter().skip(index) {
            // SAFETY: Every relocated edge remains installed in this payload.
            unsafe { access.republish_existing(value) };
        }
        access.commit_structure();
        self.values.insert(index, value)
    }

    pub(super) fn remove(&mut self, access: &mut StorageAccess<'_>, index: usize) {
        if index >= self.len() {
            return;
        }
        for value in self.values.traced().iter().skip(index + 1) {
            // SAFETY: Every relocated edge remains installed in this payload.
            unsafe { access.republish_existing(value) };
        }
        access.commit_structure();
        self.values.remove(index);
    }

    /// Copy from a call-rooted Array without executing child converters. The
    /// source protects all edges until ordinary result scanner admission finishes.
    pub(crate) fn from_array(
        call: &CallContext<'_>,
        source: HeapObjectId,
        result: &TypeArgument,
    ) -> NativeResult<Self> {
        let element = call.type_parameter(result, 0)?;
        let mut payload = Self::empty_element(element.clone(), call.owner())?;
        let count = call
            .heap()
            .array_len(source)
            .ok_or_else(|| RuntimeError::module_validation("Vec array source"))?;
        payload
            .values
            .try_reserve(count)
            .map_err(|_| RuntimeError::resource_limit("Vec capacity"))?;
        let mut cx = call.native_context()?;
        for index in 0..count {
            call.poll()?;
            let value = call
                .heap()
                .array_get(source, index)
                .ok_or_else(|| RuntimeError::module_validation("Vec array element"))?;
            cx.conversion().check_value(&element, &value)?;
            payload.values.push(value)?;
        }
        Ok(payload)
    }

    pub(crate) fn empty(cx: &NativeContext<'_>, ty: &NativeType<Self>) -> NativeResult<Self> {
        let conversion = cx.conversion_scope(ty.owner())?;
        Self::empty_element(conversion.parameter(ty.type_argument(), 0)?, ty.owner())
    }

    pub(crate) fn empty_element(
        element: TypeArgument,
        owner: &ExecutableModule,
    ) -> NativeResult<Self> {
        Ok(Self {
            values: SequenceStorage::empty(element.ty()),
            scratch: SequenceStorage::empty(element.ty()),
            contract: StorageType::prepare_scoped(element, owner)?,
            work: None,
        })
    }
}

// SAFETY: All script edges reside in initialized main/scratch slots. Scalar buffers
// have no edges. Continuations contain indices and captured finite extents only.
unsafe impl NativePayload for VecStorage {
    type ScanState = (usize, usize, usize, usize);
    const SCAN_KIND: NativeScanKind = NativeScanKind::Resumable;

    fn begin_scan(&self) -> Self::ScanState {
        (
            0,
            self.values.traced().len(),
            0,
            self.scratch.traced().len(),
        )
    }

    fn scan<'a>(
        &'a self,
        state: &mut Self::ScanState,
        budget: &mut ScanBudget,
        visit: &mut dyn FnMut(&'a Value),
    ) -> ScanStatus {
        while state.0 < state.1 {
            if !budget.take(1) {
                return ScanStatus::Pending;
            }
            if let Some(value) = self.values.traced().get(state.0) {
                visit(value);
            }
            state.0 += 1;
        }
        while state.2 < state.3 {
            if !budget.take(1) {
                return ScanStatus::Pending;
            }
            if let Some(value) = self.scratch.traced().get(state.2) {
                visit(value);
            }
            state.2 += 1;
        }
        ScanStatus::Complete
    }

    fn charge(&self) -> HeapCharge {
        HeapCharge::new(
            self.values.len() + self.scratch.len(),
            self.values.retained_bytes() + self.scratch.retained_bytes(),
        )
    }
}

/// Restore the authoritative gap without callbacks, allocation or fallible admission.
pub(super) fn recover(values: &mut VecStorage, access: &mut StorageAccess<'_>) {
    match values.work.take() {
        Some(Work::Merge {
            mut scratch,
            mut output,
            ..
        }) => {
            while scratch < values.scratch.len() {
                let value = values
                    .scratch
                    .get(scratch)
                    .expect("initialized merge scratch");
                // SAFETY: Source remains in this same protected payload.
                unsafe { access.republish_existing(&value) };
                values
                    .values
                    .set(output, value)
                    .expect("same-layout merge gap");
                #[cfg(feature = "execution-diagnostics")]
                diagnostics::update(|counts| counts.recovery_copies += 1);
                scratch += 1;
                output += 1;
            }
        }
        Some(Work::Compact {
            mut read,
            mut write,
        }) => {
            while read < values.values.len() {
                let value = values
                    .values
                    .get(read)
                    .expect("unvisited compaction suffix");
                // SAFETY: Source remains in this same protected payload.
                unsafe { access.republish_existing(&value) };
                values
                    .values
                    .set(write, value)
                    .expect("same-layout compaction gap");
                read += 1;
                write += 1;
            }
            values.values.truncate(write);
        }
        None => {}
    }
    // Scratch is operation-owned; release its backing on success/error/unwind.
    // Constructing an empty buffer and deallocation require no allocation.
    values.scratch = values.scratch.empty_like();
}
