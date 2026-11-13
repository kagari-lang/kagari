//! Independent failure/capacity oracles for source and detached control ownership.
use super::{ExecutionSpace, reserve};
#[cfg(feature = "execution-diagnostics")]
use crate::diagnostics::allocations;
use crate::{
    Runtime,
    error::RuntimeErrorKind,
    execution::roots::{ExecutionRoots, ValueBank},
    test_support::update_programs,
    value::{carriers::ScopedValue, strings::StringObject},
};
use kagari_gc::error::GcError;
use std::sync::Arc;

#[test]
fn exhausted_capacity_preserves_values_and_detached_leases() {
    #[cfg(feature = "execution-diagnostics")]
    {
        let (space, allocation) = allocations::measure(|| Arc::new(ExecutionSpace::new(24)));
        assert_eq!(allocation.requests, 1);
        assert_eq!(
            allocation.requested_bytes,
            size_of::<ExecutionSpace>() + 2 * size_of::<usize>()
        );
        drop(space);
    }
    let space = Arc::new(ExecutionSpace::new(24));
    let mut values = Vec::new();
    reserve(&mut values, 2, &space).unwrap();
    values.extend([7_u64, 11]);
    assert_eq!(space.reserved(), 24);
    assert_eq!(reserve(&mut values, 2, &space), Err(GcError::SourceLimit));
    assert_eq!(values, [7, 11]);
    let held = space.lease(0).unwrap();
    // Source retirement releases its capacity while detached Rust control still
    // owns the same ledger. An unassigned cache may then be adopted by a new one.
    values.clear();
    space.release(values.capacity() * size_of::<u64>());
    let lease = space.lease(16).unwrap();
    assert_eq!(space.reserved(), 16);
    drop(held);
    assert_eq!(space.reserved(), 16);
    drop(lease);
    assert_eq!(space.reserved(), 0);
    let smaller = ExecutionSpace::new(8);
    smaller.adopt(&mut values);
    assert_eq!(values.capacity(), 0);
    reserve(&mut values, 1, &smaller).unwrap();
    values.push(19);
    assert_eq!(smaller.reserved(), 8);
    assert_eq!(values, [19]);
}

#[test]
fn source_capacity_failure_keeps_its_graph_and_retires_cached_capacity() {
    let mut runtime = Runtime::default();
    let (execution, value) = runtime
        .memory
        .scope(&runtime.resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                let execution = slots.enter(0, 96)?;
                let object = StringObject::alloc(slots.scope(), "survives rejected growth".into())?;
                let value = slots.push_local(execution, ScopedValue::managed(object))?;
                assert!(matches!(
                    slots.push_dependency(execution, object),
                    Err(GcError::SourceLimit)
                ));
                assert_eq!(
                    slots.grow(execution, ValueBank::Managed, 1),
                    Err(GcError::SourceLimit)
                );
                Ok((execution, value))
            })
        })
        .unwrap();
    runtime.memory.collect(&runtime.resources).unwrap();
    assert_eq!(runtime.gc_stats().objects, 1);
    runtime
        .memory
        .scope(&runtime.resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                let value = slots.get(value)?;
                let object = slots
                    .scope()
                    .downcast::<StringObject>(value.object().unwrap())?;
                assert!(slots.scope().read(object, |string| string.as_str()
                    == "survives rejected growth"));
                Ok(())
            })
        })
        .unwrap();
    let detached_account = runtime.memory.execution_space(execution).unwrap();
    runtime
        .memory
        .retire_execution(&runtime.resources, execution)
        .unwrap();
    assert_eq!(detached_account.reserved(), 0);
    let next = runtime
        .memory
        .scope(&runtime.resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                let next = slots.enter(0, 64)?;
                assert_ne!(next, execution);
                assert!(matches!(slots.get(value), Err(GcError::Index)));
                assert_eq!(slots.storage(next)?.backing_bytes(), 64);
                Ok(next)
            })
        })
        .unwrap();
    assert_eq!(
        detached_account.reserved(),
        0,
        "a cache cannot reset an account still held by detached control"
    );
    let rejected = runtime
        .memory
        .scope(&runtime.resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| slots.grow(next, ValueBank::Managed, 1))
        })
        .unwrap_err();
    assert_eq!(rejected, GcError::SourceLimit);
    runtime
        .memory
        .retire_execution(&runtime.resources, next)
        .unwrap();
    runtime.memory.collect(&runtime.resources).unwrap();
    assert_eq!(runtime.gc_stats().objects, 0);
    assert_eq!(runtime.gc_stats().root_registrations, 0);
    assert!(!runtime.is_quarantined());
}

#[test]
fn allocator_capacity_failure_has_a_distinct_runtime_kind() {
    let space = ExecutionSpace::new(usize::MAX);
    let mut bytes = Vec::<u8>::new();
    // Vec rejects an allocation larger than isize::MAX before invoking the
    // allocator. This deterministically exercises allocation failure without OOM.
    let error = reserve(&mut bytes, isize::MAX as usize + 1, &space).unwrap_err();
    assert_eq!(error, GcError::Allocation);
    assert_eq!(space.reserved(), 0);
    assert!(bytes.is_empty());
    let runtime = Runtime::default();
    assert_eq!(
        runtime.managed_memory_error(error).kind(),
        RuntimeErrorKind::AllocationFailure
    );
    assert_eq!(
        runtime.managed_memory_error(GcError::SourceLimit).kind(),
        RuntimeErrorKind::StackOverflow
    );
    assert!(!runtime.is_quarantined());
}

#[test]
fn source_exhaustion_is_sticky_only_in_the_current_session() {
    let mut runtime = Runtime::default();
    let program = update_programs::load_empty(&mut runtime, "space-termination");
    let owner = runtime.select_program(&program).unwrap();
    let mut session = runtime.begin_execution(&owner, Default::default()).unwrap();
    {
        let runtime = session.runtime_mut();
        let id = runtime.resources.active_session().unwrap().id;
        let error = runtime
            .memory
            .scope(&runtime.resources, |cx, roots| {
                ExecutionRoots::with(roots, cx, |slots| {
                    slots.grow(id, ValueBank::Local, 8 * 1024 * 1024)
                })
            })
            .unwrap_err();
        let error = runtime.managed_memory_error(error);
        assert_eq!(error.kind(), RuntimeErrorKind::StackOverflow);
        assert_eq!(runtime.resources.poll_execution().unwrap_err(), error);
    }
    drop(session);
    assert!(!runtime.is_quarantined());
    let fresh = runtime.begin_execution(&owner, Default::default()).unwrap();
    assert!(fresh.runtime().resources.poll_execution().is_ok());
}
