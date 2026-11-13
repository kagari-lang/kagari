#[cfg(test)]
use crate::test_support::bytecode::load_bytecode;
use crate::test_support::bytecode::reload_bytecode;
use crate::{
    execution::owned::DriveResult,
    interpreter_tests::{common::compile_with_provider, common::test_runtime},
};

use crate::{
    native::{
        binding::NativeResult, builder::ModuleBuilder, declarations::FunctionDecl,
        typed::NativeContext,
    },
    session::ExecutionOptions,
    value::carriers::RootedValue,
};
use kagari_types::{scalar::BuiltinType, ty::Ty};
use std::{
    num::NonZeroUsize,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
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
    // Their subsequent ordinary calls use the new local/module/shared bodies.
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
