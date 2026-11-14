//! Close physical loans before attributing committed heap work to a session.
#[cfg(feature = "execution-diagnostics")]
use crate::diagnostics::{self, Event};
use crate::{execution::roots::ExecutionRoots, resource::ResourceState};
use kagari_gc::root_source::RootDriver;

/// Does not collect, allocate or invoke user code. The guard also observes
/// commits finalized while the access callback unwinds.
pub(super) struct Accounting<'service> {
    pub(super) driver: &'service mut RootDriver<ExecutionRoots>,
    resources: &'service ResourceState,
    #[cfg(feature = "execution-diagnostics")]
    registrations: usize,
}

impl<'service> Accounting<'service> {
    pub(super) fn new(
        driver: &'service mut RootDriver<ExecutionRoots>,
        resources: &'service ResourceState,
    ) -> Self {
        #[cfg(feature = "execution-diagnostics")]
        let registrations = driver.stats().root_registrations;
        Self {
            driver,
            resources,
            #[cfg(feature = "execution-diagnostics")]
            registrations,
        }
    }
}

impl Drop for Accounting<'_> {
    fn drop(&mut self) {
        self.resources.observe_heap(self.driver.accounting());
        #[cfg(feature = "execution-diagnostics")]
        {
            // Access cannot retire registrations: drops enqueue notices, and
            // collection cannot register roots. Only access's positive delta
            // represents committed new registrations, including on unwind.
            let registered = self
                .driver
                .stats()
                .root_registrations
                .saturating_sub(self.registrations);
            if registered != 0 {
                diagnostics::record_amount(Event::RootRegistration, registered as u64);
            }
        }
    }
}
