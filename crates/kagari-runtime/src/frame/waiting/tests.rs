use super::{PendingOperation, PendingWait};
use crate::{
    Runtime,
    error::RuntimeErrorKind,
    execution::roots::{ExecutionRoots, ValueLocation, pending::PendingSlots},
    frame::{ExecutionStack, owned::tests::module},
    host::{HostObjectId, HostSchemaEpoch},
    native::{binding::NativeResult, builder::ModuleBuilder, future::PendingNative},
    value::{
        aggregates::tuples::TupleObject,
        carriers::ScopedValue,
        cells::CellObject,
        host_values::HostRootObject,
        place::{PublishedResult, ResultPlace},
        strings::StringObject,
    },
};
use kagari_abi::representation::ValueType;
use kagari_bytecode::instruction::Slot;
use kagari_contract::ids::FunctionRef;
use kagari_gc::root::Root;
use kagari_types::{
    declaration::host::{HostReflectionPolicy, HostTypeOwnership, HostTypePolicy, PathAccess},
    scalar::BuiltinType,
    ty::Ty,
};
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    task::Poll,
};

#[derive(Debug)]
struct MutatingPoll {
    cell: Root<CellObject>,
    awaitable: ValueLocation,
    external: Root<HostRootObject>,
    cancelled: Arc<AtomicUsize>,
}

impl PendingNative for MutatingPoll {
    fn poll<'result>(
        &mut self,
        runtime: &mut Runtime,
        _: ResultPlace<'result>,
    ) -> NativeResult<Poll<PublishedResult<'result>>> {
        runtime
            .memory
            .scope(&runtime.resources, |cx, roots| {
                ExecutionRoots::with(roots, cx, |slots| {
                    let cell = slots.scope().get(&self.cell)?;
                    let external = slots.scope().get(&self.external)?;
                    let tuple = TupleObject::alloc(
                        slots.scope(),
                        &[ScopedValue::managed(cell), ScopedValue::managed(external)],
                    )?;
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
    let mut library = ModuleBuilder::new("waiting", &Default::default()).unwrap();
    let mut external = library.define_type("External").unwrap();
    external
        .external(HostTypePolicy {
            ownership: HostTypeOwnership::HostRoot,
            reflection: HostReflectionPolicy::Hidden,
            path_access: PathAccess::ReadOnly,
        })
        .unwrap();
    external.finish().unwrap();
    library.finish().unwrap().install(&mut runtime).unwrap();
    let ty = runtime.types().get_by_name("waiting::External").unwrap().id;
    let handle = runtime
        .register_host_root(HostObjectId(7), ty, HostSchemaEpoch::new(0))
        .unwrap();
    let module = module(&mut runtime, 1, false);
    let baseline = runtime.collected_gc_stats().root_registrations;
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
            let (cell, external) = runtime
                .memory
                .scope(&runtime.resources, |cx, roots| {
                    ExecutionRoots::with(roots, cx, |slots| {
                        let text =
                            StringObject::alloc(slots.scope(), "safe before polling".into())?;
                        let cell = CellObject::alloc(
                            slots.scope(),
                            ValueType::Generic,
                            ScopedValue::managed(text),
                            Default::default(),
                        )?;
                        frame.write(slots, location, ScopedValue::managed(cell))?;
                        slots.set(pending.awaitable(), ScopedValue::managed(cell))?;
                        let external = HostRootObject::alloc(slots.scope(), handle)?;
                        Ok((slots.scope().retain(cell)?, slots.scope().retain(external)?))
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
                    cell,
                    awaitable: pending.awaitable(),
                    external,
                    cancelled: cancelled.clone(),
                })),
                output,
                slots: pending,
            });
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
    assert_eq!(runtime.collected_gc_stats().root_registrations, baseline);
}
