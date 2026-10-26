//! Read accounting facts under their original heap owners.
use crate::{
    diagnostics::storage::{BufferUsage, StorageUsage},
    error::RuntimeError,
    gc::GcHeap,
};

impl GcHeap {
    /// Inspect actual Vec lengths/capacities outside ordinary timing. This copies
    /// accounting facts only and creates no roots, descriptors or backing buffers.
    ///
    /// Returns an error during a native payload loan or a conflicting storage borrow.
    /// Loaned payloads are temporarily detached and cannot be enumerated accurately.
    pub fn storage_usage(&self) -> Result<StorageUsage, RuntimeError> {
        self.ensure_no_native_borrow()?;
        let objects = self.objects.try_borrow().map_err(|_| {
            RuntimeError::module_validation("heap storage is borrowed during diagnostics")
        })?;
        let free = self.free.try_borrow().map_err(|_| {
            RuntimeError::module_validation("heap free slots are borrowed during diagnostics")
        })?;
        let interfaces = self.interfaces.try_borrow().map_err(|_| {
            RuntimeError::module_validation("interface storage is borrowed during diagnostics")
        })?;
        Ok(objects.storage_usage(
            BufferUsage::new("free_identities", &free),
            interfaces.storage_usage(),
        ))
    }
}
