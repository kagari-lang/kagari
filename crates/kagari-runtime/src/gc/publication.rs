//! Checked value publication under the original owner's short storage borrow.
#[cfg(test)]
mod tests;
use crate::{
    error::RuntimeError,
    gc::{GcHeap, directory::ObjectStorage, reference::GcRef},
    value::Value,
};
use std::cell::Ref;

/// One exact checked value, not a root or cached mark. The original-owner borrow
/// must end before callbacks or collection. Nominal admission belongs to the caller.
pub(crate) struct CheckedTarget<'owner> {
    heap: &'owner GcHeap,
    target: Option<(GcRef, Ref<'owner, ObjectStorage>)>,
}

impl GcHeap {
    /// Check the exact value once and retain its original owner through a short,
    /// non-reentrant admission. No mark is constructed until publication needs it.
    pub(crate) fn checked_target(&self, value: Value) -> Option<CheckedTarget<'_>> {
        let target = if value.is_managed() {
            let objects = self.objects.try_borrow().ok()?;
            let id = objects.validate_value(&value)?;
            Some((id, objects))
        } else {
            None
        };
        Some(CheckedTarget { heap: self, target })
    }

    /// Validate the immutable root slice completely before publishing any edge.
    /// The one local owner borrow prevents identity reuse between these two loops.
    pub(super) fn publish_root_values(&self, values: &[Value]) -> Option<()> {
        let mut objects = None;
        for value in values {
            if !value.is_managed() {
                continue;
            }
            if objects.is_none() {
                objects = Some(self.objects.try_borrow().ok()?);
            }
            objects.as_ref()?.validate_value(value)?;
        }
        for value in values {
            if value.is_managed() {
                let id = self.value_identity(objects.as_ref()?, value)?;
                self.publish_resolved_value(objects.as_ref()?, id).ok()?;
            }
        }
        Some(())
    }

    /// Identity and kind were checked under this same original-owner borrow.
    /// Only graph preparation is lazy; Idle never exempts target validation.
    pub(super) fn publish_resolved_value(
        &self,
        objects: &ObjectStorage,
        id: GcRef,
    ) -> Result<(), RuntimeError> {
        self.graph.with_publication(|| {
            (
                id.graph(),
                objects
                    .mark(objects.direct_location(id).expect("checked identity"))
                    .expect("occupied mark"),
            )
        })
    }
}

impl CheckedTarget<'_> {
    pub(crate) fn publish(self) -> Result<(), RuntimeError> {
        if let Some((id, objects)) = self.target {
            self.heap.publish_resolved_value(&objects, id)?;
        }
        Ok(())
    }
}
