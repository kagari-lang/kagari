#[cfg(test)]
use crate::test_support::bytecode::load_bytecode;
use crate::test_support::bytecode::reload_bytecode;
use crate::{
    execution::owned::DriveResult,
    interpreter_tests::{common::compile_with_provider, common::test_runtime},
};

use crate::{
    module::entry::ProgramEntry,
    native::{
        binding::NativeResult, builder::ModuleBuilder, declarations::FunctionDecl,
        typed::NativeContext,
    },
    session::ExecutionOptions,
    value::Value,
};
use std::{
    num::NonZeroUsize,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

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
        compile_with_provider(&source(100, 10, 1, 1, 1), Some(&module)),
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
        compile_with_provider(&source(1000, 20, 4, 3, 2), Some(&module)),
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
