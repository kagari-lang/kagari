use crate::interpreter_tests::common::logging_module;
use crate::interpreter_tests::common::round_trip_bytecode;
#[cfg(test)]
use crate::test_support::bytecode::load_bytecode;
use crate::test_support::bytecode::reload_bytecode;
use crate::{
    execution::owned::DriveResult,
    execution::reentry::reenter,
    executor::Executor,
    interpreter_tests::{
        common::{compile_logging_test_bytecode, standard_runtime},
        native_boundary::compile_without_foundation,
    },
};

use crate::{
    RuntimeConfig,
    error::RuntimeErrorKind,
    frame::ExecutionStack,
    module::entry::ProgramEntry,
    native::{
        binding::NativeResult, builder::ModuleBuilder, declarations::FunctionDecl,
        typed::NativeContext,
    },
    resource::RuntimeLimits,
    session::ExecutionOptions,
    value::Value,
};
use kagari_common::cancellation::CancellationToken;
use kagari_stdlib::declarations::StandardDeclarations;
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
fn resumed_frames_select_current_direct_recursive_shared_and_module_calls() {
    let source = |base, step, local, remote, shared| {
        format!(
            r#"
        mod dependency {{ pub fn value() -> i32 {{ {remote} }} }}
        fn local() -> i32 {{ {local} }}
        fn shared<T>(value: T, delta: i32) -> i32 {{ delta + {shared} }}
        fn recursive(n: i32) -> i32 {{
            example::steps::step();
            if n == 0 {{ 2 }} else {{ {step} + recursive(n - 1) }}
        }}
        fn main() -> i32 {{
            val kept = {base};
            val result = recursive(2);
            kept + result + local() + dependency::value() + shared(0, 4)
        }}
        "#
        )
    };
    let effects = Arc::new(AtomicUsize::new(0));
    let observed = effects.clone();
    let mut runtime = standard_runtime(Default::default());
    let mut builder = ModuleBuilder::new(
        "example::steps",
        &StandardDeclarations::default().catalog().unwrap(),
    )
    .unwrap();
    let step = builder.define_function(FunctionDecl::new("step")).unwrap();
    builder
        .bind(
            step,
            move |_: &mut NativeContext<'_>, (): ()| -> NativeResult<()> {
                observed.fetch_add(1, Ordering::Relaxed);
                Ok(())
            },
        )
        .unwrap();
    let module = builder.finish().unwrap();
    module.install(&mut runtime).unwrap();
    let program = load_bytecode(
        &mut runtime,
        "current-calls",
        compile_without_foundation(&source(100, 10, 1, 1, 1), Some(&module)),
    )
    .unwrap();
    let old = runtime.select_program(&program).unwrap();
    let mut vm = runtime;
    let execution = vm
        .start(&old, ProgramEntry::Main, &[], ExecutionOptions::default())
        .unwrap();
    let slice = NonZeroUsize::new(1).unwrap();
    for _ in 0..100 {
        assert!(matches!(
            vm.drive(&execution, slice).unwrap(),
            DriveResult::Runnable
        ));
        if effects.load(Ordering::Relaxed) == 1 {
            break;
        }
    }
    assert_eq!(
        effects.load(Ordering::Relaxed),
        1,
        "park inside the old recursive body"
    );
    reload_bytecode(
        &mut vm,
        &program,
        None,
        compile_without_foundation(&source(1000, 20, 4, 3, 2), Some(&module)),
    )
    .unwrap();
    vm.collect_garbage().unwrap();
    let mut result = None;
    for _ in 0..500 {
        match vm.drive(&execution, slice).unwrap() {
            DriveResult::Runnable => {}
            DriveResult::Waiting => panic!("synchronous calls cannot wait"),
            DriveResult::Complete(value) => {
                result = value.unwrap().value(vm.gc());
                break;
            }
        }
        vm.collect_garbage().unwrap();
    }
    // Both suspended bodies keep their state: 100 + (10 + 20 + 2).
    // Their subsequent ordinary calls use the new local/module/shared bodies.
    assert_eq!(result, Some(Value::I32(145)));
    assert_eq!(
        effects.load(Ordering::Relaxed),
        3,
        "native effects execute once"
    );
    assert!(vm.execution_root().is_none());
    assert_eq!(vm.resources().counters().current_call_depth, 0);
    assert!(!vm.is_quarantined());
}

#[test]
fn async_owned_drive_contract() {
    let program = compile_logging_test_bytecode(include_str!(
        "../../tests/fixtures/owned/owned-drive-state.kgr"
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
        limits: RuntimeLimits {
            max_call_depth: Some(8),
        },
        ..Default::default()
    });
    logging_module(move |context, args| {
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
    })
    .install(&mut runtime)
    .unwrap();
    let loaded = load_bytecode(&mut runtime, "owned", program).unwrap();
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();
    let vm = runtime;
    let synchronous = vm
        .execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
        .unwrap();
    let Value::Array(expected) = synchronous.return_value.value(vm.gc()).unwrap() else {
        panic!("array output")
    };
    let expected = vm.gc().array_snapshot(expected).unwrap();
    assert_eq!(expected, vec![Value::I32(9), Value::I32(1)]);
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
                    let Value::Array(id) = value.value(vm.gc()).unwrap() else {
                        panic!("array output")
                    };
                    assert_eq!(vm.gc().array_snapshot(id).unwrap(), expected);
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
    let activation = vm.resume_owned_execution(&queued).unwrap();
    assert!(!queued.is_ready());
    let before = wakes.0.load(Ordering::Relaxed);
    cancellation.cancel();
    assert!(queued.is_ready());
    assert!(
        wakes.0.load(Ordering::Relaxed) > before,
        "external token must wake the owned execution"
    );
    assert!(ExecutionStack::try_park_ready(&mut Some(activation), &vm).unwrap());
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
    assert_eq!(result.return_value.value(vm.gc()).unwrap(), Value::I32(7));
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
fn async_owned_drive_contract_iterator_invalidation() {
    let program = compile_logging_test_bytecode(
        r#"
        pub fn make() -> Vec<i32> { Vec::from([1, 2, 3]) }
        pub fn sum(items: Vec<i32>) -> i32 {
            var sum = 0;
            for value in items { sum += value; print("item"); }
            sum
        }
        pub fn ping() -> i32 { 42 }
        pub fn append(items: Vec<i32>) { items.push(9); }
        pub fn replace(items: Vec<i32>) { items[1] = 20; }
        pub fn length(items: Vec<i32>) -> u64 { items.len() }
    "#,
    );
    let artifact = round_trip_bytecode(program);
    let seen = Arc::new(Mutex::new(0));
    let observed = seen.clone();
    let mut runtime = standard_runtime(Default::default());
    logging_module(move |_, _| {
        *observed.lock().unwrap() += 1;
        Ok(Value::Unit)
    })
    .install(&mut runtime)
    .unwrap();
    let loaded = load_bytecode(&mut runtime, "owned-iteration", artifact).unwrap();
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();
    let vm = runtime;
    let edit = |name: &str, value: Value| {
        let (owner, function) = ProgramEntry::from(name).resolve(&loaded).unwrap();
        let runtime = &vm;
        let _session = runtime.begin_execution(&owner, Default::default()).unwrap();
        Executor::new(runtime, &owner, function, &[value])
            .unwrap()
            .run()
            .unwrap()
    };
    let slice = NonZeroUsize::new(1).unwrap();
    // Each case parks an actual collection loop after its first item. Normal
    // completion, invalidation, cancellation and owner drop retain no native borrows.
    for exit in 0..4 {
        *seen.lock().unwrap() = 0;
        let source = vm
            .execute_module(&loaded, "make", vm.execution_options())
            .unwrap()
            .return_value;
        let value = source.value(vm.gc()).unwrap();
        let Value::GcHandle(_) = value else {
            panic!("source Vec")
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
        vm.collect_garbage().unwrap();
        assert_eq!(
            vm.execute_module(&loaded, "ping", vm.execution_options())
                .unwrap()
                .return_value
                .value(vm.gc()),
            Some(Value::I32(42))
        );
        if exit != 0 {
            edit("append", value);
        }
        edit("replace", value);
        match exit {
            0 => {
                let mut finished = false;
                for _ in 0..100 {
                    if let DriveResult::Complete(result) = vm.drive(&owner, slice).unwrap() {
                        assert_eq!(result.unwrap().value(vm.gc()), Some(Value::I32(24)));
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
                assert_eq!(edit("length", value), Value::U64(4));
            }
            _ => {
                drop(owner);
                assert_eq!(vm.drain_retired_executions().unwrap(), 1);
            }
        }
        edit("append", value);
        assert_eq!(vm.resources().counters().current_call_depth, 0);
        assert_eq!(vm.modules().retention_counts(loaded.key()).active_calls, 0);
    }
}
