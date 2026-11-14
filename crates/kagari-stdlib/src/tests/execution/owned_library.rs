use crate::tests::execution::common::{
    compile_logging_test_bytecode, logging_module, round_trip_bytecode, standard_runtime,
};

#[cfg(test)]
use crate::tests::support::bytecode::load_bytecode;
use kagari_runtime::{
    Runtime, RuntimeConfig,
    error::RuntimeErrorKind,
    execution::{owned::DriveResult, reentry::reenter},
    frame::ExecutionStack,
    module::entry::ProgramEntry,
    native::conversion::context::ConversionContext,
    session::ExecutionOptions,
    value::{carriers::RootedValue, view::ValueView},
};

use kagari_common::cancellation::CancellationToken;
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
    let program = compile_logging_test_bytecode(include_str!(
        "../../../../kagari-runtime/tests/fixtures/owned/owned-drive-state.kgr"
    ));
    let ping = program.records().modules[program.records().root.index()]
        .functions
        .iter()
        .find(|function| function.name == "ping")
        .unwrap()
        .id;
    let events = Arc::new(Mutex::new(Vec::new()));
    let received = events.clone();
    let mut runtime = standard_runtime(RuntimeConfig {
        ..Default::default()
    });
    logging_module(move |context, args| {
        let root = context.runtime().execution_root().unwrap();
        assert!(
            context
                .runtime_mut()
                .start_owned_execution(&root, ping, &[], ExecutionOptions::default())
                .is_err()
        );
        let arguments = context.retain_temporaries(&[]).unwrap();
        let nested = reenter(context, &root, ping, arguments).unwrap();
        assert_eq!(nested.as_i32(), Some(7));
        let text = context.read_string(&args[0], str::to_owned).unwrap();
        received.lock().unwrap().push(text);
        Ok(ValueView::unit())
    })
    .install(&mut runtime)
    .unwrap();
    let loaded = load_bytecode(&mut runtime, "owned", program).unwrap();
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();
    let mut vm = runtime;
    let synchronous = vm
        .execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
        .unwrap();
    let array_output = |vm: &mut Runtime, value: &RootedValue| {
        let mut cx = ConversionContext::new(vm, &loaded).unwrap();
        let value = cx.import(value).unwrap();
        (0..cx.array_len(&value).unwrap())
            .map(|index| {
                cx.array_element(&value, index)
                    .unwrap()
                    .unwrap()
                    .as_i32()
                    .unwrap()
            })
            .collect::<Vec<_>>()
    };
    let expected = array_output(&mut vm, &synchronous.return_value);
    assert_eq!(expected, [9, 1]);
    drop(synchronous);
    events.lock().unwrap().clear();
    let first = vm
        .start(
            &loaded,
            ProgramEntry::Main,
            &[],
            ExecutionOptions::default(),
        )
        .unwrap();
    let second = vm
        .start(
            &loaded,
            ProgramEntry::Main,
            &[],
            ExecutionOptions::default(),
        )
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
                DriveResult::Waiting => panic!("synchronous loop cannot wait"),
                DriveResult::Complete(result) => {
                    let value = result.unwrap();
                    assert_eq!(array_output(&mut vm, &value), expected);
                    complete[index] = true;
                }
            }
            assert!(vm.execution_root().is_none());
            assert_eq!(vm.resources().counters().current_call_depth, 0);
            vm.collect_garbage().unwrap();
        }
        if complete.iter().all(|done| *done) {
            break;
        }
    }
    assert_eq!(complete, [true, true]);
    assert_eq!(
        *events.lock().unwrap(),
        ["step"; 6],
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
    let DriveResult::Complete(Err(error)) = vm.drive(&spin, slice).unwrap() else {
        panic!("cancelled execution must terminate")
    };
    assert_eq!(error.kind(), RuntimeErrorKind::Cancelled);

    let cancellation = CancellationToken::default();
    let queued = vm
        .start(
            &loaded,
            ProgramEntry::Main,
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
    let activation_activation = vm.resume_owned_execution(&queued).unwrap();
    {
        assert!(!queued.is_ready());
        let before = wakes.0.load(Ordering::Relaxed);
        cancellation.cancel();
        assert!(queued.is_ready());
        assert!(
            wakes.0.load(Ordering::Relaxed) > before,
            "external token must wake the owned execution"
        );
        assert!(ExecutionStack::try_park_ready(&mut Some(activation_activation)).unwrap());
        assert!(matches!(
            vm.drive(&queued, slice).unwrap(),
            DriveResult::Complete(Err(error)) if error.kind() == RuntimeErrorKind::Cancelled
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
                assert_eq!(error.kind(), RuntimeErrorKind::ResourceLimitExceeded);
                exhausted = true;
                break;
            }
        }
        assert!(exhausted, "slicing cannot reset call-depth protection");

        let abandoned = vm
            .start(
                &loaded,
                ProgramEntry::Main,
                &[],
                ExecutionOptions::default(),
            )
            .unwrap();
        assert!(matches!(
            vm.drive(&abandoned, slice).unwrap(),
            DriveResult::Runnable
        ));
        drop(abandoned);
        assert_eq!(vm.drain_retired_executions().unwrap(), 1);
        assert_eq!(vm.modules().retention_counts(loaded.key()).active_calls, 0);
        assert!(!vm.resources().is_quarantined());
        let result = vm
            .execute_module(&loaded, "ping", vm.execution_options())
            .unwrap();
        assert_eq!(result.return_value.as_i32(), Some(7));
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
}

#[test]
fn async_owned_drive_contract_iterator_invalidation() {
    let program = compile_logging_test_bytecode(include_str!(
        "../../../tests/fixtures/execution/owned-iterator-invalidation.kgr"
    ));
    let artifact = round_trip_bytecode(program);
    let seen = Arc::new(Mutex::new(0));
    let observed = seen.clone();
    let mut runtime = standard_runtime(Default::default());
    logging_module(move |_, _| {
        *observed.lock().unwrap() += 1;
        Ok(ValueView::unit())
    })
    .install(&mut runtime)
    .unwrap();
    let loaded = load_bytecode(&mut runtime, "owned-iteration", artifact).unwrap();
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();
    let mut vm = runtime;
    let edit = |vm: &mut Runtime, name: &str, value: &RootedValue| {
        crate::tests::support::invoke(vm, &loaded, name, std::slice::from_ref(value))
    };
    vm.collect_garbage().unwrap();
    let baseline = vm.gc_stats();
    let slice = NonZeroUsize::new(1).unwrap();
    // Each case parks an actual collection loop after its first item. Normal
    // completion, invalidation, cancellation and owner drop retain no native borrows.
    for exit in 0..4 {
        *seen.lock().unwrap() = 0;
        let source = vm
            .execute_module(&loaded, "make", vm.execution_options())
            .unwrap()
            .return_value;
        let owner = vm
            .start(
                &loaded,
                "sum",
                std::slice::from_ref(&source),
                ExecutionOptions::default(),
            )
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
        vm.collect_garbage().unwrap();
        assert_eq!(
            vm.execute_module(&loaded, "ping", vm.execution_options())
                .unwrap()
                .return_value
                .as_i32(),
            Some(42)
        );
        if exit != 0 {
            drop(edit(&mut vm, "append", &source));
        }
        drop(edit(&mut vm, "replace", &source));
        match exit {
            0 => {
                let mut finished = false;
                for _ in 0..100 {
                    if let DriveResult::Complete(result) = vm.drive(&owner, slice).unwrap() {
                        assert_eq!(result.unwrap().as_i32(), Some(24));
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
                    DriveResult::Complete(Err(error)) if error.kind() == RuntimeErrorKind::Cancelled
                ));
            }
            2 => {
                let mut finished = false;
                for _ in 0..100 {
                    if let DriveResult::Complete(result) = vm.drive(&owner, slice).unwrap() {
                        assert!(
                            format!("{:?}", result.unwrap_err())
                                .contains("structurally modified iterator")
                        );
                        finished = true;
                        break;
                    }
                }
                assert!(finished);
                assert_eq!(
                    *seen.lock().unwrap(),
                    1,
                    "no second item after invalidation"
                );
                assert_eq!(edit(&mut vm, "length", &source).as_u64(), Some(4));
            }
            _ => {
                drop(owner);
                assert_eq!(vm.drain_retired_executions().unwrap(), 1);
            }
        }
        drop(edit(&mut vm, "append", &source));
        assert_eq!(vm.resources().counters().current_call_depth, 0);
        assert_eq!(vm.modules().retention_counts(loaded.key()).active_calls, 0);
        drop(source);
        vm.collect_garbage().unwrap();
        assert_eq!(
            vm.gc_stats().root_registrations,
            baseline.root_registrations
        );
        assert_eq!(vm.gc_stats().objects, baseline.objects);
    }
}
