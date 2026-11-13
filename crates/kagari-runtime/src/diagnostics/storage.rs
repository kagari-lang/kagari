//! Opt-in backing observations and concrete buffer accounting.
use crate::{
    Runtime,
    frame::ExecutionFrame,
    native::context::NativeCall,
    resource::ResourceState,
    session::SessionState,
    value::{carriers::StoredValue, view::ValueView},
};
use kagari_gc::heap::metadata::MetadataStats;
use std::mem::size_of;

/// Execution/root/control backing at a closed diagnostic boundary. Excludes
/// immutable code, object payloads and allocator overhead. Collector metadata and
/// root-release channels are reported separately from execution backing.
/// Repeated names describe distinct buffers, not duplicated accounting.
#[derive(Debug)]
pub struct ExecutionUsage {
    pub runtime_bytes: usize,
    pub resource_bytes: usize,
    pub session_bytes: usize,
    pub frame_bytes: usize,
    pub value_bytes: usize,
    pub view_bytes: usize,
    pub native_call_bytes: usize,
    pub collector: MetadataStats,
    pub buffers: Vec<BufferUsage>,
}

/// Snapshot execution backing after releasing all frame, bank and root-table loans.
/// Allocates diagnostic output storage; do not include this in throughput timing.
pub fn execution_usage(runtime: &Runtime) -> ExecutionUsage {
    let mut buffers = Vec::new();
    visit_usage(runtime, &mut |buffer| buffers.push(buffer));
    ExecutionUsage {
        runtime_bytes: size_of::<Runtime>(),
        resource_bytes: size_of::<ResourceState>(),
        session_bytes: size_of::<SessionState>(),
        frame_bytes: size_of::<ExecutionFrame>(),
        value_bytes: size_of::<StoredValue>(),
        view_bytes: size_of::<ValueView<'static>>(),
        native_call_bytes: size_of::<NativeCall<'static>>(),
        collector: runtime.memory.metadata_stats(),
        buffers,
    }
}

/// Nonallocating retained execution/control backing at a closed boundary.
///
/// Runtime inline bytes already include ResourceState and the heap/source control
/// headers; do not add their individual size descriptions again. Buffer bytes
/// include active and parked storage, initialized entries versus reserved capacity.
/// Managed payload/backing, immutable code, individual Arc allocations, external
/// Rust state and allocator overhead are separate. This is a snapshot, not a peak.
#[derive(Debug, Clone, Copy)]
pub struct ExecutionBacking {
    pub inline_runtime_bytes: usize,
    pub initialized_buffer_bytes: usize,
    pub reserved_buffer_bytes: usize,
    pub collector: MetadataStats,
}

/// Summarize the same concrete buffers as execution_usage without allocating
/// diagnostic output. Collector metadata keeps its separate documented limits.
/// End frame/source/control loans before calling, as for execution_usage.
pub fn execution_backing(runtime: &Runtime) -> ExecutionBacking {
    let mut result = ExecutionBacking {
        inline_runtime_bytes: size_of::<Runtime>(),
        initialized_buffer_bytes: 0,
        reserved_buffer_bytes: 0,
        collector: runtime.memory.metadata_stats(),
    };
    visit_usage(runtime, &mut |buffer| {
        result.initialized_buffer_bytes += buffer.entries * buffer.entry_bytes;
        result.reserved_buffer_bytes += buffer.capacity * buffer.entry_bytes;
    });
    result
}

fn visit_usage(runtime: &Runtime, visit: &mut impl FnMut(BufferUsage)) {
    runtime.memory.visit_usage(visit);
    runtime.resources().sessions.visit_usage(visit);
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

#[cfg(test)]
mod tests {
    use super::{execution_backing, execution_usage};
    use crate::{
        Runtime,
        diagnostics::{self, allocations},
        execution_metadata::managed::sources::MetadataSource,
        module::managed::source::RevisionSource,
        native::conversion::context::ConversionContext,
        test_support::update_programs::load_empty,
        value::carriers::StoredValue,
    };
    use std::mem::size_of;

    #[test]
    fn closed_observations_count_real_sources_and_do_not_allocate_backing_snapshots() {
        let mut runtime = Runtime::default();
        let program = load_empty(&mut runtime, "diagnostic-observations");
        let owner = runtime.select_program(&program).unwrap();
        let mut cx = ConversionContext::new(&mut runtime, &owner).unwrap();
        let (value, created) = diagnostics::measure(|| {
            let value = cx.encode("rooted diagnostic data".to_owned()).unwrap();
            cx.runtime_mut()
                .with_metadata_graph(
                    &[MetadataSource::Revision {
                        owner: &owner,
                        source: RevisionSource::Current,
                    }],
                    |_, _, _| Ok(()),
                )
                .unwrap();
            cx.runtime_mut().collect_garbage().unwrap();
            value
        });
        assert_eq!(created.root_registrations, 1);
        assert!(created.metadata_graph_admissions > 0);
        assert!(created.execution_root_slots_examined > 0);
        assert!(created.bank_reservations > 0);
        assert!(created.bank_bytes_initialized >= size_of::<StoredValue>() as u64);
        let ((), observed) = diagnostics::measure(|| {
            let alias = value.clone();
            let view = cx.import(&alias).unwrap();
            assert_eq!(
                cx.read_string(&view, str::to_owned).unwrap(),
                "rooted diagnostic data"
            );
        });
        assert_eq!(
            observed.root_registrations, 0,
            "clone/import do not create Host registrations"
        );
        assert_eq!(
            observed.argument_transfers, 0,
            "readonly Host access does not admit frame parameters"
        );
        let (backing, costs) = allocations::measure(|| execution_backing(cx.runtime()));
        assert_eq!(costs.requests, 0);
        assert_eq!(costs.net_bytes, 0);
        let detailed = execution_usage(cx.runtime());
        assert!(
            detailed
                .buffers
                .iter()
                .any(|buffer| buffer.name == "local temporaries" && buffer.entries != 0)
        );
        assert_eq!(
            backing.initialized_buffer_bytes,
            detailed
                .buffers
                .iter()
                .map(|buffer| buffer.entries * buffer.entry_bytes)
                .sum::<usize>()
        );
        assert_eq!(
            backing.reserved_buffer_bytes,
            detailed
                .buffers
                .iter()
                .map(|buffer| buffer.capacity * buffer.entry_bytes)
                .sum::<usize>()
        );
        assert!(backing.reserved_buffer_bytes >= backing.initialized_buffer_bytes);
        assert_eq!(backing.inline_runtime_bytes, size_of::<Runtime>());
    }
}
