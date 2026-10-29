//! Captured descriptor maintenance and explicitly indivisible, one-off compaction.
use super::{
    ObjectStorage,
    descriptors::{Descriptor, DescriptorCursor, structures::StructIdentity},
};
use crate::{
    gc::{
        metrics::{IndivisibleKind, IndivisibleMeasurements, IndivisibleSample, ObservationClock},
        weak_cache::WeakCacheToken,
    },
    module::layout_storage::StructStorageKey,
    native::{
        binding::NativeResult,
        storage::scan::{ScanBudget, ScanStatus},
    },
};
use std::sync::Arc;

pub(in crate::gc) struct Maintenance {
    descriptors: DescriptorCursor,
    descriptors_done: bool,
    compact: usize,
}

pub(in crate::gc) struct MaintenanceStep {
    pub(in crate::gc) status: ScanStatus,
    pub(in crate::gc) removed_structure: Option<WeakCacheToken<StructStorageKey, StructIdentity>>,
}

/// Optional caller-stack state, never part of a maintenance result or continuation.
pub(in crate::gc) struct MaintenanceObservation {
    clock: ObservationClock,
    pub(in crate::gc) destruction: IndivisibleMeasurements,
    pub(in crate::gc) allocation: Option<(IndivisibleKind, IndivisibleSample)>,
}

impl MaintenanceObservation {
    pub(in crate::gc) fn new(clock: ObservationClock) -> Self {
        Self {
            clock,
            destruction: IndivisibleMeasurements::default(),
            allocation: None,
        }
    }
}

impl ObjectStorage {
    /// Rebuild an expired physical description under its still-leased identity.
    pub(in crate::gc) fn struct_description(
        &mut self,
        identity: Arc<StructIdentity>,
    ) -> NativeResult<Arc<Descriptor>> {
        self.pages.descriptors.struct_description(identity)
    }

    /// Register the prepared insertion's token before committing the fields cache.
    /// This also covers a factory abandoned before any physical payload is admitted.
    pub(in crate::gc) fn register_struct_descriptor(
        &mut self,
        descriptor: &Arc<Descriptor>,
        token: WeakCacheToken<StructStorageKey, StructIdentity>,
    ) -> NativeResult<()> {
        self.pages.descriptors.register_structure(descriptor, token)
    }

    pub(in crate::gc) fn begin_maintenance(&self) -> Maintenance {
        Maintenance {
            descriptors: self.pages.descriptors.begin_maintenance(),
            descriptors_done: false,
            compact: 0,
        }
    }

    /// A returned token's weak-cache dispatch is reserved. The caller consumes it
    /// outside this borrow and reports identity-probe work and final destruction;
    /// retirement never hashes or compares the semantic struct key.
    /// Vec relocation is an explicit allocator exception: charge every copied slot
    /// and stop after one operation, reporting its duration separately. Nonempty
    /// maps cannot use that exception: shrinking them traverses and rehashes owners.
    pub(in crate::gc) fn maintenance_slice(
        &mut self,
        cursor: &mut Maintenance,
        budget: &mut ScanBudget,
        mut observation: Option<&mut MaintenanceObservation>,
    ) -> MaintenanceStep {
        if !cursor.descriptors_done {
            let step = self.pages.descriptors.maintenance_slice(
                &mut cursor.descriptors,
                budget,
                observation
                    .as_mut()
                    .map(|observation| (observation.clock, &mut observation.destruction)),
            );
            cursor.descriptors_done = step.status == ScanStatus::Complete;
            if step.removed_structure.is_some() || !cursor.descriptors_done {
                return MaintenanceStep {
                    status: ScanStatus::Pending,
                    removed_structure: step.removed_structure,
                };
            }
        }
        if cursor.compact == 0 && budget.take(1) {
            cursor.compact = 1;
            // Empty release has no survivor inspection or rehashing. Retain
            // nonempty capacity rather than migrate owners beyond this budget.
            if self.initialization_owners.is_empty() && self.initialization_owners.capacity() != 0 {
                let started = observation.as_ref().and_then(|state| state.clock.start());
                self.initialization_owners.shrink_to_fit();
                if let (Some(started), Some(state)) = (started, observation.as_mut()) {
                    state.allocation = Some((
                        IndivisibleKind::EmptyMapRelease,
                        IndivisibleSample::new(
                            "initialization_owners",
                            0,
                            1,
                            None,
                            started.elapsed(),
                        ),
                    ));
                }
            }
        }
        MaintenanceStep {
            status: if cursor.compact == 1 {
                ScanStatus::Complete
            } else {
                ScanStatus::Pending
            },
            removed_structure: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(feature = "execution-diagnostics")]
    use crate::diagnostics::allocations;
    use crate::{
        Runtime, gc::reference::GcRef, module::ModuleKey, test_support::layouts::allocation_owner,
    };

    fn exercise_initialization_owner_maintenance(
        objects: &mut ObjectStorage,
        owner: ModuleKey,
        references: &[GcRef],
    ) {
        const PEAK: usize = 4096;
        const SURVIVORS: usize = 128;
        objects.initialization_owners.reserve(PEAK);
        for &reference in &references[..PEAK] {
            objects.initialization_owners.insert(reference, owner);
        }
        // Model per-identity retirement before maintenance without introducing
        // unrelated payload/descriptor allocations into this map's accounting.
        for &reference in &references[SURVIVORS..PEAK] {
            assert_eq!(
                objects.initialization_owners.remove(&reference),
                Some(owner)
            );
        }
        let capacity = objects.initialization_owners.capacity();
        assert!(capacity > SURVIVORS * 4);
        let mut observation = MaintenanceObservation::new(ObservationClock::default());
        let mut cursor = objects.begin_maintenance();
        let mut zero = ScanBudget::new(0);
        assert_eq!(
            objects
                .maintenance_slice(&mut cursor, &mut zero, None)
                .status,
            ScanStatus::Pending
        );
        assert_eq!(zero.spent(), 0);
        {
            let mut budget = ScanBudget::new(1);
            let step = objects.maintenance_slice(&mut cursor, &mut budget, Some(&mut observation));
            assert_eq!(
                budget.spent(),
                1,
                "nonempty map maintenance cannot hide a survivor migration"
            );
            assert_eq!(step.status, ScanStatus::Complete);
            assert!(step.removed_structure.is_none());
            assert_eq!(observation.destruction.count, 0);
            assert!(observation.allocation.is_none());
            assert_eq!(objects.initialization_owners.capacity(), capacity);
            assert_eq!(objects.initialization_owners.len(), SURVIVORS);
        }
        for &reference in &references[..SURVIVORS] {
            assert_eq!(
                objects.initialization_owners.remove(&reference),
                Some(owner)
            );
        }
        assert!(objects.initialization_owners.is_empty());
        let empty_capacity = objects.initialization_owners.capacity();
        assert!(empty_capacity > 0);
        let mut cursor = objects.begin_maintenance();
        {
            let mut budget = ScanBudget::new(1);
            let step = objects.maintenance_slice(&mut cursor, &mut budget, None);
            assert_eq!(
                budget.spent(),
                1,
                "empty release needs no bucket inspection or rehash charge"
            );
            assert_eq!(step.status, ScanStatus::Complete);
            assert_eq!(objects.initialization_owners.capacity(), 0);
        }
    }

    #[test]
    fn tiny_budget_preserves_initialization_survivors_and_releases_only_empty_capacity() {
        let mut runtime = Runtime::default();
        let owner = allocation_owner(&mut runtime);
        let references: Vec<_> = (0..4096)
            .map(|_| {
                runtime
                    .gc()
                    .alloc_string(String::new())
                    .unwrap()
                    .direct_reference()
                    .unwrap()
            })
            .collect();
        let mut objects = ObjectStorage::default();
        #[cfg(feature = "execution-diagnostics")]
        {
            let (_, counts) = allocations::measure(|| {
                exercise_initialization_owner_maintenance(&mut objects, owner.key(), &references);
            });
            assert_eq!(
                counts.requests, 1,
                "only the original map reservation may allocate"
            );
            assert_eq!(
                counts.net_bytes, 0,
                "empty maintenance releases the original table allocation"
            );
        }
        #[cfg(not(feature = "execution-diagnostics"))]
        exercise_initialization_owner_maintenance(&mut objects, owner.key(), &references);
        assert_eq!(objects.initialization_owners.capacity(), 0);
    }
}
