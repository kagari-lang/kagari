use crate::{
    error::VmError,
    reentry::reenter,
    tests::common::{compile_test_bytecode, standard_runtime},
    vm::{Vm, owned::DriveResult},
};
use kagari_bytecode::artifact::KbcArtifact;
use kagari_common::cancellation::CancellationToken;
use kagari_runtime::{
    RuntimeConfig, error::RuntimeErrorKind, host::HostFunction, resource::RuntimeLimits,
    session::ExecutionOptions, value::Value,
};
use kagari_types::host_interface::standard_log;
use std::{
    num::NonZeroUsize,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    task::{Wake, Waker},
};

#[derive(Default)]
struct WakeCount(AtomicUsize);

impl Wake for WakeCount {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
}

#[test]
fn async_owned_drive_contract() {
    let program = compile_test_bytecode(
        r#"
        fn helper(n: i32) -> i32 { n + 1 }
        fn main() -> Vec<i32> {
            val kept = [1, 2, 3];
            var sum = 0;
            for value in kept { sum += helper(value); print("step"); }
            [sum, kept[0]]
        }
        fn spin() { while true {} }
        fn recurse() -> i32 { recurse() }
        fn ping() -> i32 { 7 }
    "#,
    );
    let ping = program.modules[program.root.index()]
        .functions
        .iter()
        .find(|function| function.name == "ping")
        .unwrap()
        .id;
    let events = Arc::new(Mutex::new(Vec::new()));
    let received = events.clone();
    let mut runtime = standard_runtime(RuntimeConfig {
        limits: RuntimeLimits {
            max_call_depth: Some(8),
        },
        ..Default::default()
    });
    runtime
        .register_host_function(HostFunction::new(standard_log(), move |context, args| {
            let root = context.runtime().execution_root().unwrap();
            assert!(
                context
                    .runtime()
                    .start_owned_execution(&root, ping, &[], ExecutionOptions::default())
                    .is_err()
            );
            let nested = reenter(context, &root, ping, &[]).unwrap();
            assert_eq!(nested.value(context.runtime().gc()), Some(Value::I32(7)));
            received.lock().unwrap().extend_from_slice(args);
            Ok(Value::Unit)
        }))
        .unwrap();
    let loaded = runtime.load_program("owned", program).unwrap();
    let vm = Vm::new(runtime);
    let synchronous = vm.execute(&loaded, "main").unwrap();
    let Value::Array(expected) = synchronous.return_value.value(vm.runtime().gc()).unwrap() else {
        panic!("array output")
    };
    let expected = vm.runtime().gc().array_snapshot(expected).unwrap();
    events.lock().unwrap().clear();
    let first = vm
        .start(&loaded, "main", &[], ExecutionOptions::default())
        .unwrap();
    let second = vm
        .start(&loaded, "main", &[], ExecutionOptions::default())
        .unwrap();
    assert!(
        events.lock().unwrap().is_empty(),
        "start must not execute the body"
    );
    let mut complete = [false; 2];
    let slice = NonZeroUsize::new(1).unwrap();
    for _ in 0..1000 {
        for (index, owner) in [&first, &second].iter().enumerate() {
            if complete[index] {
                continue;
            }
            match vm.drive(owner, slice).unwrap() {
                DriveResult::Runnable => {}
                DriveResult::Complete(result) => {
                    let value = result.unwrap();
                    let Value::Array(id) = value.value(vm.runtime().gc()).unwrap() else {
                        panic!("array output")
                    };
                    assert_eq!(vm.runtime().gc().array_snapshot(id).unwrap(), expected);
                    complete[index] = true;
                }
            }
            assert!(vm.runtime().execution_root().is_none());
            assert_eq!(vm.runtime().resources().counters().current_call_depth, 0);
            vm.runtime().collect_garbage().unwrap();
        }
        if complete.iter().all(|done| *done) {
            break;
        }
    }
    assert_eq!(complete, [true, true]);
    assert_eq!(
        events.lock().unwrap().len(),
        6,
        "slices must not repeat effects"
    );
    assert!(
        vm.drive(&first, slice).is_err(),
        "retired handle must not reenter"
    );

    let spin = vm
        .start(&loaded, "spin", &[], ExecutionOptions::default())
        .unwrap();
    assert!(matches!(
        vm.drive(&spin, slice).unwrap(),
        DriveResult::Runnable
    ));
    spin.cancel();
    let DriveResult::Complete(Err(VmError::RuntimeError(error))) = vm.drive(&spin, slice).unwrap()
    else {
        panic!("cancelled execution must terminate")
    };
    assert_eq!(error.kind(), RuntimeErrorKind::Cancelled);

    let cancellation = CancellationToken::default();
    let queued = vm
        .start(
            &loaded,
            "main",
            &[],
            ExecutionOptions {
                cancellation: cancellation.clone(),
                ..Default::default()
            },
        )
        .unwrap();
    let wakes = Arc::new(WakeCount::default());
    queued.set_waker(&Waker::from(wakes.clone()));
    assert!(queued.is_ready());
    let activation = vm.runtime().resume_owned_execution(&queued).unwrap();
    assert!(!queued.is_ready());
    let before = wakes.0.load(Ordering::Relaxed);
    cancellation.cancel();
    assert!(queued.is_ready());
    assert!(
        wakes.0.load(Ordering::Relaxed) > before,
        "external token must wake the owned execution"
    );
    activation.park(vm.runtime()).unwrap();
    assert!(matches!(
        vm.drive(&queued, slice).unwrap(),
        DriveResult::Complete(Err(_))
    ));
    assert!(
        !queued.is_ready(),
        "terminal execution must not be scheduled again"
    );
    assert_eq!(events.lock().unwrap().len(), 6);

    let recursive = vm
        .start(&loaded, "recurse", &[], ExecutionOptions::default())
        .unwrap();
    let mut exhausted = false;
    for _ in 0..100 {
        if let DriveResult::Complete(Err(error)) = vm.drive(&recursive, slice).unwrap() {
            let VmError::RuntimeError(error) = error.cause() else {
                panic!("resource error")
            };
            assert_eq!(error.kind(), RuntimeErrorKind::ResourceLimitExceeded);
            exhausted = true;
            break;
        }
    }
    assert!(exhausted, "slicing cannot reset call-depth protection");

    let abandoned = vm
        .start(&loaded, "main", &[], ExecutionOptions::default())
        .unwrap();
    assert!(matches!(
        vm.drive(&abandoned, slice).unwrap(),
        DriveResult::Runnable
    ));
    drop(abandoned);
    assert_eq!(vm.runtime().drain_retired_executions().unwrap(), 1);
    assert_eq!(
        vm.runtime()
            .modules()
            .retention_counts(loaded.key())
            .active_calls,
        0
    );
    assert!(!vm.runtime().resources().is_quarantined());
    let result = vm.execute(&loaded, "ping").unwrap();
    assert_eq!(
        result.return_value.value(vm.runtime().gc()).unwrap(),
        Value::I32(7)
    );
    let shutdown = vm
        .start(&loaded, "spin", &[], ExecutionOptions::default())
        .unwrap();
    assert!(shutdown.is_ready());
    drop(vm);
    assert!(
        !shutdown.is_ready(),
        "runtime destruction retires outstanding readiness"
    );
}

#[test]
fn async_owned_drive_contract_iteration_leases() {
    let program = compile_test_bytecode(
        r#"
        fn make() -> Vec<i32> { [1, 2, 3] }
        fn sum(items: Vec<i32>) -> i32 {
            var sum = 0;
            for value in items { sum += value; print("item"); }
            sum
        }
        fn ping() -> i32 { 42 }
    "#,
    );
    let artifact = KbcArtifact::from_program(program, Default::default()).unwrap();
    let artifact = KbcArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
    artifact.validate_for_loader(&Default::default()).unwrap();
    let seen = Arc::new(Mutex::new(0));
    let observed = seen.clone();
    let mut runtime = standard_runtime(Default::default());
    runtime
        .register_host_function(HostFunction::new(standard_log(), move |_, _| {
            *observed.lock().unwrap() += 1;
            Ok(Value::Unit)
        }))
        .unwrap();
    let loaded = runtime
        .load_program("owned-iteration", artifact.program)
        .unwrap();
    let vm = Vm::new(runtime);
    let slice = NonZeroUsize::new(1).unwrap();
    // Each case parks an actual collection loop after its first item. Normal
    // completion, cancellation and owner drop must all release the same leases.
    for exit in 0..3 {
        *seen.lock().unwrap() = 0;
        let source = vm.execute(&loaded, "make").unwrap().return_value;
        let value = source.value(vm.runtime().gc()).unwrap();
        let Value::Array(id) = value else {
            panic!("source array")
        };
        let owner = vm
            .start(&loaded, "sum", &[value], ExecutionOptions::default())
            .unwrap();
        for _ in 0..100 {
            assert!(matches!(
                vm.drive(&owner, slice).unwrap(),
                DriveResult::Runnable
            ));
            if *seen.lock().unwrap() == 1 {
                break;
            }
        }
        assert_eq!(*seen.lock().unwrap(), 1);
        vm.runtime().collect_garbage().unwrap();
        assert_eq!(
            vm.execute(&loaded, "ping")
                .unwrap()
                .return_value
                .value(vm.runtime().gc()),
            Some(Value::I32(42))
        );
        let error = vm.runtime().gc().array_push(id, Value::I32(9)).unwrap_err();
        assert_eq!(error.kind(), RuntimeErrorKind::ScriptTrap);
        vm.runtime().gc().array_set(id, 1, Value::I32(20)).unwrap();
        match exit {
            0 => {
                let mut finished = false;
                for _ in 0..100 {
                    if let DriveResult::Complete(result) = vm.drive(&owner, slice).unwrap() {
                        assert_eq!(
                            result.unwrap().value(vm.runtime().gc()),
                            Some(Value::I32(24))
                        );
                        finished = true;
                        break;
                    }
                }
                assert!(finished);
                assert_eq!(*seen.lock().unwrap(), 3);
            }
            1 => {
                owner.cancel();
                assert!(matches!(
                    vm.drive(&owner, slice).unwrap(),
                    DriveResult::Complete(Err(_))
                ));
            }
            _ => {
                drop(owner);
                assert_eq!(vm.runtime().drain_retired_executions().unwrap(), 1);
            }
        }
        vm.runtime().gc().array_push(id, Value::I32(9)).unwrap();
        assert_eq!(vm.runtime().resources().counters().current_call_depth, 0);
        assert_eq!(
            vm.runtime()
                .modules()
                .retention_counts(loaded.key())
                .active_calls,
            0
        );
    }
}
