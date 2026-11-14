#[cfg(test)]
use crate::tests::support::bytecode::load_bytecode;
use crate::tests::{
    execution::{
        common::{
            compile_logging_test_bytecode, compile_test_bytecode, logging_module,
            round_trip_bytecode, standard_runtime,
        },
        native_boundary::publish_unused_version,
        native_fixtures,
    },
    support::bytecode::reload_bytecode,
};

use kagari_runtime::{
    Runtime, RuntimeConfig,
    backend::{BackendInvocationError, native::NativeInvocationFailure},
    error::RuntimeErrorKind,
    execution::reentry::reenter,
    host::{HostBorrowKind, HostError, HostObjectId},
    metadata::TypeId,
    module::{
        ExecutableModule, ModuleEpochRetention, entry::ProgramEntry, retention::RetentionOrigin,
    },
    native::conversion::context::ConversionContext,
    resource::RuntimeLimits,
    value::{carriers::RootedValue, view::ValueView},
};

use kagari_bytecode::program::BytecodeProgram;
use kagari_common::{cancellation::CancellationToken, identity::metadata::DefinitionMetadata};
use kagari_types::{language::binding, scalar::BuiltinType, ty::Ty};
use std::num::NonZeroUsize;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

fn runtime() -> Runtime {
    standard_runtime(RuntimeConfig {
        limits: RuntimeLimits {
            ..Default::default()
        },
        ..Default::default()
    })
}

fn route(
    program: DefinitionMetadata<BytecodeProgram>,
    encoded: bool,
) -> DefinitionMetadata<BytecodeProgram> {
    if !encoded {
        return program;
    }
    round_trip_bytecode(program)
}

fn array_values(runtime: &mut Runtime, owner: &ExecutableModule, value: &RootedValue) -> Vec<i32> {
    let mut cx = ConversionContext::new(runtime, owner).unwrap();
    let value = cx.import(value).unwrap();
    (0..cx.array_len(&value).unwrap())
        .map(|index| {
            cx.array_element(&value, index)
                .unwrap()
                .unwrap()
                .as_i32()
                .unwrap()
        })
        .collect()
}

#[test]
fn host_reentry_keeps_outer_frames_results_and_borrow_scopes_alive() {
    for encoded in [false, true] {
        for jit in [false, true] {
            let module = compile_logging_test_bytecode(include_str!(
                "../../../tests/fixtures/execution/session-reentry-storage.kgr"
            ));
            let make = module.records().modules[module.records().root.index()]
                .functions
                .iter()
                .find(|f| f.name == "make")
                .unwrap()
                .id;
            let retained = Arc::new(Mutex::new(None));
            let result = retained.clone();
            let mut runtime = runtime();
            logging_module(move |context, args| {
                if context
                    .read_string(&args[0], |text| text == "outer")
                    .unwrap()
                {
                    let root = context.runtime().execution_root().unwrap();
                    let token = context
                        .borrows()
                        .borrow_unique(HostObjectId(1), TypeId::new(0))
                        .unwrap();
                    let wrong = context
                        .retain_temporaries(&[ValueView::from(true)])
                        .unwrap();
                    assert_eq!(
                        reenter(context, &root, make, wrong).unwrap_err().kind(),
                        RuntimeErrorKind::ModuleValidation
                    );
                    let arguments = context
                        .retain_temporaries(&[ValueView::from(7i32)])
                        .unwrap();
                    let value = reenter(context, &root, make, arguments).unwrap();
                    context
                        .borrows()
                        .validate(token, HostBorrowKind::Unique)
                        .unwrap();
                    context.runtime_mut().collect_garbage().unwrap();
                    assert_eq!(array_values(context.runtime_mut(), &root, &value), [7, 8]);
                    *result.lock().unwrap() = Some(value);
                } else {
                    assert_eq!(
                        context.runtime().resources().counters().current_call_depth,
                        4
                    );
                    assert_eq!(
                        context
                            .borrows()
                            .borrow_shared(HostObjectId(1), TypeId::new(0))
                            .unwrap_err()
                            .kind(),
                        RuntimeErrorKind::HostBorrowConflict
                    );
                    context.runtime_mut().collect_garbage().unwrap();
                }
                Ok(ValueView::unit())
            })
            .install(&mut runtime)
            .unwrap();
            let loaded =
                load_bytecode(&mut runtime, "reentry.kgr", route(module, encoded)).unwrap();
            let _loaded_program = loaded;
            let loaded = runtime.select_program(&_loaded_program).unwrap();
            let mut vm = runtime;
            vm.collect_garbage().unwrap();
            let baseline = vm.gc_stats();
            let prepared = native_fixtures::unsupported();
            let report = if jit {
                vm.execute_prepared_module(
                    &loaded,
                    ProgramEntry::Main,
                    &prepared,
                    vm.execution_options(),
                )
            } else {
                vm.execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
            }
            .unwrap();
            assert_eq!(report.return_value.as_i32(), Some(42));
            drop(report);
            assert_eq!(vm.resources().counters().current_call_depth, 0);
            assert!(vm.execution_root().is_none());
            assert_eq!(vm.modules().retention_counts(loaded.key()).active_calls, 0);
            let mut resources = vm.host_scope(&[]).unwrap();
            let frame = resources.borrows();
            frame
                .borrow_unique(HostObjectId(1), TypeId::new(0))
                .unwrap();
            drop(frame);
            resources.runtime_mut().collect_garbage().unwrap();
            let value = retained.lock().unwrap().as_ref().unwrap().clone();
            assert_eq!(
                array_values(resources.runtime_mut(), &loaded, &value),
                [7, 8]
            );
            let mut other = Runtime::default();
            assert_eq!(
                other
                    .host_scope(std::slice::from_ref(&value))
                    .err()
                    .unwrap()
                    .kind(),
                RuntimeErrorKind::ModuleValidation
            );
            drop(value);
            retained.lock().unwrap().take();
            resources.runtime_mut().collect_garbage().unwrap();
            assert_eq!(
                resources.runtime().gc_stats().root_registrations,
                baseline.root_registrations
            );
            assert_eq!(resources.runtime().gc_stats().objects, baseline.objects);
        }
    }
}

#[test]
fn host_reentry_cannot_swallow_root_termination_and_releases_borrows() {
    for cancel in [false, true] {
        for encoded in [false, true] {
            for jit in [false, true] {
                let module = compile_logging_test_bytecode(include_str!(
                    "../../../tests/fixtures/execution/session-reentry-termination.kgr"
                ));
                let nested = module.records().modules[module.records().root.index()]
                    .functions
                    .iter()
                    .find(|f| f.name == "nested")
                    .unwrap()
                    .id;
                let token = CancellationToken::default();
                let cancellation = token.clone();
                let mut runtime = standard_runtime(RuntimeConfig {
                    limits: RuntimeLimits {
                        max_native_depth: NonZeroUsize::new(if cancel { 32 } else { 1 }).unwrap(),
                        ..Default::default()
                    },
                    ..Default::default()
                });
                logging_module(move |context, args| {
                    let root = context.runtime().execution_root().unwrap();
                    context.borrows().borrow_shared(HostObjectId(2), TypeId::new(0)).unwrap();
                    let temporary = context.convert_value(&root, |cx| {
                        let item = cx.type_for::<i32>()?;
                        cx.allocate_array(&item, &[ValueView::from(11i32)])
                    }).unwrap();
                    context.retain_temporaries(std::slice::from_ref(&temporary)).unwrap();
                    context.runtime_mut().collect_garbage().unwrap();
                    let observed = context.retain(&temporary).unwrap();
                    assert_eq!(array_values(context.runtime_mut(), &root, &observed), [11]);
                    drop(observed);
                    if context.read_string(&args[0], |text| text == "inner").unwrap() {
                        cancellation.cancel();
                    } else {
                        let arguments = context.retain_temporaries(&[]).unwrap();
                        let error = reenter(context, &root, nested, arguments).expect_err("nested termination");
                        assert!(matches!(&error, error if error.kind() == if cancel { RuntimeErrorKind::Cancelled } else { RuntimeErrorKind::StackOverflow }));
                    }
                    // Even a host deliberately swallowing a nested terminal error cannot resume.
                    Ok(ValueView::unit())
                }).install(&mut runtime).unwrap();
                let loaded =
                    load_bytecode(&mut runtime, "termination.kgr", route(module, encoded)).unwrap();
                let _loaded_program = loaded;
                let loaded = runtime.select_program(&_loaded_program).unwrap();
                runtime.collect_garbage().unwrap();
                let baseline = runtime.gc_stats();
                let mut options = runtime.execution_options();
                options.cancellation = token;

                let mut vm = runtime;
                let mut scope = vm.begin_execution(&loaded, options).unwrap();
                let scope_options = scope.runtime().execution_options();
                let scope_options = scope_options.clone();

                let prepared = native_fixtures::unsupported();
                let error = if jit {
                    scope.runtime_mut().execute_prepared_module(
                        &loaded,
                        ProgramEntry::Main,
                        &prepared,
                        scope_options.clone(),
                    )
                } else {
                    scope.runtime_mut().execute_module(
                        &loaded,
                        ProgramEntry::Main,
                        scope_options.clone(),
                    )
                }
                .unwrap_err();
                let names = error
                    .trace()
                    .unwrap()
                    .frames
                    .iter()
                    .map(|frame| frame.function_name.as_str())
                    .collect::<Vec<_>>();
                assert_eq!(names, ["log", "nested", "log", "main"]);
                assert!(
                    matches!(&error, error if error.kind() == if cancel { RuntimeErrorKind::Cancelled } else { RuntimeErrorKind::StackOverflow })
                );
                assert_eq!(scope.runtime().resources().counters().current_call_depth, 0);
                assert_eq!(scope.host_scope_count(), 0);
                drop(scope);
                // Both log literals belong to the selected revision cache.
                // Retire it before checking exact execution-temporary release.
                publish_unused_version(&mut vm, &_loaded_program);
                vm.collect_garbage().unwrap();
                assert!(vm.validate_executable_module(&loaded).is_err());
                assert_eq!(
                    vm.gc_stats().root_registrations,
                    baseline.root_registrations
                );
                assert_eq!(vm.gc_stats().objects, baseline.objects);
                let resources = vm.host_scope(&[]).unwrap();
                let frame = resources.borrows();
                frame
                    .borrow_unique(HostObjectId(2), TypeId::new(0))
                    .unwrap();
                assert!(resources.runtime().execution_root().is_none());
            }
        }
    }
}

#[test]
fn reentry_rejects_foreign_sources_and_retired_code() {
    let module = compile_logging_test_bytecode(include_str!(
        "../../../tests/fixtures/execution/reentry-inputs.kgr"
    ));
    let echo = module.records().modules[module.records().root.index()]
        .functions
        .iter()
        .find(|f| f.name == "echo")
        .unwrap()
        .id;
    let calls = Arc::new(AtomicUsize::new(0));
    let called = calls.clone();
    let mut foreign = standard_runtime(Default::default());
    let foreign_program = load_bytecode(
        &mut foreign,
        "foreign.kgr",
        compile_test_bytecode("fn main() -> i32 { 7 }"),
    )
    .unwrap();
    let foreign_module = foreign.select_program(&foreign_program).unwrap();
    let foreign_value = foreign
        .alloc_array(&foreign_module, Ty::Builtin(BuiltinType::I32), &[])
        .unwrap();
    let retained_foreign_value = foreign_value.clone();
    let retained_foreign_module = foreign_module.clone();
    let retired = Arc::new(Mutex::new(None::<ExecutableModule>));
    let retired_callback = retired.clone();
    let mut runtime = runtime();
    logging_module(move |context, _| {
        called.fetch_add(1, Ordering::SeqCst);
        let empty = context.retain_temporaries(&[]).unwrap();
        let main = foreign_module
            .bytecode
            .functions
            .iter()
            .find(|f| f.name == "main")
            .unwrap()
            .id;
        assert_eq!(
            reenter(context, &foreign_module, main, empty)
                .unwrap_err()
                .kind(),
            RuntimeErrorKind::StaleHandle
        );
        assert_eq!(
            context.import(&foreign_value).unwrap_err().kind(),
            RuntimeErrorKind::ModuleValidation
        );
        let old = retired_callback.lock().unwrap().as_ref().unwrap().clone();
        let main = old
            .bytecode
            .functions
            .iter()
            .find(|f| f.name == "main")
            .unwrap()
            .id;
        assert_eq!(
            reenter(context, &old, main, empty).unwrap_err().kind(),
            RuntimeErrorKind::StaleHandle
        );
        let root = context.runtime().execution_root().unwrap();
        let local = context
            .runtime_mut()
            .alloc_array(
                &root,
                Ty::Builtin(BuiltinType::I32),
                &[RootedValue::from(11i32)],
            )
            .unwrap();
        context.runtime_mut().collect_garbage().unwrap();
        let view = context.import(&local).unwrap();
        let arguments = context.retain_temporaries(&[view]).unwrap();
        let result = reenter(context, &root, echo, arguments).unwrap();
        assert_eq!(array_values(context.runtime_mut(), &root, &result), [11]);
        Ok(ValueView::unit())
    })
    .install(&mut runtime)
    .unwrap();
    let retired_program = load_bytecode(
        &mut runtime,
        "retired",
        compile_test_bytecode("fn main() -> i32 { 7 }"),
    )
    .unwrap();
    let old = runtime.select_program(&retired_program).unwrap();
    let candidate = runtime
        .stage_reload_verified_program(&retired_program, None, old.verified_program().clone())
        .unwrap();
    runtime.publish_staged_reload(candidate).unwrap();
    runtime.collect_garbage().unwrap();
    assert_eq!(
        runtime.validate_executable_module(&old).unwrap_err().kind(),
        RuntimeErrorKind::StaleHandle
    );
    *retired.lock().unwrap() = Some(old);
    let loaded_program = load_bytecode(&mut runtime, "inputs.kgr", module).unwrap();
    let loaded = runtime.select_program(&loaded_program).unwrap();
    runtime.collect_garbage().unwrap();
    let baseline = runtime.gc_stats();
    assert_eq!(
        runtime
            .execute_module(&loaded, ProgramEntry::Main, runtime.execution_options())
            .unwrap()
            .return_value
            .as_i32(),
        Some(42)
    );
    assert!(
        array_values(
            &mut foreign,
            &retained_foreign_module,
            &retained_foreign_value
        )
        .is_empty()
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    runtime.collect_garbage().unwrap();
    assert_eq!(
        runtime.gc_stats().root_registrations,
        baseline.root_registrations
    );
    assert_eq!(runtime.gc_stats().objects, baseline.objects);
    assert_eq!(runtime.resources().counters().current_call_depth, 0);
}

#[test]
fn reentry_selects_current_code_and_publication_requires_an_idle_boundary() {
    for encoded in [false, true] {
        let program = |number| {
            route(
                compile_logging_test_bytecode(&format!(
                    "fn main() -> i32 {{ print(\"enter\"); 0 }} fn value() -> i32 {{ {number} }}"
                )),
                encoded,
            )
        };
        let first = program(7);
        let function = first.records().modules[first.records().root.index()]
            .functions
            .iter()
            .find(|f| f.name == "value")
            .unwrap()
            .id;
        let versions = Arc::new(Mutex::new(Vec::<ExecutableModule>::new()));
        let observed = Arc::new(Mutex::new(Vec::new()));
        let targets = versions.clone();
        let values = observed.clone();
        let mut runtime = runtime();
        logging_module(move |context, _| {
            let versions = targets.lock().unwrap().clone();
            for version in &versions {
                let arguments = context.retain_temporaries(&[]).unwrap();
                let result = reenter(context, version, function, arguments);
                values
                    .lock()
                    .unwrap()
                    .push(result.unwrap().as_i32().unwrap());
            }
            Ok(ValueView::unit())
        })
        .install(&mut runtime)
        .unwrap();
        let old = load_bytecode(&mut runtime, "versions.kgr", first).unwrap();
        let _old_program = old;
        let old = runtime.select_program(&_old_program).unwrap();
        let mut vm = runtime;
        let retained_old = vm
            .retain_module(
                &old,
                ModuleEpochRetention::CompiledArtifact,
                RetentionOrigin::Current,
            )
            .unwrap();
        let mut scope = vm.begin_execution(&old, vm.execution_options()).unwrap();
        let scope_options = scope.runtime().execution_options();
        let scope_options = scope_options.clone();
        // This live session excludes mutable reload through Rust borrowing.
        assert_eq!(
            scope.runtime().select_program(&_old_program).unwrap().key(),
            old.key()
        );
        versions.lock().unwrap().push(old.clone());
        scope
            .runtime_mut()
            .execute_module(&old, ProgramEntry::Main, scope_options.clone())
            .unwrap();
        drop(scope);
        reload_bytecode(
            &mut vm,
            &_old_program,
            Some("versions.kgr".into()),
            program(9),
        )
        .unwrap();
        vm.collect_garbage().unwrap();
        let new = vm.select_program(&_old_program).unwrap();
        versions.lock().unwrap().push(new.clone());
        vm.execute_module(&new, ProgramEntry::Main, vm.execution_options())
            .unwrap();
        assert_eq!(*observed.lock().unwrap(), [7, 9, 9]);
        drop(retained_old);
        vm.collect_garbage().unwrap();
        assert!(vm.validate_executable_module(&old).is_err());
    }
}

#[test]
fn reentry_trap_cleans_nested_frames_without_terminating_the_outer_call() {
    let module = compile_logging_test_bytecode(include_str!(
        "../../../tests/fixtures/execution/session-reentry-trap.kgr"
    ));
    let fail = module.records().modules[module.records().root.index()]
        .functions
        .iter()
        .find(|f| f.name == "fail")
        .unwrap()
        .id;
    let mut runtime = runtime();
    logging_module(move |context, args| {
        let root = context.runtime().execution_root().unwrap();
        let arguments = context
            .retain_temporaries(&[ValueView::from(i32::MAX)])
            .unwrap();
        context.runtime_mut().collect_garbage().unwrap();
        let objects = context.runtime().gc_stats().objects;
        let error = reenter(context, &root, fail, arguments).unwrap_err();
        assert!(matches!(&error, error if error.kind() == RuntimeErrorKind::ScriptTrap));
        assert_eq!(
            context.runtime().resources().counters().current_call_depth,
            2
        );
        context.runtime_mut().collect_garbage().unwrap();
        assert_eq!(context.runtime().gc_stats().objects, objects);
        assert_eq!(
            context.read_string(&args[0], str::to_owned).unwrap(),
            "enter"
        );
        Ok(ValueView::unit())
    })
    .install(&mut runtime)
    .unwrap();
    let loaded = load_bytecode(&mut runtime, "trap.kgr", module).unwrap();
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();
    let mut vm = runtime;
    vm.collect_garbage().unwrap();
    let baseline = vm.gc_stats();
    assert_eq!(
        vm.execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
            .unwrap()
            .return_value
            .as_i32(),
        Some(42)
    );
    vm.collect_garbage().unwrap();
    assert_eq!(
        vm.gc_stats().root_registrations,
        baseline.root_registrations
    );
    assert_eq!(vm.gc_stats().objects, baseline.objects);
    assert_eq!(vm.resources().counters().current_call_depth, 0);
}

#[test]
fn native_safepoint_reports_cancellation_at_the_current_instruction() {
    let mut runtime = runtime();
    let module = compile_test_bytecode("fn main() -> i32 { 42 }");
    let loaded = load_bytecode(&mut runtime, "native-cancel.kgr", module).unwrap();
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();
    let native = native_fixtures::install_i32::<42>(&mut runtime, &loaded, false);
    let token = CancellationToken::default();
    let mut options = runtime.execution_options();
    options.cancellation = token.clone();
    let mut session = runtime.begin_execution(&loaded, options).unwrap();
    token.cancel();
    assert!(
        matches!(session.runtime_mut().invoke_native_function(&native),
        Err(NativeInvocationFailure { error: BackendInvocationError::RuntimeFailure(error), .. }) if error.kind() == RuntimeErrorKind::Cancelled)
    );

    drop(session);
    assert_eq!(
        runtime.invoke_native_function(&native).unwrap().as_i32(),
        Some(42)
    );
}

#[test]
fn cancellation_after_a_host_effect_releases_frames_and_preserves_the_effect() {
    for encoded in [false, true] {
        for jit in [false, true] {
            let mut runtime = runtime();
            let token = CancellationToken::default();
            let cancel = token.clone();
            let calls = Arc::new(AtomicUsize::new(0));
            let recorded = calls.clone();
            logging_module(move |_, _| {
                recorded.fetch_add(1, Ordering::SeqCst);
                cancel.cancel();
                Ok(ValueView::unit())
            })
            .install(&mut runtime)
            .unwrap();
            let module = compile_logging_test_bytecode(include_str!(
                "../../../tests/fixtures/execution/session-effect-cancellation.kgr"
            ));
            let loaded = load_bytecode(&mut runtime, "cancel.kgr", route(module, encoded)).unwrap();
            let _loaded_program = loaded;
            let loaded = runtime.select_program(&_loaded_program).unwrap();
            runtime.collect_garbage().unwrap();
            let baseline = runtime.gc_stats();
            let mut options = runtime.execution_options();
            options.cancellation = token;
            let mut vm = runtime;
            let mut session = vm.begin_execution(&loaded, options).unwrap();
            let session_options = session.runtime().execution_options();
            let session_options = session_options.clone();
            let prepared = native_fixtures::unsupported();
            let error = if jit {
                session
                    .runtime_mut()
                    .execute_prepared_module(
                        &loaded,
                        ProgramEntry::Main,
                        &prepared,
                        session_options.clone(),
                    )
                    .unwrap_err()
            } else {
                session
                    .runtime_mut()
                    .execute_module(&loaded, ProgramEntry::Main, session_options.clone())
                    .unwrap_err()
            };
            assert!(matches!(&error, error if error.kind() == RuntimeErrorKind::Cancelled));
            assert_eq!(calls.load(Ordering::SeqCst), 1);
            assert_eq!(
                session.runtime().resources().counters().current_call_depth,
                0
            );
            assert!(
                session
                    .runtime_mut()
                    .execute_module(&loaded, "ready", session_options.clone())
                    .is_err()
            );
            drop(session);
            vm.collect_garbage().unwrap();
            assert_eq!(vm.gc_stats().objects, baseline.objects);
            assert_eq!(
                vm.gc_stats().root_registrations,
                baseline.root_registrations
            );
            assert_eq!(
                vm.execute_module(&loaded, "ready", vm.execution_options())
                    .unwrap()
                    .return_value
                    .as_i32(),
                Some(7)
            );
            assert!(!vm.is_quarantined());
        }
    }
}

#[test]
fn staged_modules_cannot_execute_effects_through_an_ordinary_vm_entry() {
    for encoded in [false, true] {
        let program = || {
            route(
                compile_logging_test_bytecode(include_str!(
                    "../../../tests/fixtures/execution/session-staged-effect.kgr"
                )),
                encoded,
            )
        };
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = calls.clone();
        let mut runtime = runtime();
        logging_module(move |_, _| {
            observed.fetch_add(1, Ordering::SeqCst);
            Ok(ValueView::unit())
        })
        .install(&mut runtime)
        .unwrap();
        let old = load_bytecode(&mut runtime, "staged.kgr", program()).unwrap();
        let _old_program = old;
        let old = runtime.select_program(&_old_program).unwrap();
        let candidate = runtime
            .stage_reload_program(&_old_program, Some("staged.kgr".into()), program())
            .unwrap();
        let mut vm = runtime;
        let error = vm
            .execute_module(
                candidate.module(),
                ProgramEntry::Main,
                vm.execution_options(),
            )
            .unwrap_err();
        assert!(
            matches!(&error, error if error.kind() == RuntimeErrorKind::ExecutionPhaseViolation)
        );
        assert!(vm.execution_root().is_none());
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        let mut session = vm.begin_candidate_initialization(&candidate).unwrap();
        let session_options = session.runtime_mut().execution_options();
        let session_options = session_options.clone();
        assert!(
            session
                .runtime_mut()
                .begin_candidate_initialization(&candidate)
                .is_err()
        );
        let error = session
            .runtime_mut()
            .execute_module(
                candidate.module(),
                ProgramEntry::Main,
                session_options.clone(),
            )
            .unwrap_err();
        assert!(
            matches!(&error, error if error.kind() == RuntimeErrorKind::ExecutionPhaseViolation)
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        drop(session);
        drop(candidate);
        assert_eq!(vm.modules().loaded_count(), old.members().count());
        assert!(vm.execution_root().is_none());
        assert!(!vm.is_quarantined());
    }
}

#[test]
fn host_created_err_captures_script_site_and_reentry_traps_keep_inner_origin() {
    for encoded in [false, true] {
        let module = compile_logging_test_bytecode(include_str!(
            "../../../tests/fixtures/execution/session-error-origin.kgr"
        ));
        let fail = module.records().modules[module.records().root.index()]
            .functions
            .iter()
            .find(|f| f.name == "fail")
            .unwrap()
            .id;
        let captured = Arc::new(Mutex::new(None));
        let saved = captured.clone();
        let mut runtime = runtime();
        logging_module(move |context, _| {
            let owner = context.runtime().execution_root().unwrap();
            let ty = binding::result(
                &context.runtime().host().definitions(),
                Ty::Builtin(BuiltinType::I32),
                Ty::Builtin(BuiltinType::String),
            )
            .unwrap();
            let applied = context
                .runtime()
                .resolve_type_arguments(&owner, &[ty])
                .unwrap()
                .remove(0);
            let text = ConversionContext::new(context.runtime_mut(), &owner)
                .unwrap()
                .encode("host failure".to_owned())
                .unwrap();
            let value = context
                .runtime_mut()
                .make_enum_member(&owner, &applied, "Err", &[text])
                .unwrap();
            *saved.lock().unwrap() = context.runtime_mut().result_failure(&value);
            let root = context.runtime().execution_root().unwrap();
            let arguments = context.retain_temporaries(&[]).unwrap();
            let error = reenter(context, &root, fail, arguments).unwrap_err();
            Err(HostError::new("nested call failed").with_trace(error.trace().unwrap().clone()))
        })
        .install(&mut runtime)
        .unwrap();
        let loaded =
            load_bytecode(&mut runtime, "host-origin.kgr", route(module, encoded)).unwrap();
        let _loaded_program = loaded;
        let loaded = runtime.select_program(&_loaded_program).unwrap();
        let mut vm = runtime;
        vm.collect_garbage().unwrap();
        let baseline = vm.gc_stats();
        let error = vm
            .execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
            .unwrap_err();
        assert_eq!(
            error
                .trace()
                .unwrap()
                .frames
                .iter()
                .map(|frame| frame.function_name.as_str())
                .collect::<Vec<_>>(),
            ["fail", "log", "main"]
        );
        let saved = captured.lock().unwrap();
        let report = saved.as_ref().unwrap();
        assert_eq!(report.message, "host failure");
        assert_eq!(report.trace.frames.len(), 2);
        assert_eq!(report.trace.frames[0].function_name, "log");
        assert_eq!(report.trace.frames[1].function_name, "main");
        vm.collect_garbage().unwrap();
        assert_eq!(
            vm.gc_stats().root_registrations,
            baseline.root_registrations
        );
        assert_eq!(vm.gc_stats().objects, baseline.objects);
        assert!(vm.execution_root().is_none());
    }
}
