use super::{PendingOperation, PendingWait};
use crate::{
    Runtime,
    error::RuntimeErrorKind,
    execution::roots::{ExecutionRoots, ValueLocation, pending::PendingSlots},
    frame::{ExecutionStack, owned::tests::module},
    host::HostObjectId,
    native::{
        binding::NativeResult,
        future::{PendingNative, storage::ColdObject},
    },
    test_support::layouts::host_values,
    value::{
        aggregates::tuples::TupleObject,
        carriers::{RootedValue, ScopedValue},
        cells::CellObject,
        place::{PublishedResult, ResultPlace},
    },
};
use kagari_abi::representation::ValueType;
use kagari_bytecode::instruction::Slot;
use kagari_contract::ids::FunctionRef;
use kagari_gc::root::Root;
use kagari_types::{scalar::BuiltinType, ty::Ty};
use std::{
    mem,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    task::Poll,
};

#[derive(Debug)]
struct MutatingPoll {
    first_poll: bool,
    awaitable: ValueLocation,
    path: RootedValue,
    cancelled: Arc<AtomicUsize>,
}

impl PendingNative for MutatingPoll {
    fn conversion_source(&self) -> Option<&Root<ColdObject>> {
        None
    }

    fn poll<'result>(
        &mut self,
        runtime: &mut Runtime,
        _: ResultPlace<'result>,
    ) -> NativeResult<Poll<PublishedResult<'result>>> {
        if mem::take(&mut self.first_poll) {
            return Ok(Poll::Pending);
        }
        runtime
            .memory
            .scope(&runtime.resources, |cx, roots| {
                ExecutionRoots::with(roots, cx, |slots| {
                    let cell = slots.get(self.awaitable)?;
                    let path = self.path.get(slots.scope())?;
                    let tuple = TupleObject::alloc(slots.scope(), &[cell, path])?;
                    slots.set(self.awaitable, ScopedValue::managed(tuple))
                })
            })
            .map_err(|error| runtime.managed_memory_error(error))?;
        Ok(Poll::Pending)
    }

    fn cancel(&mut self) -> NativeResult<()> {
        self.cancelled.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

#[test]
fn waiting_commit_rechecks_graph_after_poll_mutation() {
    let mut runtime = Runtime::default();
    let module = module(&mut runtime, 1, false);
    let baseline = runtime.collected_gc_stats();
    let (host, path) = host_values(&mut runtime, 7);
    let handle = runtime.host().root(HostObjectId(7)).unwrap();
    let owner = runtime
        .start_owned_execution(&module, FunctionRef::new(0), &[], Default::default())
        .unwrap();
    let mut activation = Some(runtime.resume_owned_execution(&owner).unwrap());
    let cancelled = Arc::new(AtomicUsize::new(0));
    activation
        .as_mut()
        .unwrap()
        .with(|runtime, stack| {
            let (frame, location) = {
                let frame = stack.current(runtime)?;
                (
                    frame.slots(),
                    frame.slot_location(runtime.resources(), Slot::new(0), false)?,
                )
            };
            let pending = PendingSlots::new(stack.session);
            runtime
                .memory
                .scope(&runtime.resources, |cx, roots| {
                    ExecutionRoots::with(roots, cx, |slots| {
                        let value = host.get(slots.scope())?;
                        let cell = CellObject::alloc(
                            slots.scope(),
                            ValueType::Generic,
                            value,
                            Default::default(),
                        )?;
                        frame.write(slots, location, ScopedValue::managed(cell))?;
                        slots.set(pending.awaitable(), ScopedValue::managed(cell))?;
                        Ok(())
                    })
                })
                .map_err(|error| runtime.managed_memory_error(error))?;
            assert!(stack.can_park(runtime)?);
            let output = runtime
                .resolve_type_arguments(&module, &[Ty::Builtin(BuiltinType::Unit)])?
                .remove(0);
            *stack.session_state(runtime).pending.borrow_mut() = Some(PendingWait {
                destination: None,
                pending: PendingOperation::Native(Box::new(MutatingPoll {
                    first_poll: true,
                    awaitable: pending.awaitable(),
                    path,
                    cancelled: cancelled.clone(),
                })),
                output,
                slots: pending,
            });
            assert!(matches!(stack.poll_await(runtime)?, Poll::Pending));
            Ok(())
        })
        .unwrap();
    ExecutionStack::park_waiting(&mut activation).unwrap();
    drop(activation);
    drop(host);
    runtime.collect_garbage().unwrap();
    runtime.retire_host_root(handle).unwrap();
    let replacement = runtime
        .register_host_root(HostObjectId(8), handle.type_id(), handle.schema_epoch())
        .unwrap();
    let mut activation = Some(runtime.resume_owned_execution(&owner).unwrap());
    activation
        .as_mut()
        .unwrap()
        .with(|runtime, stack| {
            // Parking is still legal. Only access checks whether the captured ID is live.
            assert!(stack.can_park(runtime)?);
            let stale = runtime
                .memory
                .scope(&runtime.resources, |cx, roots| {
                    ExecutionRoots::with(roots, cx, |slots| {
                        let value = slots.get(PendingSlots::new(stack.session).awaitable())?;
                        let cell = slots
                            .scope()
                            .downcast::<CellObject>(value.object().unwrap())?;
                        let host = CellObject::get(slots.scope(), cell, ValueType::Generic)?;
                        host.retain(slots.scope())
                    })
                })
                .unwrap();
            assert_eq!(
                runtime.host_scope(&[stale]).err().unwrap().kind(),
                RuntimeErrorKind::HostCallFailure
            );
            assert_eq!(
                runtime.host().root(replacement.object_id()),
                Some(replacement)
            );
            assert!(matches!(stack.poll_await(runtime)?, Poll::Pending));
            assert!(!stack.can_park(runtime)?);
            Ok(())
        })
        .unwrap();
    let error = ExecutionStack::park_waiting(&mut activation).unwrap_err();
    assert_eq!(error.kind(), RuntimeErrorKind::ModuleValidation);
    assert!(error.to_string().contains("no suspendable pending wait"));
    assert!(activation.is_some());
    assert!(!owner.is_ready());
    activation
        .as_mut()
        .unwrap()
        .with(|runtime, _| {
            assert!(!runtime.is_quarantined());
            assert_eq!(runtime.capture_error_trace().frames.len(), 1);
            Ok(())
        })
        .unwrap();
    drop(activation);
    runtime.finish_owned_execution(&owner).unwrap();
    assert_eq!(cancelled.load(Ordering::SeqCst), 1);
    assert_eq!(runtime.resources().counters().current_call_depth, 0);
    let after = runtime.collected_gc_stats();
    assert_eq!(after.root_registrations, baseline.root_registrations);
    assert_eq!(after.objects, baseline.objects);
}
