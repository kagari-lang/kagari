use super::{PendingOperation, PendingWait};
use crate::{
    Runtime,
    error::RuntimeErrorKind,
    frame::{
        ExecutionStack,
        owned::tests::{module, set_frame},
    },
    gc::{reference::GcRef, retention_probe::replace_cell},
    native::{binding::NativeResult, future::PendingNative},
    value::{EphemeralValueId, Value},
};
use kagari_abi::representation::ValueType;
use kagari_contract::ids::FunctionRef;
use kagari_types::{scalar::BuiltinType, ty::Ty};
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    task::Poll,
};

#[derive(Debug)]
struct MutatingPoll {
    cell: GcRef,
    cancelled: Arc<AtomicUsize>,
}

impl PendingNative for MutatingPoll {
    fn poll(&mut self, runtime: &Runtime) -> NativeResult<Poll<Value>> {
        replace_cell(
            runtime.gc(),
            self.cell,
            Value::RuntimeEphemeral(EphemeralValueId(1)),
        );
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
    let owner = runtime
        .start_owned_execution(&module, FunctionRef::new(0), &[], Default::default())
        .unwrap();
    let mut activation = Some(runtime.resume_owned_execution(&owner).unwrap());
    let stack = activation.as_ref().unwrap();
    let cell = runtime
        .gc()
        .alloc_cell(ValueType::Unit, Value::Unit)
        .unwrap();
    set_frame(&runtime, stack, Value::Cell(cell));
    assert!(stack.can_park(&runtime).unwrap());
    let output = runtime
        .resolve_type_arguments(&module, &[Ty::Builtin(BuiltinType::Unit)])
        .unwrap()
        .remove(0);
    let cancelled = Arc::new(AtomicUsize::new(0));
    *stack.session.state().pending.borrow_mut() = Some(PendingWait {
        destination: None,
        pending: PendingOperation::Native(Box::new(MutatingPoll {
            cell,
            cancelled: cancelled.clone(),
        })),
        output,
        awaitable: runtime.root_value(Value::Cell(cell)).unwrap(),
    });
    assert!(matches!(stack.poll_await(&runtime).unwrap(), Poll::Pending));
    let error = ExecutionStack::park_waiting(&mut activation, &runtime).unwrap_err();
    assert_eq!(error.kind(), RuntimeErrorKind::ScriptTrap);
    assert!(activation.is_some());
    assert!(!owner.is_ready());
    assert_eq!(runtime.capture_error_trace().frames.len(), 1);
    drop(activation);
    runtime.finish_owned_execution(&owner).unwrap();
    assert_eq!(cancelled.load(Ordering::SeqCst), 1);
    assert_eq!(runtime.resources().counters().current_call_depth, 0);
    assert_eq!(runtime.gc().active_roots(), 0);
}
