#[cfg(test)]
use crate::test_support::bytecode::load_bytecode;
use crate::test_support::bytecode::reload_bytecode;
use crate::{
    Runtime, RuntimeConfig,
    error::RuntimeErrorKind,
    execution::{owned::DriveResult, reentry::reenter},
    frame::ExecutionStack,
    host::HostCallContext,
    interpreter_tests::common::{compile_with_provider, test_runtime},
    module::entry::ProgramEntry,
    native::{
        binding::NativeResult, builder::ModuleBuilder, conversion::context::ConversionContext,
        declarations::FunctionDecl, host::callback, typed::NativeContext, types::Type,
    },
    resource::RuntimeLimits,
    session::ExecutionOptions,
    value::{carriers::RootedValue, view::ValueView},
};
use kagari_common::cancellation::CancellationToken;
use kagari_types::{
    declaration::host::{HostCallPolicy, HostFunctionEffects, HostPassingStyle},
    scalar::BuiltinType,
    ty::Ty,
};
use std::{
    num::NonZeroUsize,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    task::{Wake, Waker},
};

#[test]
fn submitted_arguments_and_resumed_frames_preserve_retained_and_current_calls() {
    let effects = Arc::new(AtomicUsize::new(0));
    let observed = effects.clone();
    let mut runtime = test_runtime(Default::default());
    let mut builder = ModuleBuilder::new("example::steps", &Default::default()).unwrap();
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
        compile_with_provider(
            include_str!("../../tests/fixtures/owned/submitted-calls.kgr"),
            Some(&module),
        ),
    )
    .unwrap();
    let old = runtime.select_program(&program).unwrap();
    let mut vm = runtime;
    let argument = vm
        .alloc_array(
            &old,
            Ty::Builtin(BuiltinType::I32),
            &[RootedValue::from(0i32)],
        )
        .unwrap();
    // Retire construction leases before comparing live submission ownership.
    vm.collect_garbage().unwrap();
    let roots = vm.gc_stats().root_registrations;
    let execution = vm
        .start(
            &old,
            "run",
            std::slice::from_ref(&argument),
            ExecutionOptions::default(),
        )
        .unwrap();
    assert_eq!(
        vm.gc_stats().root_registrations,
        roots,
        "submission copies into execution storage without another Host root"
    );
    drop(argument);
    vm.collect_garbage().unwrap();
    assert_eq!(vm.gc_stats().root_registrations + 1, roots);

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
        compile_with_provider(
            include_str!("../../tests/fixtures/owned/submitted-calls-next.kgr"),
            Some(&module),
        ),
    )
    .unwrap();
    vm.collect_garbage().unwrap();
    let mut result = None;
    for _ in 0..500 {
        match vm.drive(&execution, slice).unwrap() {
            DriveResult::Runnable => {}
            DriveResult::Waiting => panic!("synchronous calls cannot wait"),
            DriveResult::Complete(value) => {
                result = value.unwrap().as_i32();
                break;
            }
        }
        vm.collect_garbage().unwrap();
    }
    // Both suspended bodies keep their state: 100 + (10 + 20 + 2).
    // The retained caller's ordinary qualified forward call returns 4, not the old 1;
    // its module and shared calls likewise select current bodies.
    assert_eq!(result, Some(145));
    assert_eq!(
        effects.load(Ordering::Relaxed),
        3,
        "native effects execute once"
    );
    assert!(vm.execution_root().is_none());
    assert_eq!(vm.resources().counters().current_call_depth, 0);
    assert!(!vm.is_quarantined());
}

#[derive(Default)]
struct WakeCount(AtomicUsize);

impl Wake for WakeCount {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
}

#[test]
fn async_owned_drive_contract() {
    let mut builder = ModuleBuilder::new("test::drive", &Default::default()).unwrap();
    let step_policy = HostCallPolicy {
        passing: vec![HostPassingStyle::SharedBorrow],
        effects: HostFunctionEffects {
            may_trap: true,
            may_call_host_services: true,
            may_mutate_host_state: true,
            ..Default::default()
        },
    };
    let step = builder
        .define_function(
            FunctionDecl::new("step")
                .parameter("message", Type::scalar(BuiltinType::String))
                .host_policy(step_policy.clone()),
        )
        .unwrap();
    let mut producer = builder.clone();
    producer
        .bind(
            step.clone(),
            callback(step_policy.clone(), |_: &mut HostCallContext<'_>, _| {
                Ok(ValueView::unit())
            }),
        )
        .unwrap();
    let program = compile_with_provider(
        include_str!("../../tests/fixtures/owned/owned-drive-state.kgr"),
        Some(&producer.finish().unwrap()),
    );
    let ping = program.records().modules[program.records().root.index()]
        .functions
        .iter()
        .find(|function| function.name == "ping")
        .unwrap()
        .id;
    let events = Arc::new(Mutex::new(Vec::new()));
    let received = events.clone();
    let mut runtime = test_runtime(RuntimeConfig {
        limits: RuntimeLimits {
            max_stack_bytes: NonZeroUsize::new(16 * 1024).unwrap(),
            ..Default::default()
        },
        ..Default::default()
    });
    builder
        .bind(
            step,
            callback(
                step_policy,
                move |context: &mut HostCallContext<'_>, args| {
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
                },
            ),
        )
        .unwrap();
    builder.finish().unwrap().install(&mut runtime).unwrap();
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
    let activation = vm.resume_owned_execution(&queued).unwrap();
    {
        assert!(!queued.is_ready());
        let before = wakes.0.load(Ordering::Relaxed);
        cancellation.cancel();
        assert!(queued.is_ready());
        assert!(
            wakes.0.load(Ordering::Relaxed) > before,
            "external token must wake the owned execution"
        );
        assert!(ExecutionStack::try_park_ready(&mut Some(activation)).unwrap());
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
        for _ in 0..1000 {
            match vm.drive(&recursive, slice).unwrap() {
                DriveResult::Runnable => {}
                DriveResult::Waiting => panic!("synchronous recursion cannot wait"),
                DriveResult::Complete(result) => {
                    assert_eq!(result.unwrap_err().kind(), RuntimeErrorKind::StackOverflow);
                    exhausted = true;
                    break;
                }
            }
        }
        assert!(exhausted, "slicing cannot reset stack-space protection");

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
