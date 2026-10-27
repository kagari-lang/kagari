//! Read accounting facts under their original heap owners.
use crate::{diagnostics::storage::StorageUsage, error::RuntimeError, gc::GcHeap};

impl GcHeap {
    /// Inspect page backing and side-table capacities outside ordinary timing. This copies
    /// accounting facts only and creates no roots, descriptors or backing buffers.
    ///
    /// Returns an error during a native payload loan or a conflicting storage borrow.
    /// Loaned inline payloads cannot be inspected until their exclusive loan ends.
    pub fn storage_usage(&self) -> Result<StorageUsage, RuntimeError> {
        self.ensure_no_native_borrow()?;
        let objects = self.objects.try_borrow().map_err(|_| {
            RuntimeError::module_validation("heap storage is borrowed during diagnostics")
        })?;

        let interfaces = self.interfaces.try_borrow().map_err(|_| {
            RuntimeError::module_validation("interface storage is borrowed during diagnostics")
        })?;
        let (pending, credits, work_credits, retirement) = self.graph.try_lock()?.storage_usage();
        let mut usage = objects.storage_usage(interfaces.storage_usage(), pending, credits);
        usage.graph_work_credits = work_credits;
        usage.retirement = retirement;
        Ok(usage)
    }
}
