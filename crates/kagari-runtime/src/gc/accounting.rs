//! Read accounting facts under their original heap owners.
use crate::{
    diagnostics::storage::{BufferUsage, StorageUsage},
    gc::GcHeap,
};

impl GcHeap {
    /// Inspect actual Vec lengths/capacities outside ordinary timing. This copies
    /// accounting facts only and creates no roots, descriptors or backing buffers.
    pub fn storage_usage(&self) -> StorageUsage {
        self.objects.borrow().storage_usage(
            BufferUsage::new("free_identities", &self.free.borrow()),
            self.interfaces.borrow().storage_usage(),
        )
    }
}
