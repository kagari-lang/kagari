//! Call-scoped handles borrow existing frame roots; buffer access stays scoped.
use crate::{
    error::RuntimeError,
    gc::{GcHeap, HeapObjectId, leases::BorrowedLease, roots::RootedValue},
    native::{
        binding::NativeResult,
        context::{ArgumentView, CallContext},
        scalar::NativeScalar,
        sequence::{NativeElement, SequencePayload},
        sequence_edit::SequenceEdit,
        storage::NativePayload,
    },
    value::Value,
};
use kagari_common::identity::table::DefinitionId;
use kagari_types::ty::Ty;

/// A generic value keeps the exact closed Kagari type of its rooted argument.
/// Use a scalar copy or an explicit owning root when retaining it in Rust.
pub struct ValueHandle<'call> {
    heap: &'call GcHeap,
    arguments: ArgumentView<'call>,
    slot: usize,
    ty: &'call Ty<DefinitionId>,
}

/// Prevents recursive entry into an explicitly guarded native payload operation.
/// The receiver remains protected by its call argument; no payload borrow spans reentry.
pub struct NativeOperationGuard<'guard> {
    _receiver: &'guard ValueHandle<'guard>,
    _lease: BorrowedLease<'guard>,
}

impl<'call> ValueHandle<'call> {
    /// Returns None when the same receiver already has a guarded operation active.
    /// The library chooses its recursion error. Drop releases the marker on every exit.
    pub fn try_enter_operation(&self) -> NativeResult<Option<NativeOperationGuard<'_>>> {
        let Value::GcHandle(id) = self.value() else {
            return Err(RuntimeError::module_validation("native operation argument"));
        };
        self.heap.try_enter_native_operation(id).map(|lease| {
            lease.map(|lease| NativeOperationGuard {
                _receiver: self,
                _lease: lease,
            })
        })
    }

    pub(crate) fn from_argument(cx: &CallContext<'call>, slot: usize) -> NativeResult<Self> {
        if !cx.arguments().contains(slot) {
            return Err(RuntimeError::module_validation("native argument slot"));
        }
        Ok(Self {
            heap: cx.heap(),
            arguments: cx.arguments(),
            slot,
            ty: cx.argument_type(slot)?,
        })
    }

    pub fn declared_type(&self) -> &'call Ty<DefinitionId> {
        self.ty
    }

    /// This copy remains protected by the argument only while that argument is
    /// alive. Root it before retaining it beyond the call or mutable reentry.
    pub fn value(&self) -> Value {
        self.arguments
            .get(self.slot)
            .expect("checked native argument slot")
    }

    pub fn scalar<S: NativeScalar>(&self) -> NativeResult<S> {
        if self.ty != &S::abi_type_in() {
            return Err(RuntimeError::module_validation(
                "native scalar differs from its argument type",
            ));
        }
        S::decode(self.value())
    }

    pub fn root(&self) -> NativeResult<RootedValue> {
        self.heap.ensure_no_native_borrow()?;
        self.heap
            .root_value(self.value())
            .ok_or_else(|| RuntimeError::module_validation("native owning value root"))
    }

    pub fn with_payload<S: NativePayload, R>(
        &self,
        access: impl for<'payload> FnOnce(&'payload S) -> NativeResult<R>,
    ) -> NativeResult<R> {
        let Value::GcHandle(id) = self.value() else {
            return Err(RuntimeError::module_validation("native storage argument"));
        };
        self.heap.with_native(id, access)
    }
}

/// A sequence argument remains rooted for the entire native call. This is a
/// handle, not an outstanding payload borrow: collection and callbacks may run
/// between accesses, but cannot run inside `with_slice`.
pub struct SequenceHandle<'call> {
    heap: &'call GcHeap,
    id: HeapObjectId,
    ty: &'call Ty<DefinitionId>,
    _arguments: ArgumentView<'call>,
}

impl<'call> SequenceHandle<'call> {
    pub(crate) fn from_argument(cx: &CallContext<'call>, index: usize) -> NativeResult<Self> {
        let id = match cx.argument(index)? {
            Value::Array(id) | Value::GcHandle(id) => id,
            _ => return Err(RuntimeError::module_validation("native sequence argument")),
        };
        Ok(Self {
            heap: cx.heap(),
            id,
            ty: cx.argument_type(index)?,
            _arguments: cx.arguments(),
        })
    }

    pub fn declared_type(&self) -> &'call Ty<DefinitionId> {
        self.ty
    }

    pub fn len(&self) -> NativeResult<usize> {
        self.heap
            .with_native::<SequencePayload, _>(self.id, |sequence| Ok(sequence.values.len()))
    }

    pub fn is_empty(&self) -> NativeResult<bool> {
        self.len().map(|length| length == 0)
    }

    /// Selects the exact scalar layout once, then exposes contiguous Rust values.
    /// Neither references nor mutable payload borrows can escape the closure.
    pub fn with_slice<E: NativeElement, R>(
        &self,
        access: impl for<'slice> FnOnce(&'slice [E]) -> NativeResult<R>,
    ) -> NativeResult<R> {
        self.heap
            .with_native::<SequencePayload, _>(self.id, |sequence| {
                let values = E::slice(&sequence.values)
                    .ok_or_else(|| RuntimeError::module_validation("sequence scalar layout"))?;
                access(values)
            })
    }
}

/// A binding accepts this view only for a mutable sequence declaration.
/// Fixed-length scalar writes preserve the declared element layout and tracing.
pub struct SequenceMutHandle<'call>(SequenceHandle<'call>);

impl<'call> SequenceMutHandle<'call> {
    pub(crate) fn from_argument(cx: &CallContext<'call>, index: usize) -> NativeResult<Self> {
        SequenceHandle::from_argument(cx, index).map(Self)
    }

    /// Edit the actual storage under an exclusive slot lease. Callbacks may use
    /// unrelated values, but cannot access this receiver's slots until the edit
    /// ends. Completed edits survive errors; traced elements remain rooted.
    pub fn edit<R>(
        &mut self,
        edit: impl for<'buffer> FnOnce(SequenceEdit<'buffer>) -> NativeResult<R>,
    ) -> NativeResult<R> {
        self.0.heap.edit_sequence(self.0.id, edit)
    }

    pub fn read(&self) -> &SequenceHandle<'call> {
        &self.0
    }

    pub fn with_slice_mut<E: NativeElement, R>(
        &mut self,
        access: impl for<'slice> FnOnce(&'slice mut [E]) -> NativeResult<R>,
    ) -> NativeResult<R> {
        self.0.heap.with_sequence_mut(self.0.id, access)
    }
}
