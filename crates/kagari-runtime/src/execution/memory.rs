//! Exclusive managed heap and execution source, serviced at driver boundaries.
mod accounting;
use self::accounting::Accounting;
#[cfg(feature = "execution-diagnostics")]
use crate::diagnostics::storage::BufferUsage;
use crate::{
    execution::roots::{
        DependencyId, ExecutionRoots, SessionId, ValueLocation, frames::FrameBanks,
    },
    resource::ResourceState,
    value::view::ValueView,
};
#[cfg(feature = "execution-diagnostics")]
use kagari_gc::heap::metadata::MetadataStats;
use kagari_gc::{
    access::Scope,
    error::GcError,
    heap::{CollectionReport, HeapConfig, HeapStats, Phase},
    root_source::{RootAccess, RootDriver},
    scan::WorkBudget,
};
use std::fmt;

/// Runtime policy for starting collection from committed managed pressure.
#[derive(Debug, Clone, Copy)]
pub struct GcHeapConfig {
    /// Minimum combined heap-unit/executable-metadata threshold at execution safepoints.
    pub collection_threshold: Option<usize>,
    /// Minimum attributed backing-byte pressure for automatic collection.
    /// Independent of logical units; collection_threshold=None disables both.
    pub retained_byte_threshold: usize,
}

impl Default for GcHeapConfig {
    fn default() -> Self {
        Self {
            collection_threshold: Some(1024),
            // A modest 64-KiB batch avoids collecting for each small reserve while
            // admitting large empty buffers. This is a policy floor, not a tuned optimum.
            retained_byte_threshold: 64 * 1024,
        }
    }
}

/// Owns both memory and its complete execution source. Runtime policy observes
/// committed pressure; the collector owns scanning, publication and progress.
pub(crate) struct ManagedMemory {
    driver: RootDriver<ExecutionRoots>,
    config: GcHeapConfig,
    next_units: u128,
    next_bytes: u128,
}

impl fmt::Debug for ManagedMemory {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ManagedMemory")
            .field("stats", &self.driver.stats())
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

impl ManagedMemory {
    pub(crate) fn new(config: GcHeapConfig) -> Self {
        Self {
            // The default core configuration has no admission limits. Exhausting
            // the process-wide heap identity is an unrecoverable construction
            // failure, as it was for Runtime's previous heap owner.
            driver: RootDriver::new(HeapConfig::default()).expect("managed heap identity"),
            config,
            next_units: config.collection_threshold.unwrap_or(usize::MAX) as u128,
            next_bytes: config.retained_byte_threshold.max(1) as u128,
        }
    }

    /// Lend paired, branded memory/source services. No stored carrier or source
    /// loan can escape, and collection/reentry requires ending this operation.
    pub(crate) fn scope<T>(
        &mut self,
        resources: &ResourceState,
        operation: impl for<'gc> FnOnce(
            &mut Scope<'gc>,
            &mut RootAccess<'_, ExecutionRoots>,
        ) -> Result<T, GcError>,
    ) -> Result<T, GcError> {
        Accounting::new(&mut self.driver, resources)
            .driver
            .scope(operation)
    }

    /// Admit one independent execution. Nested synchronous calls reuse this
    /// identity and own only their stack/local suffixes.
    pub(crate) fn enter_execution(
        &mut self,
        resources: &ResourceState,
    ) -> Result<SessionId, GcError> {
        self.scope(resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| slots.enter(0))
        })
    }

    /// Retire the execution after control records and active Native suffixes end.
    /// Faulted memory rejects access; its inert carriers remain until shutdown.
    pub(crate) fn retire_execution(
        &mut self,
        resources: &ResourceState,
        id: SessionId,
    ) -> Result<(), GcError> {
        self.scope(resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| slots.leave(id))
        })
    }

    /// Copy the innermost Native origin coordinate; reading its target still
    /// requires paired source admission, downcast and semantic identity checks.
    pub(crate) fn native_origin<T>(
        &self,
        execution: SessionId,
    ) -> Result<Option<DependencyId<T>>, GcError> {
        self.driver
            .inspect_source(|source| source.inspect_native_origin(execution))
    }

    pub(crate) fn stats(&self) -> HeapStats {
        self.driver.stats()
    }

    pub(crate) fn contains_frame_banks(&self, banks: FrameBanks) -> bool {
        self.driver
            .inspect_source(|source| source.contains_frame_banks(banks))
    }

    /// The caller's frame/source owner must retain any managed location for the
    /// chosen view lifetime. Public cold frame access binds it to a shared loan.
    pub(crate) fn inspect_value<'view>(
        &self,
        location: ValueLocation,
    ) -> Result<ValueView<'view>, GcError> {
        self.driver
            .inspect_source(|source| source.inspect_value(location))
    }

    pub(crate) fn inspect_scalar(
        &self,
        execution: SessionId,
        slot: usize,
    ) -> Result<Option<u64>, GcError> {
        self.driver
            .inspect_source(|source| source.inspect_scalar(execution, slot))
    }

    #[cfg(feature = "execution-diagnostics")]
    pub(crate) fn visit_usage(&self, visit: &mut impl FnMut(BufferUsage)) {
        self.driver
            .inspect_source(|source| source.visit_usage(visit));
    }

    #[cfg(feature = "execution-diagnostics")]
    pub(crate) fn metadata_stats(&self) -> MetadataStats {
        self.driver.metadata_stats()
    }

    pub(crate) fn collect(
        &mut self,
        resources: &ResourceState,
    ) -> Result<CollectionReport, GcError> {
        let report = Accounting::new(&mut self.driver, resources)
            .driver
            .collect()?;
        self.update_thresholds();
        Ok(report)
    }

    /// Observe policy without entering collection or installing a fault guard on
    /// an idle scalar path. Existing work remains due when allocation has stopped.
    pub(crate) fn service_requested(&self, abandoned_publication: bool) -> bool {
        let stats = self.driver.stats();
        match stats.phase {
            Phase::Idle => {
                self.automatic_collection_enabled()
                    && (abandoned_publication
                        || Self::units(stats) >= self.next_units
                        || stats.indirect_bytes as u128 >= self.next_bytes)
            }
            Phase::Roots | Phase::Mark | Phase::Sweep | Phase::Faulted | Phase::Closed => true,
        }
    }

    /// Advance collector work only after an admitted service request. Runtime
    /// owns its collection gate; the paired driver always includes every source.
    pub(crate) fn safepoint(&mut self, resources: &ResourceState) -> Result<(), GcError> {
        Accounting::new(&mut self.driver, resources)
            .driver
            .step(WorkBudget::NORMAL)?;
        if self.driver.stats().phase == Phase::Idle {
            self.update_thresholds();
        }
        Ok(())
    }

    pub(crate) fn automatic_collection_enabled(&self) -> bool {
        self.config.collection_threshold.is_some()
    }

    fn units(stats: HeapStats) -> u128 {
        stats.provider_units.saturating_add(stats.objects as u128)
    }

    fn update_thresholds(&mut self) {
        let stats = self.driver.stats();
        self.next_units = Self::units(stats)
            .saturating_mul(2)
            .max(self.config.collection_threshold.unwrap_or(usize::MAX) as u128)
            .max(1);
        self.next_bytes = (stats.indirect_bytes as u128)
            .saturating_mul(2)
            .max(self.config.retained_byte_threshold.max(1) as u128);
    }
}

#[cfg(test)]
mod tests;
