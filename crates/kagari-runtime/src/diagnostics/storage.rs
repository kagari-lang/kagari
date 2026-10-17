//! Borrowed storage accounting without allocation or persistent counters.
use std::mem::size_of;

/// Occupied and reserved backing bytes for one concrete Vec, excluding children.
#[derive(Debug, Clone, Copy)]
pub struct BufferUsage {
    pub name: &'static str,
    pub entries: usize,
    pub capacity: usize,
    pub entry_bytes: usize,
}

impl BufferUsage {
    pub(crate) fn new<T>(name: &'static str, values: &Vec<T>) -> Self {
        Self {
            name,
            entries: values.len(),
            capacity: values.capacity(),
            entry_bytes: size_of::<T>(),
        }
    }
}

/// Heap backing stores and native payload boxes/sequence buffers. Shared
/// descriptions, roots, metadata, other payload children and allocator overhead
/// are outside these byte counts. Logical GC units are not allocator bytes.
#[derive(Debug, Clone, Copy)]
pub struct StorageUsage {
    pub buffers: [BufferUsage; 14],
    pub interface_snapshots: BufferUsage,
    pub distinct_struct_layouts: usize,
    pub initialization_owners: usize,
    pub initialization_owner_capacity: usize,
    pub native_payload_bytes: usize,
    pub sequence_element_bytes: usize,
    pub sequence_capacity_bytes: usize,
}
