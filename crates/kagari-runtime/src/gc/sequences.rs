//! Family admission for fixed builtin arrays and registered growable sequences.
#[cfg(feature = "execution-diagnostics")]
use crate::diagnostics::{self, Event};

use crate::{
    error::{RuntimeError, RuntimeErrorKind},
    frame::types::compatibility::TypeView,
    gc::{GcHeap, HeapObjectId},
    native::{
        binding::NativeResult,
        sequence::{SequencePayload, SequenceStorage},
        storage_type::StorageType,
        views::SequenceRead,
    },
    value::Value,
};
use kagari_types::{declaration::native::NativeStorageLayout, ty::Ty};
use std::{ops::Bound, sync::Arc};

#[derive(Clone, Copy)]
enum BufferFamily {
    Array,
    Sequence,
}

impl BufferFamily {
    fn accepts(self, ty: &Ty) -> bool {
        matches!(
            (self, ty),
            (Self::Array, Ty::Array(..)) | (Self::Sequence, Ty::NativeObject(_))
        )
    }

    fn invalid(self) -> RuntimeError {
        match self {
            Self::Array => RuntimeError::new(
                RuntimeErrorKind::ScriptTrap,
                "expected a builtin array object",
            ),
            Self::Sequence => RuntimeError::module_validation("expected a nominal sequence object"),
        }
    }
}

impl GcHeap {
    pub(crate) fn read_sequence<R>(
        &self,
        id: HeapObjectId,
        expected: TypeView<'_>,
        read: impl for<'payload> FnOnce(SequenceRead<'payload>) -> NativeResult<R>,
    ) -> NativeResult<R> {
        #[cfg(feature = "execution-diagnostics")]
        {
            diagnostics::record(Event::SequenceAdmission);
            diagnostics::record(Event::SequencePayloadRead);
        }
        self.with_native_object(id, |object| {
            if !matches!(object.ty, Ty::NativeObject(_)) || !object.matches(expected) {
                return Err(RuntimeError::module_validation(
                    "native argument differs from its declared type",
                ));
            }
            if !matches!(
                object.storage.layout(),
                NativeStorageLayout::Sequence { .. }
            ) {
                return Err(BufferFamily::Sequence.invalid());
            }
            let sequence = object.payload::<SequencePayload>()?;
            if sequence.detached {
                return Err(RuntimeError::module_validation("array handle length"));
            }
            read(SequenceRead {
                values: &sequence.values,
            })
        })
    }

    pub(crate) fn ensure_sequence(&self, id: HeapObjectId) -> Result<(), RuntimeError> {
        #[cfg(feature = "execution-diagnostics")]
        diagnostics::record(Event::SequenceAdmission);
        self.ensure_buffer_family(id, BufferFamily::Sequence)
    }

    fn ensure_array(&self, id: HeapObjectId) -> Result<(), RuntimeError> {
        self.ensure_buffer_family(id, BufferFamily::Array)
    }

    fn ensure_buffer_family(
        &self,
        id: HeapObjectId,
        family: BufferFamily,
    ) -> Result<(), RuntimeError> {
        self.with_buffer_payload(id, |ty, _| family.accepts(ty))
            .filter(|valid| *valid)
            .map(|_| ())
            .ok_or_else(|| family.invalid())
    }

    fn read_buffer<R>(
        &self,
        id: HeapObjectId,
        family: BufferFamily,
        read: impl FnOnce(&SequenceStorage) -> R,
    ) -> Result<R, RuntimeError> {
        #[cfg(feature = "execution-diagnostics")]
        if matches!(family, BufferFamily::Sequence) {
            diagnostics::record(Event::SequenceAdmission);
        }
        self.with_buffer_payload(id, |ty, sequence| {
            if !family.accepts(ty) {
                return Err(family.invalid());
            }
            if sequence.detached {
                return Err(RuntimeError::module_validation("array handle length"));
            }
            Ok(read(&sequence.values))
        })
        .ok_or_else(|| family.invalid())?
    }

    pub fn array_len(&self, id: HeapObjectId) -> Option<usize> {
        self.read_buffer(id, BufferFamily::Array, SequenceStorage::len)
            .ok()
    }

    pub fn array_snapshot(&self, id: HeapObjectId) -> Option<Vec<Value>> {
        self.read_buffer(id, BufferFamily::Array, SequenceStorage::snapshot)
            .ok()
    }

    pub fn array_get(&self, id: HeapObjectId, index: impl TryInto<usize>) -> Option<Value> {
        self.ensure_array(id).ok()?;
        self.buffer_get(id, index)
    }

    pub fn array_element(
        &self,
        id: HeapObjectId,
        index: impl TryInto<usize>,
    ) -> Result<Option<Value>, RuntimeError> {
        self.ensure_array(id)?;
        self.read_buffer_element(id, index)
    }

    pub fn array_set(
        &self,
        id: HeapObjectId,
        index: impl TryInto<usize>,
        value: Value,
    ) -> Result<(), RuntimeError> {
        self.ensure_array(id)?;
        self.buffer_set(id, index, value)
    }

    pub(crate) fn array_contract(&self, id: HeapObjectId) -> Option<Arc<StorageType>> {
        self.ensure_array(id).ok()?;
        self.buffer_contract(id)
    }

    pub fn array_fill(&self, id: HeapObjectId, value: Value) -> Result<(), RuntimeError> {
        self.ensure_array(id)?;
        self.buffer_fill(id, value)
    }

    pub fn array_copy_from(
        &self,
        id: HeapObjectId,
        source: HeapObjectId,
    ) -> Result<(), RuntimeError> {
        self.ensure_array(id)?;
        self.ensure_array(source)?;
        self.buffer_copy_from(id, source)
    }

    pub fn array_copy_within(
        &self,
        id: HeapObjectId,
        start: Bound<usize>,
        end: Bound<usize>,
        destination: usize,
    ) -> Result<(), RuntimeError> {
        self.ensure_array(id)?;
        self.buffer_copy_within(id, start, end, destination)
    }

    pub fn sequence_len(&self, id: HeapObjectId) -> Option<usize> {
        self.read_buffer(id, BufferFamily::Sequence, SequenceStorage::len)
            .ok()
    }

    pub fn sequence_snapshot(&self, id: HeapObjectId) -> Option<Vec<Value>> {
        self.read_buffer(id, BufferFamily::Sequence, SequenceStorage::snapshot)
            .ok()
    }

    pub fn sequence_get(&self, id: HeapObjectId, index: impl TryInto<usize>) -> Option<Value> {
        self.ensure_sequence(id).ok()?;
        self.buffer_get(id, index)
    }

    pub fn sequence_element(
        &self,
        id: HeapObjectId,
        index: impl TryInto<usize>,
    ) -> Result<Option<Value>, RuntimeError> {
        self.ensure_sequence(id)?;
        self.read_buffer_element(id, index)
    }

    fn read_buffer_element(
        &self,
        id: HeapObjectId,
        index: impl TryInto<usize>,
    ) -> Result<Option<Value>, RuntimeError> {
        // TryInto is host code. Preserve receiver admission before conversion and
        // reacquire storage afterward; no heap borrow may span that conversion.
        let index = index.try_into().ok();
        self.with_buffer(id, |values| values.get(index?))
            .ok_or_else(|| RuntimeError::module_validation("array handle length"))
    }

    pub fn sequence_set(
        &self,
        id: HeapObjectId,
        index: impl TryInto<usize>,
        value: Value,
    ) -> Result<(), RuntimeError> {
        self.ensure_sequence(id)?;
        self.buffer_set(id, index, value)
    }

    pub(crate) fn sequence_contract(&self, id: HeapObjectId) -> Option<Arc<StorageType>> {
        self.ensure_sequence(id).ok()?;
        self.buffer_contract(id)
    }

    pub fn sequence_fill(&self, id: HeapObjectId, value: Value) -> Result<(), RuntimeError> {
        self.ensure_sequence(id)?;
        self.buffer_fill(id, value)
    }

    pub fn sequence_copy_from(
        &self,
        id: HeapObjectId,
        source: HeapObjectId,
    ) -> Result<(), RuntimeError> {
        self.ensure_sequence(id)?;
        self.ensure_sequence(source)?;
        self.buffer_copy_from(id, source)
    }

    pub fn sequence_copy_within(
        &self,
        id: HeapObjectId,
        start: Bound<usize>,
        end: Bound<usize>,
        destination: usize,
    ) -> Result<(), RuntimeError> {
        self.ensure_sequence(id)?;
        self.buffer_copy_within(id, start, end, destination)
    }

    pub(crate) fn check_sequence_replacement(
        &self,
        id: HeapObjectId,
        index: impl TryInto<usize>,
    ) -> Result<(), RuntimeError> {
        self.ensure_sequence(id)?;
        self.check_buffer_replacement(id, index)
    }
}
