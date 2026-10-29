//! Retained Vec handles and owned adapters implemented through public Native APIs.
mod algorithms;
pub(crate) mod borrowed;
mod buffer;
mod conversion;
pub mod cursor;
pub mod scalar;
#[cfg(panic = "unwind")]
pub(crate) mod sort_scalar;
mod storage;
use crate::collections::vector::{
    buffer::SequenceStorage,
    scalar::NativeElement,
    storage::{Work, recover},
};
use kagari_runtime::{
    error::{RuntimeError, RuntimeErrorKind},
    frame::types::arguments::TypeArgument,
    native::{
        binding::NativeResult,
        conversion::{FromKagari, IntoKagari},
        payload::Opaque,
        storage::access::EditPlan,
        storage_type::StorageType,
        typed::NativeContext,
        value_handle::ScriptValue,
        views::{NativeOperationGuard, NativeOperationKind},
    },
};
use std::marker::PhantomData;

/// Library-owned Vec payload. Its representation is not a runtime protocol.
#[derive(Debug)]
pub struct VecStorage {
    contract: StorageType,
    values: SequenceStorage,
    scratch: SequenceStorage,
    work: Option<Work>,
}

/// A retained shared Vec with its exact element type and host access capability.
#[derive(Debug)]
pub struct ScriptVec<T> {
    pub(crate) object: Opaque<VecStorage>,
    element: TypeArgument,
    mutable: bool,
    mapping: PhantomData<fn() -> T>,
}

/// Convert an owned Rust vector through the standard Vec provider.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct VecValues<T>(pub Vec<T>);

impl<T> Clone for ScriptVec<T> {
    fn clone(&self) -> Self {
        Self {
            object: self.object.clone(),
            element: self.element.clone(),
            mutable: self.mutable,
            mapping: PhantomData,
        }
    }
}

impl<T> ScriptVec<T> {
    pub(crate) fn from_object(
        cx: &NativeContext<'_>,
        object: Opaque<VecStorage>,
    ) -> NativeResult<Self> {
        let conversion = cx.conversion_scope(cx.owner())?;
        let element = conversion.parameter(object.type_argument(), 0)?;
        Ok(Self {
            object,
            element,
            mutable: true,
            mapping: PhantomData,
        })
    }

    /// Borrow a compact scalar slice for a non-reentrant read. The runtime rejects
    /// nested execution/storage access until the callback returns; no slice escapes.
    pub fn with_slice<E: NativeElement, R>(
        &self,
        cx: &NativeContext<'_>,
        read: impl FnOnce(&[E]) -> NativeResult<R>,
    ) -> NativeResult<R> {
        // SAFETY: E is sealed to pointer-free scalar layouts. The loan prevents
        // conflicting access and the closure cannot return a borrowed payload slice.
        unsafe {
            self.object.read(cx, None, |values, _| {
                let values = E::slice(&values.values).ok_or_else(|| {
                    RuntimeError::module_validation("Vec scalar view differs from its element type")
                })?;
                read(values)
            })
        }
    }

    /// Edit a pointer-free scalar slice under one checked operation. Completed
    /// writes survive an error or panic, and cursors invalidate when access ends.
    pub fn with_slice_mut<E: NativeElement, R>(
        &self,
        cx: &mut NativeContext<'_>,
        edit: impl FnOnce(&mut [E]) -> NativeResult<R>,
    ) -> NativeResult<R> {
        let count = self.len(cx)?;
        let guard = self.operation(cx)?;
        // SAFETY: E is sealed to pointer-free scalar storage, so writes cannot
        // introduce edges or change length/accounting. The slice cannot escape.
        unsafe {
            self.object.edit(
                cx,
                Some(&guard),
                EditPlan {
                    max_logical_units: count,
                    structural: false,
                },
                |values, _| {
                    let values = E::slice_mut(&mut values.values).ok_or_else(|| {
                        RuntimeError::module_validation(
                            "Vec scalar view differs from its element type",
                        )
                    })?;
                    edit(values)
                },
            )
        }
    }

    pub fn is_mutable(&self) -> bool {
        self.mutable
    }

    pub fn read_only(&self) -> Self {
        let mut view = self.clone();
        view.mutable = false;
        view
    }

    fn writable(&self) -> NativeResult<()> {
        if self.mutable {
            Ok(())
        } else {
            Err(RuntimeError::module_validation(
                "collection view is read-only",
            ))
        }
    }

    pub fn len(&self, cx: &NativeContext<'_>) -> NativeResult<usize> {
        // SAFETY: Only scalar state leaves the loan.
        unsafe { self.object.read(cx, None, |v, _| Ok(v.len())) }
    }

    pub fn is_empty(&self, cx: &NativeContext<'_>) -> NativeResult<bool> {
        Ok(self.len(cx)? == 0)
    }

    pub fn clear(&self, cx: &mut NativeContext<'_>) -> NativeResult<()> {
        self.truncate(cx, 0)
    }

    pub fn truncate(&self, cx: &mut NativeContext<'_>, length: usize) -> NativeResult<()> {
        self.writable()?;
        let count = self.len(cx)?;
        // SAFETY: Only removes edges; commit precedes the first structural change.
        unsafe {
            self.object.edit(
                cx,
                None,
                EditPlan {
                    max_logical_units: count,
                    structural: true,
                },
                |v, access| {
                    v.truncate(access, length);
                    Ok(())
                },
            )
        }
    }

    pub fn swap(&self, cx: &mut NativeContext<'_>, left: usize, right: usize) -> NativeResult<()> {
        self.writable()?;
        let count = self.len(cx)?;
        // SAFETY: Republish both same-payload edges before relocating them.
        unsafe {
            self.object.edit(
                cx,
                None,
                EditPlan {
                    max_logical_units: count,
                    structural: true,
                },
                |v, access| {
                    let a = v.values.get(left).ok_or_else(index_error)?;
                    let b = v.values.get(right).ok_or_else(index_error)?;
                    access.republish_existing(&a);
                    access.republish_existing(&b);
                    if left != right {
                        access.commit_structure();
                    }
                    v.values.swap(left, right)
                },
            )
        }
    }

    pub fn reverse(&self, cx: &mut NativeContext<'_>) -> NativeResult<()> {
        let count = self.len(cx)?;
        let guard = self.operation(cx)?;
        for left in 0..count / 2 {
            let right = count - left - 1;
            // SAFETY: Both edges remain in this guarded payload and are published
            // before relocation; each completed swap is valid on cancellation.
            unsafe {
                self.object.edit(
                    cx,
                    Some(&guard),
                    EditPlan {
                        max_logical_units: count,
                        structural: false,
                    },
                    |values, access| {
                        let a = values.values.get(left).expect("reverse left index");
                        let b = values.values.get(right).expect("reverse right index");
                        access.republish_existing(&a);
                        access.republish_existing(&b);
                        values.values.swap(left, right)
                    },
                )?;
            }
        }
        Ok(())
    }

    pub(crate) fn revision(&self, cx: &NativeContext<'_>) -> NativeResult<u64> {
        // SAFETY: Scalar revision only.
        unsafe {
            self.object
                .read(cx, None, |_, access| Ok(access.revision()))
        }
    }

    pub(crate) fn check_revision(&self, cx: &NativeContext<'_>, revision: u64) -> NativeResult<()> {
        // SAFETY: Only the scalar revision is inspected.
        unsafe {
            self.object.read(cx, None, |_, access| {
                cursor::check_revision(access, revision)
            })
        }
    }

    fn operation<'a>(&self, cx: &NativeContext<'a>) -> NativeResult<NativeOperationGuard<'a>> {
        self.writable()?;
        // SAFETY: recover is allocation-free and all work edges remain in this payload.
        unsafe {
            self.object.try_enter_operation(
                cx,
                NativeOperationKind::ExclusiveSlots,
                Some(recover),
            )?
        }
        .ok_or_else(|| {
            RuntimeError::module_validation("Vec operation conflicts with active access")
        })
    }
}

impl<T: FromKagari> ScriptVec<T> {
    pub fn get(&self, cx: &mut NativeContext<'_>, index: usize) -> NativeResult<Option<T>> {
        // SAFETY: Retention precedes leaving the installed source slot.
        let value = unsafe {
            self.object.read(cx, None, |v, access| {
                v.get(index)
                    .map(|value| access.retain(&v.contract, &value))
                    .transpose()
            })?
        };
        value.map(|value| value.decode(cx)).transpose()
    }

    pub fn pop(&self, cx: &mut NativeContext<'_>) -> NativeResult<Option<T>> {
        let index = self.len(cx)?.saturating_sub(1);
        self.remove(cx, index)
    }

    pub fn remove(&self, cx: &mut NativeContext<'_>, index: usize) -> NativeResult<Option<T>> {
        self.writable()?;
        let count = self.len(cx)?;
        // SAFETY: Retain before removing; relocation publishes every shifted edge.
        let value = unsafe {
            self.object.edit(
                cx,
                None,
                EditPlan {
                    max_logical_units: count,
                    structural: true,
                },
                |v, access| {
                    let Some(value) = v.values.get(index) else {
                        return Ok(None);
                    };
                    let value = access.retain(&v.contract, &value)?;
                    v.remove(access, index);
                    Ok(Some(value))
                },
            )?
        };
        value.map(|value| value.decode(cx)).transpose()
    }

    pub fn for_each(
        &self,
        cx: &mut NativeContext<'_>,
        mut visit: impl FnMut(&mut NativeContext<'_>, T) -> NativeResult<()>,
    ) -> NativeResult<()> {
        let revision = self.revision(cx)?;
        let count = self.len(cx)?;
        for index in 0..count {
            self.check_revision(cx, revision)?;
            let value = self.get(cx, index)?.ok_or_else(index_error)?;
            visit(cx, value)?;
        }
        // Includes the exhaustion read after the final callback.
        self.check_revision(cx, revision)
    }
}

impl<T: IntoKagari> ScriptVec<T> {
    fn retained(&self, cx: &mut NativeContext<'_>, value: T) -> NativeResult<ScriptValue> {
        let mut conversion = cx.conversion_scope(cx.owner())?;
        conversion.encode_value(&self.element, value)
    }

    pub fn set(&self, cx: &mut NativeContext<'_>, index: usize, value: T) -> NativeResult<()> {
        self.writable()?;
        let count = self.len(cx)?;
        if index >= count {
            return Err(index_error());
        }
        let value = self.retained(cx, value)?;
        // Conversion may mutate this Vec through an alias. Charge the current state.
        let count = self.len(cx)?;
        // SAFETY: Checked publication precedes replacing the source edge.
        unsafe {
            self.object.edit(
                cx,
                None,
                EditPlan {
                    max_logical_units: count,
                    structural: false,
                },
                |v, access| {
                    let value = access.publish(&v.contract, &value)?;
                    v.set(index, value)
                },
            )
        }
    }

    pub fn push(&self, cx: &mut NativeContext<'_>, value: T) -> NativeResult<()> {
        self.writable()?;
        self.len(cx)?;
        let value = self.retained(cx, value)?;
        // Append after conversion, which may have changed the length through an alias.
        let index = self.len(cx)?;
        self.insert_retained(cx, index, value)
    }

    pub fn insert(&self, cx: &mut NativeContext<'_>, index: usize, value: T) -> NativeResult<()> {
        self.writable()?;
        let count = self.len(cx)?;
        if index > count {
            return Err(index_error());
        }
        let value = self.retained(cx, value)?;
        self.insert_retained(cx, index, value)
    }

    fn insert_retained(
        &self,
        cx: &mut NativeContext<'_>,
        index: usize,
        value: ScriptValue,
    ) -> NativeResult<()> {
        // No user conversion may run between this charge preflight and the edit.
        let units = self
            .len(cx)?
            .checked_add(1)
            .ok_or_else(|| RuntimeError::resource_limit("Vec length"))?;
        // SAFETY: Reserve before commit; publish imported and relocated edges.
        unsafe {
            self.object.edit(
                cx,
                None,
                EditPlan {
                    max_logical_units: units,
                    structural: true,
                },
                |v, access| {
                    let value = access.publish(&v.contract, &value)?;
                    v.insert(access, index, value)
                },
            )
        }
    }
}

fn index_error() -> RuntimeError {
    RuntimeError::new(
        RuntimeErrorKind::IndexOutOfBounds,
        "Vec index is out of bounds",
    )
}
