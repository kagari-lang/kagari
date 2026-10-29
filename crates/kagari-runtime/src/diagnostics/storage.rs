//! Borrowed storage accounting without allocation or persistent counters.
use std::mem::size_of;

/// Occupied and reserved backing bytes for one concrete Vec, excluding children.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

/// Last synchronous sweep's allocation snapshot and bounded detached-owner peak.
/// Balances are relative to an enclosing allocation measurement, if active.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct RetirementUsage {
    pub pre_detach_net_bytes: Option<i128>,
    pub pre_detach_interval_peak_bytes: Option<i128>,
    pub max_detached: usize,
    pub module_slots: Option<BufferUsage>,
}

/// Physical heap backing and side tables. Slab and large bytes are disjoint;
/// initialized payload, object headers and Native payload bytes are subsets, not
/// additional allocations. Object headers are within the initialized payload area;
/// page/large-control headers are separate from that area.
/// Unused backing includes vacant slots, class rounding and page tails. Arc control
/// records for allocation backing and representation descriptors are counted explicitly.
/// Coallocated headers, slots, bitmaps and alignment padding are disjoint from the
/// payload areas; their sum is the complete requested raw allocation, not extra backing.
/// HashMap bucket/control arrays, canonical struct layout children, native registration
/// metadata, roots, other payload children and allocator overhead remain outside these
/// byte counts; registry entry counts/capacities are reported, not claimed as exact bytes.
/// Logical GC units are not allocator bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StorageUsage {
    pub buffers: [BufferUsage; 3],
    /// Admitted identities represented by owner-local marks, including active work.
    pub graph_credits: usize,
    /// Registered slot blocks plus singleton owners covered by pending capacity.
    pub graph_work_credits: usize,
    pub retirement: RetirementUsage,
    pub slab_bytes: usize,
    pub large_bytes: usize,
    pub slot_metadata_bytes: usize,
    pub bitmap_bytes: usize,
    pub page_directory_bytes: usize,
    pub page_metadata_bytes: usize,
    pub large_metadata_bytes: usize,
    pub backing_control_bytes: usize,
    /// Interior alignment gaps between payload, header, slots and bitmap regions.
    pub allocation_padding_bytes: usize,
    pub descriptor_bytes: usize,
    pub representation_descriptors: usize,
    pub descriptor_directory_bytes: usize,
    /// Registry occupancy/capacity; HashMap bucket/control overhead is not included.
    pub descriptor_registry_entries: usize,
    pub descriptor_registry_capacity: usize,
    /// Complete occupied representations, including in-object headers.
    pub initialized_payload_bytes: usize,
    /// In-object headers, already included in initialized_payload_bytes.
    pub object_header_bytes: usize,
    pub unused_backing_bytes: usize,
    pub interface_snapshots: BufferUsage,
    pub distinct_struct_layouts: usize,
    pub initialization_owners: usize,
    pub initialization_owner_capacity: usize,
    pub native_payload_bytes: usize,
    pub array_element_bytes: usize,
    pub array_capacity_bytes: usize,
}
