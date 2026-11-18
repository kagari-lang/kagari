use crate::{
    BytecodeArtifact,
    context::{ExecutionContext, JitPolicy},
    engine::KagariEngine,
    program::Program,
    runtime::Runtime,
};
use kagari_runtime::{
    error::{RuntimeError, RuntimeErrorKind},
    frame::ExecutionFrame,
    host::{HostObjectId, HostSchemaEpoch},
    native::{
        builder::ModuleBuilder,
        declarations::{FunctionDecl, MethodDecl},
        host,
        storage::NativeStorage,
        typed::{NativeContext, loans::LoanContext},
        types::Type,
    },
    reload::ReloadValidationError,
    session::{ExecutionEvent, ExecutionObserver},
    value::view::ValueView,
};
use kagari_source::source::SourceFile;
use kagari_stdlib as stdlib;
use kagari_types::{
    declaration::host::{
        HostCallPolicy, HostFunctionEffects, HostPassingStyle, HostReflectionPolicy,
        HostTypeOwnership, HostTypePolicy, PathAccess,
    },
    reference::ReferenceAccess,
};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

fn decoded_program(engine: &KagariEngine, filename: &str, source: &str) -> Program {
    let artifact = engine
        .compile_to_artifact(SourceFile::new(filename, source), Default::default())
        .unwrap();
    let bytes = artifact.records().to_bytes(artifact.definitions()).unwrap();
    Program::from_artifact(
        BytecodeArtifact::from_bytes(&bytes).unwrap(),
        &Default::default(),
        &Default::default(),
    )
    .unwrap()
}

kagari_runtime::native_data! { struct Stats { hp: i32 } }

#[test]
fn opaque_loans_accept_compatible_reloaded_descriptors() {
    let mut engine = KagariEngine::builder().unwrap();
    engine.install_all(stdlib::modules().unwrap()).unwrap();
    let mut module = ModuleBuilder::new("game::reload", engine.declarations()).unwrap();
    let mut stats = module.define_type("Stats").unwrap();
    stats
        .native_storage(NativeStorage::data::<Stats>())
        .unwrap();
    let stats = stats.finish().unwrap();
    module
        .implement(stats.clone(), |implementation| {
            implementation.inherent_impl(|methods| {
                let read = methods.define_method(
                    MethodDecl::instance("read")
                        .receiver_access(ReferenceAccess::Shared)
                        .returns(Type::i32()),
                )?;
                methods.bind(read, |_: &mut LoanContext<'_>, (stats,): (&Stats,)| {
                    Ok(stats.hp)
                })?;
                let heal = methods.define_method(
                    MethodDecl::instance("heal")
                        .receiver_access(ReferenceAccess::Mutable)
                        .returns(Type::i32()),
                )?;
                methods.bind(heal, |_: &mut LoanContext<'_>, (stats,): (&mut Stats,)| {
                    stats.hp += 1;
                    Ok(stats.hp)
                })
            })
        })
        .unwrap();
    engine.install(module.finish().unwrap()).unwrap();
    let engine = engine.build().unwrap();
    let initial = decoded_program(
        &engine,
        "scoped-reload.kgr",
        include_str!("../../../../tests/fixtures/embedding/reload/opaque-loans-v1.kgr"),
    );
    let replacement = decoded_program(
        &engine,
        "scoped-reload.kgr",
        include_str!("../../../../tests/fixtures/embedding/reload/opaque-loans-v2.kgr"),
    );
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime.load_program(&initial, Default::default()).unwrap();
    let current = loaded.clone();
    let before = runtime.select_program(&loaded).unwrap();
    let descriptor = runtime
        .bind_native_type::<Stats>(&loaded.root(), &stats, &[])
        .unwrap();
    let value = descriptor
        .create(
            &mut NativeContext::new(&mut runtime, &before).unwrap(),
            Stats { hp: 40 },
        )
        .unwrap();
    let alias = value.clone();
    assert_eq!(
        runtime
            .execute_typed::<_, i32>(&loaded, "inspect", (alias.clone(),), &context)
            .unwrap(),
        40
    );

    runtime
        .program(&loaded)
        .map_err(ReloadValidationError::Runtime)
        .and_then(|base| base.prepare_replacement(&replacement, &Default::default()))
        .and_then(|candidate| runtime.replace_program(&loaded, &candidate, Default::default()))
        .unwrap();
    let after = runtime.select_program(&current).unwrap();
    assert_ne!(before.epoch, after.epoch);
    for (entry, expected) in [
        ("inspect", 140),
        ("heal", 41),
        ("inspect", 141),
        ("heal", 42),
    ] {
        let (program, argument) = if entry == "inspect" {
            (&loaded, &alias)
        } else {
            (&current, &value)
        };
        assert_eq!(
            runtime
                .execute_typed::<_, i32>(program, entry, (argument.clone(),), &context)
                .unwrap(),
            expected
        );
    }
    {
        let mut cx = NativeContext::new(&mut runtime, &after).unwrap();
        assert_eq!(value.read(&mut cx, |stats| Ok(stats.hp)).unwrap(), 42);
        alias
            .edit(&mut cx, |stats| {
                stats.hp += 1;
                Ok(())
            })
            .unwrap();
    }
    assert_eq!(
        runtime
            .execute_typed::<_, i32>(&loaded, "inspect", (value,), &context)
            .unwrap(),
        143
    );
}

#[derive(Debug)]
struct ContextObserver(Arc<AtomicUsize>);

impl ExecutionObserver for ContextObserver {
    fn observe(
        &mut self,
        runtime: &Runtime,
        _: ExecutionEvent,
        _: &[ExecutionFrame],
    ) -> Result<(), RuntimeError> {
        assert_eq!(runtime.execution_time_millis()?, 123);
        assert!(runtime.execution_options().record_host_calls);
        self.0.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
}

#[test]
fn scoped_host_entries_use_explicit_context_and_release_argument_loans() {
    const HP: HostObjectId = HostObjectId(1);
    const GOLD: HostObjectId = HostObjectId(2);

    let mut engine = KagariEngine::builder().unwrap();
    engine.install_all(stdlib::modules().unwrap()).unwrap();
    let mut module = ModuleBuilder::new("game::scoped", engine.declarations()).unwrap();
    let mut counter = module.define_type("Counter").unwrap();
    counter
        .external(HostTypePolicy {
            ownership: HostTypeOwnership::HostRoot,
            reflection: HostReflectionPolicy::Hidden,
            path_access: PathAccess::ReadWrite,
        })
        .unwrap();
    let counter = counter.finish().unwrap().apply([]).unwrap();
    let state = Arc::new(Mutex::new((40, 25)));
    let heal = module
        .define_function(
            FunctionDecl::new("heal")
                .parameter("hp", counter.clone())
                .parameter("gold", counter.clone())
                .returns(Type::i32())
                .host_policy(HostCallPolicy {
                    passing: vec![HostPassingStyle::UniqueBorrow; 2],
                    effects: HostFunctionEffects {
                        may_mutate_host_state: true,
                        ..Default::default()
                    },
                }),
        )
        .unwrap();
    let write = state.clone();
    module
        .bind(
            heal,
            host::callback(move |cx, _| {
                for id in [HP, GOLD] {
                    let root = cx.runtime().host().root(id).unwrap();
                    assert_eq!(
                        cx.borrows()
                            .borrow_shared(id, root.type_id())
                            .unwrap_err()
                            .kind(),
                        RuntimeErrorKind::HostBorrowConflict
                    );
                }
                let mut state = write.lock().unwrap();
                state.0 += 30;
                state.1 -= 5;
                Ok(ValueView::from(state.0))
            }),
        )
        .unwrap();
    let read = module
        .define_function(
            FunctionDecl::new("read")
                .parameter("hp", counter)
                .returns(Type::i32())
                .host_policy(HostCallPolicy {
                    passing: vec![HostPassingStyle::SharedBorrow],
                    effects: Default::default(),
                }),
        )
        .unwrap();
    let inspect = state.clone();
    module
        .bind(
            read,
            host::callback(move |cx, _| {
                let root = cx.runtime().host().root(HP).unwrap();
                assert_eq!(
                    cx.borrows()
                        .borrow_unique(HP, root.type_id())
                        .unwrap_err()
                        .kind(),
                    RuntimeErrorKind::HostBorrowConflict
                );
                Ok(ValueView::from(inspect.lock().unwrap().0))
            }),
        )
        .unwrap();
    engine.install(module.finish().unwrap()).unwrap();
    let engine = engine.build().unwrap();
    let program = decoded_program(
        &engine,
        "scoped-sdk.kgr",
        include_str!("../../../../tests/fixtures/embedding/host/scoped-context.kgr"),
    );
    let mut runtime = engine.runtime(ExecutionContext::default());
    let loaded = runtime.load_program(&program, Default::default()).unwrap();
    let counter = runtime
        .types()
        .get_by_name("game::scoped::Counter")
        .unwrap()
        .id;
    let [hp, gold] = [HP, GOLD].map(|id| {
        let root = runtime
            .register_host_root(id, counter, HostSchemaEpoch::new(0))
            .unwrap();
        runtime.host_root_value(root).unwrap()
    });
    runtime.collect_garbage().unwrap();
    let baseline_roots = runtime.gc_stats().root_registrations;
    let observations = Arc::new(AtomicUsize::new(0));
    runtime
        .set_execution_observer(ContextObserver(observations.clone()))
        .unwrap();
    let mut context = ExecutionContext {
        tracing_enabled: true,
        ..Default::default()
    };
    context.inputs.unix_time_millis = 123;
    runtime
        .with_host_scope(|scope| {
            let hp = scope.import(&hp)?;
            let gold = scope.import(&gold)?;
            let args = [hp, gold];
            let unsupported = ExecutionContext {
                jit_policy: JitPolicy::Enabled,
                ..context.clone()
            };
            assert_eq!(
                Runtime::execute_scoped(scope, &loaded, "tick", &args, &unsupported)
                    .unwrap_err()
                    .kind(),
                RuntimeErrorKind::UnsupportedExecution
            );
            let cancelled = ExecutionContext {
                cancellation: Default::default(),
                ..context.clone()
            };
            cancelled.cancellation.cancel();
            assert_eq!(
                Runtime::execute_scoped(scope, &loaded, "tick", &args, &cancelled)
                    .unwrap_err()
                    .kind(),
                RuntimeErrorKind::Cancelled
            );
            assert_eq!(*state.lock().unwrap(), (40, 25));

            // The first unique reservation must be released when the second argument aliases it.
            assert_eq!(
                Runtime::execute_scoped(scope, &loaded, "tick", &[hp, hp], &context)
                    .unwrap_err()
                    .kind(),
                RuntimeErrorKind::HostBorrowConflict
            );
            assert_eq!(
                Runtime::execute_scoped(scope, &loaded, "inspect", &[hp], &context)?.as_i32(),
                Some(40)
            );
            assert_eq!(
                Runtime::execute_scoped(scope, &loaded, "tick", &args, &context)?.as_i32(),
                Some(70)
            );
            assert_eq!(
                Runtime::execute_scoped(scope, &loaded, "inspect", &[hp], &context)?.as_i32(),
                Some(70)
            );
            Ok(())
        })
        .unwrap();
    assert_eq!(*state.lock().unwrap(), (70, 20));
    assert!(observations.load(Ordering::Relaxed) > 0);
    assert!(runtime.execution_root().is_none());
    assert_eq!(runtime.resources().counters().current_call_depth, 0);
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc_stats().root_registrations, baseline_roots);
    assert!(!runtime.is_quarantined());
}
