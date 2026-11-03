//! Reviewed value-vector provider using the same scalar/reference storage family.
//! This component is not yet registered as the production Vec implementation.

use kagari_gc::{
    error::GcError,
    object::{Gc, GcObject, ObjectCharge},
    scan::{EdgeVisitor, ScanBudget, ScanKind, ScanStatus},
};
use kagari_runtime::native::{
    binding::NativeResult,
    managed::{
        call::CallRegion, error::failure, payload::NativeObject, slots::Plain, value::StoredValue,
    },
};
use std::marker::PhantomData;

/// Private traced storage; ordinary methods cannot access its owned carriers.
pub struct VectorStorage<T: Plain> {
    values: Vec<StoredValue>,
    revision: u64,
    marker: PhantomData<fn() -> T>,
}

/// Scoped read capability; it is not a long-lived Rust payload loan.
pub struct Vector<'gc, T: Plain> {
    object: Gc<'gc, NativeObject<VectorStorage<T>>>,
}

/// Scoped write capability for the provider proof, constructed only by its API.
/// Declared script write-access admission must supply it at production cutover.
pub struct VectorMut<'gc, T: Plain> {
    vector: Vector<'gc, T>,
}

impl<T: Plain> Copy for Vector<'_, T> {}

impl<T: Plain> Clone for Vector<'_, T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<'gc, T: Plain> VectorMut<'gc, T> {
    pub fn alloc(region: &mut CallRegion<'_, 'gc>) -> NativeResult<Self> {
        // SAFETY: empty storage has no edges. All subsequent edits go through
        // this provider's publication, charge and cursor-repair operations.
        let object = unsafe {
            NativeObject::allocate(region, |_| {
                Ok(VectorStorage {
                    values: Vec::new(),
                    revision: 0,
                    marker: PhantomData,
                })
            })?
        };
        Ok(Self {
            vector: Vector { object },
        })
    }

    pub fn object(self) -> Gc<'gc, NativeObject<VectorStorage<T>>> {
        self.vector.object
    }

    pub fn from_object(
        region: &CallRegion<'_, 'gc>,
        object: Gc<'gc, NativeObject<VectorStorage<T>>>,
    ) -> NativeResult<Self> {
        NativeObject::read(region, object, |_| ())?;
        Ok(Self {
            vector: Vector { object },
        })
    }

    pub fn readable(&self) -> Vector<'gc, T> {
        self.vector
    }

    pub fn push(&self, region: &mut CallRegion<'_, 'gc>, value: T::View<'gc>) -> NativeResult<()> {
        T::validate(region.scope(), value).map_err(failure)?;
        let value = T::lower(value);
        // SAFETY: imported edge is published before commit. Growth is staged
        // while old edges stay traced, checked at its actual allocator capacity,
        // then moved without allocation. Append does not move existing indices.
        unsafe {
            NativeObject::mutate(region, self.vector.object, |storage, mutation| {
                let revision = storage
                    .revision
                    .checked_add(1)
                    .ok_or(GcError::Limit("vector revision"))?;
                let incoming = StoredValue::publish(mutation, value)?;
                if storage.values.len() == storage.values.capacity() {
                    let needed = storage
                        .values
                        .len()
                        .checked_add(1)
                        .ok_or(GcError::Limit("vector length"))?;
                    let capacity = storage
                        .values
                        .capacity()
                        .checked_mul(2)
                        .unwrap_or(needed)
                        .max(needed)
                        .max(4);
                    let mut staged = Vec::new();
                    staged.try_reserve(capacity)?;
                    let bytes = staged
                        .capacity()
                        .checked_mul(size_of::<StoredValue>())
                        .ok_or(GcError::Limit("vector capacity"))?;
                    mutation.prepare_charge(ObjectCharge {
                        bytes,
                        units: storage.values.len() + 1,
                    })?;
                    staged.append(&mut storage.values);
                    storage.values = staged;
                }
                storage.values.push(incoming);
                storage.revision = revision;
                mutation.continue_scan();
                Ok(())
            })
        }
    }

    /// Remove first, then return the protected scoped value without a host Root.
    pub fn pop(&self, region: &mut CallRegion<'_, 'gc>) -> NativeResult<Option<T::View<'gc>>> {
        let value =
            StoredValue::read_from(region, self.vector.object, |storage| storage.values.last())?;
        // SAFETY: the loaded Gc remains valid throughout this scope after source
        // deletion. Tail shrink repairs the captured frontier before continuation.
        unsafe {
            NativeObject::mutate(region, self.vector.object, |storage, mutation| {
                if !storage.values.is_empty() {
                    let revision = storage
                        .revision
                        .checked_add(1)
                        .ok_or(GcError::Limit("vector revision"))?;
                    storage.values.pop();
                    storage.revision = revision;
                }
                mutation.with_scan_state(|state| {
                    if let Some(Some((next, end))) = state {
                        *end = (*end).min(storage.values.len());
                        *next = (*next).min(*end);
                    }
                });
                mutation.continue_scan();
                Ok(())
            })?;
        }
        value
            .map(|value| T::admit(region.scope(), value).map_err(failure))
            .transpose()
    }
}

impl<'gc, T: Plain> Vector<'gc, T> {
    pub fn get(
        &self,
        region: &CallRegion<'_, 'gc>,
        index: usize,
    ) -> NativeResult<Option<T::View<'gc>>> {
        StoredValue::read_from(region, self.object, |storage| storage.values.get(index))?
            .map(|value| T::admit(region.scope(), value).map_err(failure))
            .transpose()
    }

    pub fn is_empty(&self, region: &CallRegion<'_, 'gc>) -> NativeResult<bool> {
        Ok(self.len(region)? == 0)
    }

    pub fn revision(&self, region: &CallRegion<'_, 'gc>) -> NativeResult<u64> {
        NativeObject::read(region, self.object, |storage| storage.revision)
    }

    pub fn len(&self, region: &CallRegion<'_, 'gc>) -> NativeResult<u64> {
        NativeObject::read(region, self.object, |storage| storage.values.len() as u64)
    }
}

// SAFETY: the private vector owns all its edges, inserted only through publication.
// Numeric scan indices survive reallocations and tail removal repairs the frontier.
unsafe impl<T: Plain> GcObject for VectorStorage<T> {
    type ScanState = (usize, usize);

    const SCAN_KIND: ScanKind = ScanKind::Resumable;

    fn begin_scan(&self) -> Self::ScanState {
        (0, self.values.len())
    }

    fn scan(
        &self,
        state: &mut Self::ScanState,
        budget: &mut ScanBudget,
        visitor: &mut EdgeVisitor<'_>,
    ) -> Result<ScanStatus, GcError> {
        while state.0 < state.1 {
            if !budget.take(1) {
                return Ok(ScanStatus::Pending);
            }
            // SAFETY: edge remains in this live source. Retry Pending unchanged.
            if unsafe { self.values[state.0].scan(budget, visitor)? } == ScanStatus::Pending {
                return Ok(ScanStatus::Pending);
            }
            state.0 += 1;
        }
        Ok(ScanStatus::Complete)
    }

    fn charge(&self) -> ObjectCharge {
        ObjectCharge {
            bytes: self.values.capacity() * size_of::<StoredValue>(),
            units: self.values.len(),
        }
    }
}
