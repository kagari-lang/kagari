#[cfg(test)]
use crate::tests::support::bytecode::load_bytecode;
use crate::tests::{
    execution::{
        common::{compile_test_bytecode, round_trip_bytecode, standard_runtime},
        native_fixtures,
    },
    support::bytecode::reload_bytecode,
};

use kagari_runtime::{
    Runtime, RuntimeConfig,
    debug::{DebugSession, SourceBreakpoint},
    execution::{JitExecutionStatus, memory::GcHeapConfig, native::PreparedNativeEntry},
    module::entry::ProgramEntry,
    native::{conversion::context::ConversionContext, function_handle::Function},
    resource::RuntimeLimits,
    value::carriers::RootedValue,
};

use kagari_bytecode::instruction::BytecodeInstruction;
use kagari_common::identity::metadata::DefinitionMetadata;
use kagari_contract::ids::FunctionRef;
use kagari_types::{scalar::BuiltinType, ty::Ty};

fn runtime() -> Runtime {
    standard_runtime(RuntimeConfig {
        gc: GcHeapConfig {
            collection_threshold: Some(1),
            ..Default::default()
        },
        limits: RuntimeLimits {
            ..Default::default()
        },
        ..Default::default()
    })
}

#[test]
fn frame_roots_preserve_returned_objects_across_calls_and_collection_safepoints() {
    let module = compile_test_bytecode(include_str!(
        "../../../tests/fixtures/gc/returned-array.kgr"
    ));
    for encoded in [false, true] {
        for jit in [false, true] {
            let mut runtime = runtime();
            let program = module.clone();
            let program = if encoded {
                round_trip_bytecode(program)
            } else {
                program
            };
            let loaded = load_bytecode(&mut runtime, "gc.kgr", program).unwrap();
            let _loaded_program = loaded;
            let loaded = runtime.select_program(&_loaded_program).unwrap();
            let mut vm = runtime;
            vm.collect_garbage().unwrap();
            let baseline = vm.gc_stats();
            let report = if jit {
                vm.execute_prepared_module(
                    &loaded,
                    ProgramEntry::Main,
                    &native_fixtures::unsupported(),
                    vm.execution_options(),
                )
                .unwrap()
            } else {
                vm.execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
                    .unwrap()
            };
            assert!(vm.gc_stats().completed_cycles > baseline.completed_cycles);
            let retained = report.return_value;
            vm.collect_garbage().unwrap();
            assert_eq!(
                vm.gc_stats().root_registrations,
                baseline.root_registrations + 1
            );
            {
                let mut conversion = ConversionContext::new(&mut vm, &loaded).unwrap();
                let array = conversion.import(&retained).unwrap();
                assert_eq!(conversion.array_len(&array).unwrap(), 1);
                assert_eq!(
                    conversion
                        .array_element(&array, 0)
                        .unwrap()
                        .unwrap()
                        .as_i32(),
                    Some(42)
                );
            }
            drop(retained);
            vm.collect_garbage().unwrap();
            assert_eq!(vm.gc_stats().objects, baseline.objects);
            assert_eq!(vm.gc_stats().provider_units, baseline.provider_units);
            assert_eq!(
                vm.gc_stats().root_registrations,
                baseline.root_registrations
            );
        }
    }
}

#[test]
fn closures_keep_captured_objects_and_cells_alive_across_collection() {
    let module = compile_test_bytecode(include_str!(
        "../../../tests/fixtures/gc/captured-object-and-cell.kgr"
    ));
    let mut runtime = runtime();
    let installed = load_bytecode(&mut runtime, "closure_gc.kgr", module).unwrap();
    let loaded = runtime.select_program(&installed).unwrap();
    runtime.collect_garbage().unwrap();
    let baseline = runtime.gc_stats();
    let report = runtime
        .execute_module(&loaded, ProgramEntry::Main, runtime.execution_options())
        .unwrap();
    assert!(runtime.gc_stats().completed_cycles > baseline.completed_cycles);
    let mut conversion = ConversionContext::new(&mut runtime, &loaded).unwrap();
    assert_eq!(conversion.decode::<i32>(&report.return_value).unwrap(), 43);
    drop((conversion, report));
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc_stats().objects, baseline.objects);
    assert_eq!(runtime.gc_stats().provider_units, baseline.provider_units);
    assert_eq!(
        runtime.gc_stats().root_registrations,
        baseline.root_registrations
    );
    assert!(!runtime.is_quarantined());
}

#[test]
fn closure_handles_reject_other_runtimes_and_release_their_retained_graph() {
    let module = compile_test_bytecode(include_str!(
        "../../../tests/fixtures/gc/closure-handle.kgr"
    ));
    let mut foreign_runtime = runtime();
    let mut vm = runtime();
    let installed = load_bytecode(&mut vm, "closure_handles.kgr", module).unwrap();
    vm.collect_garbage().unwrap();
    let baseline = vm.gc_stats();
    let closure: Function<(), i32> = vm
        .execute_typed(&installed, "make", (), &Default::default())
        .unwrap();
    assert!(foreign_runtime.call(&closure, ()).is_err());
    vm.collect_garbage().unwrap();
    assert_eq!(vm.call(&closure, ()).unwrap(), 42);
    drop(closure);
    vm.collect_garbage().unwrap();
    assert_eq!(vm.gc_stats().objects, baseline.objects);
    assert_eq!(
        vm.gc_stats().root_registrations,
        baseline.root_registrations
    );
}

#[test]
fn malformed_closure_function_is_rejected_before_execution() {
    let (definitions, mut module) = compile_test_bytecode(include_str!(
        "../../../tests/fixtures/gc/malformed-closure.kgr"
    ))
    .into_parts();
    let instruction = module.modules[module.root.index()]
        .functions
        .iter_mut()
        .flat_map(|function| &mut function.instructions)
        .find(|instruction| matches!(instruction, BytecodeInstruction::MakeClosure { .. }))
        .expect("compiled closure");
    if let BytecodeInstruction::MakeClosure { function, .. } = instruction {
        *function = FunctionRef::new(999);
    }
    let mut runtime = runtime();
    assert!(
        load_bytecode(
            &mut runtime,
            "malformed_closure.kgr",
            DefinitionMetadata::checked(definitions, module, &Default::default()).unwrap()
        )
        .is_err()
    );
}

#[test]
fn rooted_closure_retains_its_old_program_after_new_publish() {
    let old = compile_test_bytecode(include_str!("../../../tests/fixtures/gc/closure-old.kgr"));
    let new = compile_test_bytecode(include_str!("../../../tests/fixtures/gc/closure-new.kgr"));
    let mut runtime = runtime();
    let loaded = load_bytecode(&mut runtime, "closure_epoch.kgr", old).unwrap();
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();
    let mut vm = runtime;
    let closure: Function<(), i32> = vm
        .execute_typed(&_loaded_program, "make", (), &Default::default())
        .unwrap();
    reload_bytecode(&mut vm, &_loaded_program, None, new).unwrap();
    let replacement = vm.select_program(&_loaded_program).unwrap();
    drop(loaded);
    vm.collect_garbage().unwrap();
    let prepared = closure.prepare_call(&mut vm).unwrap();
    vm.validate_executable_module(prepared.owner()).unwrap();
    assert_ne!(prepared.owner().key(), replacement.key());
    assert_eq!(vm.call(&closure, ()).unwrap(), 41);
    let fresh: Function<(), i32> = vm
        .execute_typed(&_loaded_program, "make", (), &Default::default())
        .unwrap();
    assert_eq!(vm.call(&fresh, ()).unwrap(), 42);
}

#[test]
fn native_scalar_execution_visits_the_same_collection_safepoint() {
    let mut runtime = runtime();
    let module =
        compile_test_bytecode(include_str!("../../../tests/fixtures/gc/native-scalar.kgr"));
    let loaded = load_bytecode(&mut runtime, "gc.kgr", module).unwrap();
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();
    runtime.collect_garbage().unwrap();
    let baseline = runtime.gc_stats();
    let dead = runtime
        .alloc_array(
            &loaded,
            Ty::Builtin(BuiltinType::I32),
            &[RootedValue::from(7i32)],
        )
        .unwrap();
    let native = PreparedNativeEntry::Native(native_fixtures::install_i32::<42>(
        &mut runtime,
        &loaded,
        false,
    ));
    drop(dead);
    let mut vm = runtime;
    let report = vm
        .execute_prepared_module(&loaded, ProgramEntry::Main, &native, vm.execution_options())
        .unwrap();
    assert_eq!(report.jit.unwrap().status, JitExecutionStatus::Native);
    assert_eq!(report.return_value.as_i32(), Some(42));
    assert!(vm.gc_stats().completed_cycles > baseline.completed_cycles);
    assert_eq!(vm.gc_stats().objects, baseline.objects);
}

#[test]
fn traps_release_frame_roots_and_call_depth() {
    let module = compile_test_bytecode(include_str!("../../../tests/fixtures/gc/trap-array.kgr"));
    {
        let mut runtime = runtime();
        let loaded = load_bytecode(&mut runtime, "gc.kgr", module.clone()).unwrap();
        let _loaded_program = loaded;
        let loaded = runtime.select_program(&_loaded_program).unwrap();
        let mut vm = runtime;
        vm.collect_garbage().unwrap();
        let baseline = vm.gc_stats();
        assert!(
            vm.execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
                .is_err()
        );
        assert_eq!(vm.resources().counters().current_call_depth, 0);
        vm.collect_garbage().unwrap();
        assert_eq!(vm.gc_stats().objects, baseline.objects);
        assert_eq!(vm.gc_stats().provider_units, baseline.provider_units);
        assert_eq!(
            vm.gc_stats().root_registrations,
            baseline.root_registrations
        );
        assert_eq!(
            vm.resources().counters().current_heap_units,
            baseline.objects as u128 + baseline.provider_units
        );
    }
}

#[test]
fn cloned_debug_bindings_keep_inspected_objects_alive_after_the_session_is_replaced() {
    let source = include_str!("../../../tests/fixtures/gc/debug-binding.kgr");
    let module = compile_test_bytecode(source);
    let mut runtime = standard_runtime(RuntimeConfig {
        ..Default::default()
    });
    let loaded = load_bytecode(&mut runtime, "gc.kgr", module).unwrap();
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();
    let mut session = DebugSession::new(&runtime).unwrap();
    session
        .add_breakpoint(SourceBreakpoint::at_source_offset(
            "gc.kgr",
            source.find("kept.len").unwrap(),
        ))
        .unwrap();
    let mut vm = runtime;
    vm.collect_garbage().unwrap();
    let baseline = vm.gc_stats();
    vm.attach_debug_session(session).unwrap();
    vm.execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
        .unwrap();
    let binding = vm
        .debug_session()
        .unwrap()
        .pauses()
        .iter()
        .flat_map(|pause| &pause.frames)
        .flat_map(|frame| &frame.bindings)
        .find(|binding| binding.name == "kept")
        .unwrap()
        .clone();
    vm.attach_debug_session(DebugSession::new(&vm).unwrap())
        .unwrap();
    vm.collect_garbage().unwrap();
    {
        let mut conversion = ConversionContext::new(&mut vm, &loaded).unwrap();
        let array = conversion.import(&binding.value).unwrap();
        assert_eq!(conversion.array_len(&array).unwrap(), 1);
        assert_eq!(
            conversion
                .array_element(&array, 0)
                .unwrap()
                .unwrap()
                .as_i32(),
            Some(7)
        );
    }
    drop(binding);
    vm.collect_garbage().unwrap();
    assert_eq!(vm.gc_stats().objects, baseline.objects);
    assert_eq!(
        vm.gc_stats().root_registrations,
        baseline.root_registrations
    );
}

#[test]
fn growing_execution_windows_keep_outer_values_alive_and_release_them_on_return() {
    let program = compile_test_bytecode(include_str!(
        "../../../tests/fixtures/gc/recursive-window-roots.kgr"
    ));
    let mut runtime = runtime();
    let loaded = load_bytecode(&mut runtime, "growing_windows", program).unwrap();
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();
    let mut vm = runtime;
    vm.collect_garbage().unwrap();
    let baseline = vm.gc_stats();
    let report = vm
        .execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
        .unwrap();
    assert_eq!(report.return_value.as_i32(), Some(74));
    assert!(vm.gc_stats().completed_cycles > baseline.completed_cycles);
    assert_eq!(vm.resources().counters().current_call_depth, 0);
    drop(report);
    vm.collect_garbage().unwrap();
    assert_eq!(vm.gc_stats().objects, baseline.objects);
    assert_eq!(vm.gc_stats().provider_units, baseline.provider_units);
    assert_eq!(
        vm.gc_stats().root_registrations,
        baseline.root_registrations
    );
}
