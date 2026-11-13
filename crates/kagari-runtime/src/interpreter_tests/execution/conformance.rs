use crate::{
    Runtime, RuntimeConfig,
    debug::{DebugPauseReason, DebugSession, DebugWatch, SourceBreakpoint},
    error::{RuntimeErrorKind, reason::ErrorReason},
    module::entry::ProgramEntry,
    native::{
        binding::NativeResult, builder::ModuleBuilder, catalog::DeclarationCatalog,
        conversion::context::ConversionContext, declarations::FunctionDecl, typed::NativeContext,
        types::Type,
    },
    resource::RuntimeLimits,
};
use std::num::NonZeroUsize;

#[cfg(test)]
use crate::test_support::bytecode::load_bytecode;

use crate::interpreter_tests::{
    common::{
        compile_native_test_bytecode, compile_test_bytecode, load_bytecode_program,
        load_test_module, round_trip_bytecode, test_runtime,
    },
    native_fixtures,
};

use kagari_common::identity::metadata::DefinitionMetadata;

fn debug_runtime(_module_name: &str) -> Runtime {
    test_runtime(RuntimeConfig {
        ..RuntimeConfig::default()
    })
}

#[test]
fn missing_entry_is_rejected_before_execution() {
    let bytecode = compile_test_bytecode("fn main() -> i32 { 42 }");
    for encoded in [false, true] {
        let program = bytecode.clone();
        let program = if encoded {
            round_trip_bytecode(program)
        } else {
            program
        };
        let mut runtime = test_runtime(Default::default());
        let loaded = load_bytecode(&mut runtime, "missing-entry.kgr", program).unwrap();
        let _loaded_program = loaded;
        let loaded = runtime.select_program(&_loaded_program).unwrap();
        let mut vm = runtime;
        vm.collect_garbage().unwrap();
        let baseline = vm.gc_stats();
        for jit in [false, true] {
            let error = if jit {
                let prepared = native_fixtures::unsupported();
                vm.execute_prepared_module(&loaded, "missing", &prepared, vm.execution_options())
                    .unwrap_err()
            } else {
                vm.execute_module(&loaded, "missing", vm.execution_options())
                    .unwrap_err()
            };
            assert!(
                matches!(error.reason(), ErrorReason::MissingFunction(name) if name == "missing"),
                "{encoded}/{jit}: {error:?}"
            );

            assert_eq!(
                vm.gc_stats().root_registrations,
                baseline.root_registrations
            );
        }
    }
}

#[test]
fn public_entries_use_verified_identity_despite_duplicate_display_names() {
    let (definitions, mut bytecode) =
        compile_test_bytecode("pub fn first() -> i32 { 1 } pub fn second() -> i32 { 2 }")
            .into_parts();
    let second = bytecode.modules[bytecode.root.index()]
        .functions
        .iter()
        .position(|function| function.name == "second")
        .unwrap();
    bytecode.modules[bytecode.root.index()].functions[second].name = "first".into();
    bytecode.modules[bytecode.root.index()].function_table[second].name = "first".into();
    let bytecode = DefinitionMetadata::checked(definitions, bytecode, &Default::default()).unwrap();
    for encoded in [false, true] {
        let program = bytecode.clone();
        let program = if encoded {
            round_trip_bytecode(program)
        } else {
            program
        };
        let mut runtime = test_runtime(Default::default());
        let loaded = load_bytecode(&mut runtime, "ambiguous-entry.kgr", program).unwrap();
        let _loaded_program = loaded;
        let loaded = runtime.select_program(&_loaded_program).unwrap();
        let mut vm = runtime;
        vm.collect_garbage().unwrap();
        let baseline = vm.gc_stats();
        for jit in [false, true] {
            for (path, expected) in [("first", 1), ("second", 2)] {
                let report = if jit {
                    let prepared = native_fixtures::unsupported();
                    vm.execute_prepared_module(&loaded, path, &prepared, vm.execution_options())
                        .unwrap()
                } else {
                    vm.execute_module(&loaded, path, vm.execution_options())
                        .unwrap()
                };
                assert_eq!(
                    report.return_value.as_i32(),
                    Some(expected),
                    "{encoded}/{jit}/{path}"
                );
                drop(report);
                assert_eq!(
                    vm.gc_stats().root_registrations,
                    baseline.root_registrations
                );
            }
        }
    }
}

#[test]
fn interpreter_conformance_classifies_failure_paths() {
    let (runtime, loaded) = load_test_module("fn main() -> i32 { val values = [1]; values[3] }");
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();
    let mut vm = runtime;
    let error = vm
        .execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
        .expect_err("out of bounds index should trap");
    assert!(matches!(&error, error if error.kind() == RuntimeErrorKind::IndexOutOfBounds));

    let bytecode = compile_test_bytecode("fn main() -> i32 { 1 + 2 }");
    let mut runtime = test_runtime(RuntimeConfig {
        limits: RuntimeLimits {
            max_stack_bytes: NonZeroUsize::new(64).unwrap(),
            ..Default::default()
        },
        ..RuntimeConfig::default()
    });
    let loaded =
        load_bytecode(&mut runtime, "resource_limit.kgr", bytecode).expect("module should load");
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();
    let mut vm = runtime;
    let error = vm
        .execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
        .expect_err("resource limit should be classified");
    assert!(matches!(
        error,
        ref error
            if error.kind() == RuntimeErrorKind::StackOverflow
    ));
}

#[test]
fn interpreter_debug_conformance_covers_stack_values_and_watch_expressions() {
    let source = include_str!("../../../tests/fixtures/debug/conformance-watch-values.kgr");
    let mut runtime = debug_runtime("debug_conformance.kgr");
    let loaded = load_bytecode(
        &mut runtime,
        "debug_conformance.kgr",
        compile_test_bytecode(source),
    )
    .expect("module should load");
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();
    let mut session = DebugSession::new(&runtime).expect("debug session should be allowed");
    let breakpoint = session
        .add_breakpoint(SourceBreakpoint::at_source_offset(
            "debug_conformance.kgr",
            source
                .find("point.x")
                .expect("source should contain tail expr"),
        ))
        .expect("breakpoint should be allowed");

    let mut vm = runtime;
    vm.attach_debug_session(session)
        .expect("debug attach should be allowed");
    let report = vm
        .execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
        .expect("vm should execute");
    assert_eq!(report.return_value.as_i32(), Some(12));

    let debug = vm
        .debug_session()
        .expect("debug session should be attached");
    let pause = debug
        .pauses()
        .iter()
        .find(|pause| pause.reason == DebugPauseReason::Breakpoint(breakpoint))
        .expect("breakpoint should pause")
        .clone();
    drop(debug);
    vm.collect_garbage().unwrap();
    assert_eq!(pause.frames.len(), 2);
    let caller = &pause.frames[0];
    let callee = pause.top_frame().expect("pause should expose top frame");
    assert_eq!(caller.function_name, "main");
    assert_eq!(callee.function_name, "callee");
    assert_eq!(
        pause
            .evaluate_watch(
                &mut vm,
                callee.id,
                &DebugWatch::Binding("doubled".to_owned()),
            )
            .expect("watch should read live local")
            .as_i32(),
        Some(8)
    );
    assert!(matches!(
        pause
            .evaluate_watch(
                &mut vm,
                callee.id,
                &DebugWatch::Binding("missing".to_owned())
            )
            .expect_err("missing watch binding should be classified").reason(),
        ErrorReason::MissingField(name) if name == "missing"
    ));

    let numbers = callee
        .bindings
        .iter()
        .find(|binding| binding.name == "numbers")
        .expect("numbers binding should be inspectable")
        .value
        .clone();
    let point = callee
        .bindings
        .iter()
        .find(|binding| binding.name == "point")
        .expect("point binding should be inspectable")
        .value
        .clone();
    let mut cx = ConversionContext::new(&mut vm, &loaded).unwrap();
    let numbers = cx.import(&numbers).unwrap();
    assert_eq!(cx.array_len(&numbers).unwrap(), 2);
    assert_eq!(
        cx.array_element(&numbers, 0usize)
            .unwrap()
            .unwrap()
            .as_i32(),
        Some(4)
    );
    assert_eq!(
        cx.array_element(&numbers, 1usize)
            .unwrap()
            .unwrap()
            .as_i32(),
        Some(8)
    );
    let point = cx.import(&point).unwrap();
    let layout = cx.struct_layout(&point).unwrap();
    assert_eq!(
        layout.layout().name(layout.module().definitions()),
        Some("Point")
    );
    assert_eq!(
        layout
            .layout()
            .fields
            .iter()
            .map(|field| field.name.as_str())
            .collect::<Vec<_>>(),
        ["x", "y"]
    );
    assert_eq!(
        cx.struct_field(&point, &layout, 0).unwrap().as_i32(),
        Some(4)
    );
    assert_eq!(
        cx.struct_field(&point, &layout, 1).unwrap().as_i32(),
        Some(8)
    );
}

#[test]
fn interpreter_debug_conformance_covers_stepping_and_run_to_cursor() {
    let source = include_str!("../../../tests/fixtures/debug/conformance-stepping.kgr");
    let mut runtime = debug_runtime("debug_steps.kgr");
    let loaded = load_bytecode(
        &mut runtime,
        "debug_steps.kgr",
        compile_test_bytecode(source),
    )
    .expect("module should load");
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();
    let mut session = DebugSession::new(&runtime).expect("debug session should be allowed");
    let cursor = session
        .run_to_cursor(
            "debug_steps.kgr",
            source
                .find("helper(seed)")
                .expect("source should contain cursor target"),
        )
        .expect("run to cursor should be allowed");
    session.step_into().expect("step should be allowed");

    let mut vm = runtime;
    vm.attach_debug_session(session)
        .expect("debug attach should be allowed");
    let report = vm
        .execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
        .expect("vm should execute");

    assert_eq!(report.return_value.as_i32(), Some(3));
    let debug = vm
        .debug_session()
        .expect("debug session should be attached");
    assert!(
        debug
            .pauses()
            .iter()
            .any(|pause| pause.reason == DebugPauseReason::Step)
    );
    assert!(
        debug
            .pauses()
            .iter()
            .any(|pause| pause.reason == DebugPauseReason::Breakpoint(cursor))
    );
    assert!(
        !debug
            .resolved_breakpoints()
            .iter()
            .any(|breakpoint| breakpoint.breakpoint_id == cursor),
        "temporary run-to-cursor breakpoint should clear after it is hit"
    );
}

#[test]
fn empty_provider_and_custom_native_calls_need_no_standard_installation() {
    let (mut vm, owner) = load_bytecode_program(
        "empty-provider",
        round_trip_bytecode(compile_test_bytecode("pub fn main() {}")),
    );
    let main = vm.bind_function::<(), ()>(&owner.root(), "main").unwrap();
    vm.call(&main, ()).unwrap();

    let mut module = ModuleBuilder::new("example::core", &DeclarationCatalog::default()).unwrap();
    let echo = module
        .define_function(
            FunctionDecl::new("echo")
                .parameter("value", Type::i32())
                .returns(Type::i32()),
        )
        .unwrap();
    module
        .bind(
            echo,
            |_: &mut NativeContext<'_>, (value,): (i32,)| -> NativeResult<i32> { Ok(value) },
        )
        .unwrap();
    let module = module.finish().unwrap();
    let mut vm = Runtime::default();
    module.install(&mut vm).unwrap();
    let owner = load_bytecode(
        &mut vm,
        "core-entry",
        round_trip_bytecode(compile_native_test_bytecode(
            include_str!("../../../tests/fixtures/core_only_calls.kgr"),
            &[&module],
        )),
    )
    .unwrap();
    vm.collect_garbage().unwrap();
    let baseline = vm.gc_stats();
    let main = vm.bind_function::<(), i32>(&owner.root(), "main").unwrap();
    assert_eq!(vm.call(&main, ()).unwrap(), 42);
    assert_eq!(vm.resources().counters().current_call_depth, 0);
    vm.collect_garbage().unwrap();
    assert_eq!(vm.gc_stats().objects, baseline.objects);
    assert_eq!(vm.gc_stats().provider_units, baseline.provider_units);
    assert_eq!(
        vm.gc_stats().root_registrations,
        baseline.root_registrations
    );
}
