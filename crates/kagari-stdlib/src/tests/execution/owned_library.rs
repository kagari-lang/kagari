use crate::tests::execution::common::{
    compile_print_test_bytecode, print_module, round_trip_bytecode, standard_runtime,
};

#[cfg(test)]
use crate::tests::support::bytecode::load_bytecode;
use kagari_runtime::{
    Runtime, error::RuntimeErrorKind, execution::owned::DriveResult, session::ExecutionOptions,
    value::carriers::RootedValue,
};

use std::{
    num::NonZeroUsize,
    sync::{Arc, Mutex},
};

#[test]
fn async_owned_drive_contract_iterator_invalidation() {
    let program = compile_print_test_bytecode(include_str!(
        "../../../tests/fixtures/execution/owned-iterator-invalidation.kgr"
    ));
    let artifact = round_trip_bytecode(program);
    let seen = Arc::new(Mutex::new(0));
    let observed = seen.clone();
    let mut runtime = standard_runtime(Default::default());
    print_module(move |_, _, _| {
        *observed.lock().unwrap() += 1;
        Ok(())
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
