//! Checked value publication under the original owner's short storage borrow.
#[cfg(test)]
mod tests;
use crate::{
    error::RuntimeError,
    gc::{GcHeap, HeapObjectId, directory::ObjectStorage, graph::GraphIdentity},
    value::Value,
};
use std::cell::Ref;

/// One exact checked value, not a root or cached mark. The original-owner borrow
/// must end before callbacks or collection. Nominal admission belongs to the caller.
pub(crate) struct CheckedTarget<'owner> {
    heap: &'owner GcHeap,
    target: Option<(HeapObjectId, Ref<'owner, ObjectStorage>)>,
}

impl GcHeap {
    /// Check the exact value once and retain its original owner through a short,
    /// non-reentrant admission. No mark is constructed until publication needs it.
    pub(crate) fn checked_target(&self, value: Value) -> Option<CheckedTarget<'_>> {
        let target = if let Some(id) = value.object_id() {
            let objects = self.objects.try_borrow().ok()?;
            self.validate_heap_value(&objects, &value)?;
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
            if value.object_id().is_none() {
                continue;
            }
            if objects.is_none() {
                objects = Some(self.objects.try_borrow().ok()?);
            }
            self.validate_heap_value(objects.as_ref()?, value)?;
        }
        for value in values {
            if let Some(id) = value.object_id() {
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
        id: HeapObjectId,
    ) -> Result<(), RuntimeError> {
        self.graph.with_publication(|| {
            (
                GraphIdentity::Object(id),
                objects.mark_for_directory(id.index()),
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
