//! One execution's committed storage capacity, shared only by its source/control.
#[cfg(test)]
mod tests;
use kagari_gc::error::GcError;
use std::{
    mem::{self, size_of},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

pub(crate) const DEFAULT_STACK_BYTES: usize = 8 * 1024 * 1024;

/// Private accounting contains no heap, service access or escaping capability.
/// Atomic counters preserve Runtime's Send contract across both Rust owners;
/// execution and source mutation still require exclusive Runtime activation.
#[derive(Debug)]
pub(crate) struct ExecutionSpace {
    limit: usize,
    reserved: AtomicUsize,
}

impl ExecutionSpace {
    pub(crate) fn new(limit: usize) -> Self {
        Self {
            limit,
            reserved: AtomicUsize::new(0),
        }
    }

    /// Only a uniquely owned retired source cache may reset this plain account.
    pub(crate) fn reset(&mut self, limit: usize) {
        self.limit = limit;
        *self.reserved.get_mut() = 0;
    }

    pub(crate) fn claim(&self, bytes: usize) -> Result<(), GcError> {
        self.reserved
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |reserved| {
                reserved
                    .checked_add(bytes)
                    .filter(|next| *next <= self.limit)
            })
            .map(|_| ())
            .map_err(|_| GcError::SourceLimit)
    }

    pub(crate) fn release(&self, bytes: usize) {
        self.reserved
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |reserved| {
                reserved.checked_sub(bytes)
            })
            .expect("execution space ownership underflow");
    }

    pub(crate) fn reserved(&self) -> usize {
        self.reserved.load(Ordering::Relaxed)
    }

    pub(crate) fn lease(self: &Arc<Self>, bytes: usize) -> Result<SpaceLease, GcError> {
        self.claim(bytes)?;
        Ok(SpaceLease {
            space: self.clone(),
            bytes,
        })
    }

    /// An empty reusable control buffer is unassigned between executions. Keep
    /// it only if its actual capacity fits the new owner's limit.
    pub(crate) fn adopt<T>(&self, values: &mut Vec<T>) {
        assert!(
            values.is_empty(),
            "only empty control capacity can be adopted"
        );
        if self.claim(values.capacity() * size_of::<T>()).is_err() {
            *values = Vec::new();
        }
    }
}

/// Uncommon control may outlive session-directory detachment and source retirement.
pub(crate) struct SpaceLease {
    space: Arc<ExecutionSpace>,
    bytes: usize,
}

impl Drop for SpaceLease {
    fn drop(&mut self) {
        self.space.release(self.bytes);
    }
}

/// Bounded geometric growth with exact, fallible allocation. Capacity sufficient
/// for a warmed operation performs no budget update, allocation or Arc clone.
/// The uncommitted replacement is checked before moving original values. No GC,
/// callback or destructor runs while the source's carriers change backing.
pub(crate) fn reserve<T>(
    values: &mut Vec<T>,
    additional: usize,
    space: &ExecutionSpace,
) -> Result<(), GcError> {
    let needed = values
        .len()
        .checked_add(additional)
        .ok_or(GcError::SourceLimit)?;
    if needed <= values.capacity() {
        return Ok(());
    }
    let element = size_of::<T>();
    assert!(element != 0, "execution buffers have physical entries");
    let available = space.limit - space.reserved();
    let maximum = values.capacity().saturating_add(available / element);
    if needed > maximum {
        return Err(GcError::SourceLimit);
    }
    let target = needed
        .max(values.capacity().saturating_mul(2).max(4))
        .min(maximum);
    let mut replacement = Vec::new();
    replacement.try_reserve_exact(target)?;
    let delta = replacement
        .capacity()
        .checked_sub(values.capacity())
        .and_then(|count| count.checked_mul(element))
        .ok_or(GcError::SourceLimit)?;
    space.claim(delta)?;
    replacement.append(values);
    let empty = mem::replace(values, replacement);
    drop(empty);
    Ok(())
}
