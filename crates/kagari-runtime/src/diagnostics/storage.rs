//! Opt-in backing observations and concrete buffer accounting.
use crate::{
    Runtime, frame::ExecutionFrame, resource::ResourceState, session::SessionState, value::Value,
};
use std::mem::size_of;

/// Execution/root/control backing at a closed diagnostic boundary. Excludes
/// immutable code, object payloads, allocator overhead and root-release channels.
/// Repeated names describe distinct buffers, not duplicated accounting.
#[derive(Debug)]
pub struct ExecutionUsage {
    pub runtime_bytes: usize,
    pub resource_bytes: usize,
    pub session_bytes: usize,
    pub frame_bytes: usize,
    pub value_bytes: usize,
    pub buffers: Vec<BufferUsage>,
}

/// Snapshot execution backing after releasing all frame, bank and root-table loans.
/// Allocates diagnostic output storage; do not include this in throughput timing.
pub fn execution_usage(runtime: &Runtime) -> ExecutionUsage {
    let mut buffers = Vec::new();
    runtime
        .resources()
        .frame_values
        .borrow()
        .append_usage(&mut buffers);
    runtime.resources().sessions.append_usage(&mut buffers);
    runtime.gc().append_root_usage(&mut buffers);
    ExecutionUsage {
        runtime_bytes: size_of::<Runtime>(),
        resource_bytes: size_of::<ResourceState>(),
        session_bytes: size_of::<SessionState>(),
        frame_bytes: size_of::<ExecutionFrame>(),
        value_bytes: size_of::<Value>(),
        buffers,
    }
}

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
    pub buffers: [BufferUsage; 15],
    pub interface_snapshots: BufferUsage,
    pub distinct_struct_layouts: usize,
    pub initialization_owners: usize,
    pub initialization_owner_capacity: usize,
    pub native_payload_bytes: usize,
    pub sequence_element_bytes: usize,
    pub sequence_capacity_bytes: usize,
}
