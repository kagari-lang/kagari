use super::source_free_vm;
use crate::error::VmError;
use crate::tests::native_boundary::{compile_program, support::round_trip_program};
use crate::vm::Vm;
use kagari_common::identity::metadata::DefinitionMetadata;
use kagari_runtime::{
    RuntimeConfig,
    error::RuntimeErrorKind,
    gc::GcHeapConfig,
    host::{
        HostCallContext, HostError, HostObjectId, HostPathAdapter, HostSchemaEpoch,
        PreparedHostPathWrite,
    },
    native::{
        builder::ModuleBuilder,
        context::CallContext,
        declarations::{FunctionDecl, MethodDecl},
        installation::NativeInstallation,
        types::Type,
    },
    session::{DeterministicInputs, ExecutionPhase, TraceValue},
    value::Value,
};
use kagari_types::{
    declaration::host::{
        HostCallPolicy, HostFunctionEffects, HostPassingStyle, HostReflectionPolicy,
        HostTypeOwnership, HostTypePolicy, PathAccess,
    },
    declaration::path::{
        HostPathDeclaration, HostPathSegmentDeclaration, HostVirtualSegmentDeclaration,
    },
    scalar::BuiltinType,
    ty::Ty,
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

#[test]
fn host_policy_uses_native_entry_with_rooted_inputs_and_execution_session() {
    let catalog = kagari_stdlib::declarations::StandardDeclarations::default()
        .catalog()
        .unwrap();
    let mut builder = ModuleBuilder::new("example::services", &catalog).unwrap();
    let echo = builder
        .define_function(
            FunctionDecl::new("echo")
                .parameter("text", Type::scalar(BuiltinType::String))
                .returns(Type::scalar(BuiltinType::String))
                .host_policy(HostCallPolicy {
                    passing: vec![HostPassingStyle::SharedBorrow],
                    effects: HostFunctionEffects {
                        may_allocate: true,
                        may_call_host_services: true,
                        ..Default::default()
                    },
                }),
        )
        .unwrap();
    assert!(
        builder
            .bind(echo.clone(), |cx: &mut CallContext<'_>| cx.argument(0))
            .is_err()
    );
    builder
        .bind(echo.clone(), |cx: &HostCallContext<'_>, args: &[Value]| {
            cx.runtime().collect_garbage().unwrap();
            let Value::Str(id) = args[0] else {
                panic!("string argument")
            };
            let text = cx.runtime().gc().string(id).unwrap().to_owned();
            let time = cx.execution_time_millis().unwrap();
            cx.runtime()
                .gc()
                .alloc_string(format!("{text}:{time}"))
                .map_err(|error| HostError::new(error.to_string()))
        })
        .unwrap();
    let entered = Arc::new(AtomicUsize::new(0));
    for name in ["configuration", "fail", "cancel", "panic"] {
        let function = builder
            .define_function(FunctionDecl::new(name).returns(Type::i32()).host_policy(
                HostCallPolicy {
                    passing: vec![],
                    effects: HostFunctionEffects {
                        may_trap: true,
                        may_read_immutable_configuration: name == "configuration",
                        may_call_host_services: name != "configuration",
                        ..Default::default()
                    },
                },
            ))
            .unwrap();
        let entered = entered.clone();
        builder
            .bind(function, move |cx: &HostCallContext<'_>, _: &[Value]| {
                entered.fetch_add(1, Ordering::Relaxed);
                match name {
                    "configuration" => Ok(Value::I32(7)),
                    "fail" => Err(HostError::new("service failed")),
                    "cancel" => {
                        cx.runtime().execution_options().cancellation.cancel();
                        Err(HostError::new("failure after cancellation"))
                    }
                    _ => panic!("service unwind"),
                }
            })
            .unwrap();
    }
    let missing_policy = builder
        .define_function(FunctionDecl::new("unclassified"))
        .unwrap();
    assert!(
        builder
            .bind(
                missing_policy.clone(),
                |_: &HostCallContext<'_>, _: &[Value]| Ok(Value::Unit)
            )
            .is_err()
    );
    // A rejected binding does not consume the declaration; complete it normally.
    builder
        .bind(missing_policy, |_: &mut CallContext<'_>| Ok(Value::Unit))
        .unwrap();
    let module = builder.finish().unwrap();
    let (vm, owner) = source_free_vm(
        r#"
        use example::services;
        pub fn run() -> String { services::echo("hello") }
        pub fn configuration() -> i32 { services::configuration() }
        pub fn fail() -> i32 { services::fail() }
        pub fn cancel() -> i32 { services::cancel() }
        pub fn unwind() -> i32 { services::panic() }
        "#,
        &module,
    );
    {
        let mut options = vm.runtime().execution_options();
        options.record_host_calls = true;
        options.inputs = DeterministicInputs {
            unix_time_millis: 123,
            random_seed: 9,
        };
        let session = vm.runtime().begin_execution(&owner, options).unwrap();
        let report = vm.execute(&owner, "run").unwrap();
        let Value::Str(id) = report.return_value.value(vm.runtime().gc()).unwrap() else {
            panic!("string result")
        };
        assert_eq!(&*vm.runtime().gc().string(id).unwrap(), "hello:123");
        assert_eq!(session.host_scope_count(), 0);
        let error = vm.execute(&owner, "fail").unwrap_err();
        assert!(
            matches!(error.cause(), VmError::RuntimeError(error) if error.kind() == RuntimeErrorKind::HostCallFailure && error.message() == "host call failed: service failed"),
            "{error:?}"
        );
        let trace = session.trace().unwrap();
        assert_eq!(trace.host_calls.len(), 2);
        assert!(
            trace.host_calls[0]
                .symbol
                .ends_with("example::services::echo")
        );
        assert_eq!(
            trace.host_calls[0].arguments,
            vec![TraceValue::Str {
                prefix: "hello".into(),
                truncated: false
            }]
        );
        assert_eq!(
            trace.host_calls[0].outcome,
            Some(Ok(TraceValue::Str {
                prefix: "hello:123".into(),
                truncated: false
            }))
        );
        assert_eq!(
            trace.host_calls[1].outcome,
            Some(Err(RuntimeErrorKind::HostCallFailure))
        );
        assert_eq!(session.host_scope_count(), 0);
    }
    {
        let mut options = vm.runtime().execution_options();
        options.phase = ExecutionPhase::CandidateInitialization;
        let session = vm.runtime().begin_execution(&owner, options).unwrap();
        let before = entered.load(Ordering::Relaxed);
        let report = vm.execute(&owner, "configuration").unwrap();
        assert_eq!(
            report.return_value.value(vm.runtime().gc()),
            Some(Value::I32(7))
        );
        let error = vm.execute(&owner, "fail").unwrap_err();
        assert!(
            matches!(error.cause(), VmError::RuntimeError(error) if error.kind() == RuntimeErrorKind::ExecutionPhaseViolation)
        );
        assert_eq!(entered.load(Ordering::Relaxed), before + 1);
        assert_eq!(session.host_scope_count(), 0);
    }
    {
        let session = vm
            .runtime()
            .begin_execution(&owner, vm.runtime().execution_options())
            .unwrap();
        let error = vm.execute(&owner, "cancel").unwrap_err();
        assert!(
            matches!(error.cause(), VmError::RuntimeError(error) if error.kind() == RuntimeErrorKind::Cancelled)
        );
        assert_eq!(session.host_scope_count(), 0);
    }
    let unwind = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        vm.execute(&owner, "unwind")
    }));
    assert!(unwind.is_err());
    assert!(!vm.runtime().is_quarantined());
    assert_eq!(vm.runtime().resources().counters().current_call_depth, 0);
    assert!(vm.execute(&owner, "run").is_ok());
}

#[test]
fn shared_root_registration_runs_native_methods_and_explicit_traits_source_free() {
    let catalog = kagari_stdlib::declarations::StandardDeclarations::default()
        .catalog()
        .unwrap();
    let mut builder = ModuleBuilder::new("example::roots", &catalog).unwrap();
    let mut player = builder.define_type("Player").unwrap();
    player
        .external(HostTypePolicy {
            ownership: HostTypeOwnership::HostRoot,
            reflection: HostReflectionPolicy::Metadata,
            path_access: PathAccess::ReadWrite,
        })
        .unwrap();
    let player = player.finish().unwrap();
    let identity = builder
        .define_path(
            DefinitionMetadata::checked(
                player.definitions().clone(),
                HostPathDeclaration {
                    root: player.id(),
                    segments: vec![HostPathSegmentDeclaration::Virtual(
                        HostVirtualSegmentDeclaration {
                            name: "identity".into(),
                            result: Ty::Host(player.id()),
                            access: PathAccess::ReadWrite,
                        },
                    )],
                    access: PathAccess::ReadWrite,
                    schema_epoch: 3,
                },
                &Default::default(),
            )
            .unwrap(),
        )
        .unwrap();
    let preview = builder
        .define_path(
            DefinitionMetadata::checked(
                player.definitions().clone(),
                HostPathDeclaration {
                    root: player.id(),
                    segments: vec![HostPathSegmentDeclaration::Virtual(
                        HostVirtualSegmentDeclaration {
                            name: "preview".into(),
                            result: Ty::Host(player.id()),
                            access: PathAccess::ReadOnly,
                        },
                    )],
                    access: PathAccess::ReadOnly,
                    schema_epoch: 3,
                },
                &Default::default(),
            )
            .unwrap(),
        )
        .unwrap();
    builder
        .bind_path(
            &preview,
            HostPathAdapter::new().with_read(|context, path| {
                context
                    .runtime()
                    .gc()
                    .alloc_host_root(path.root)
                    .map_err(|error| HostError::new(error.to_string()))
            }),
        )
        .unwrap();
    builder
        .bind_path(
            &identity,
            HostPathAdapter::new()
                .with_read(|context, path| {
                    context
                        .runtime()
                        .gc()
                        .alloc_host_root(path.root)
                        .map_err(|error| HostError::new(error.to_string()))
                })
                .with_prepare_write(|_, _, _| Ok(PreparedHostPathWrite::new(|| {}))),
        )
        .unwrap();
    let mut interface = builder.define_trait("HasHp").unwrap();
    interface
        .define_method(MethodDecl::instance("hp").returns(Type::i32()))
        .unwrap();
    let interface = interface.finish().unwrap();
    let state = Arc::new(AtomicUsize::new(10));
    builder
        .implement(player, |group| {
            let edited = state.clone();
            group.inherent_impl(|methods| {
                let method = methods.define_method(
                    MethodDecl::instance("heal")
                        .parameter("amount", Type::i32())
                        .returns(Type::i32())
                        .host_policy(HostCallPolicy {
                            passing: vec![HostPassingStyle::UniqueBorrow, HostPassingStyle::Owned],
                            effects: HostFunctionEffects {
                                may_mutate_host_state: true,
                                ..Default::default()
                            },
                        }),
                )?;
                methods.bind(
                    method,
                    move |context: &HostCallContext<'_>, values: &[Value]| {
                        context.runtime().collect_garbage().unwrap();
                        let [value, Value::I32(amount)] = values else {
                            panic!("root and amount")
                        };
                        assert_external_argument(context, value, PathAccess::ReadWrite);
                        Ok(Value::I32(
                            (edited.fetch_add(*amount as usize, Ordering::SeqCst)
                                + *amount as usize) as i32,
                        ))
                    },
                )
            })?;
            let read = state.clone();
            let identity = identity.clone();
            group.trait_impl(interface.apply([])?, |methods| {
                methods.host_policy(
                    "hp",
                    HostCallPolicy {
                        passing: vec![HostPassingStyle::SharedBorrow],
                        effects: Default::default(),
                    },
                )?;
                methods.bind(
                    "hp",
                    move |context: &HostCallContext<'_>, values: &[Value]| {
                        context.runtime().collect_garbage().unwrap();
                        let [value] = values else { panic!("root") };
                        assert_external_argument(context, value, PathAccess::ReadOnly);
                        if matches!(value, Value::HostPathView(_)) {
                            let path = context.runtime().resolve_host_path(&identity).unwrap();
                            let nested = context
                                .runtime()
                                .make_host_path_view_from_value(value, path, vec![])
                                .unwrap();
                            assert_eq!(nested.access(), PathAccess::ReadOnly);
                            assert!(
                                context
                                    .runtime()
                                    .set_host_path(value, path, vec![], *value)
                                    .is_err()
                            );
                        }
                        Ok(Value::I32(read.load(Ordering::SeqCst) as i32))
                    },
                )
            })
        })
        .unwrap();
    let module = builder.finish().unwrap();
    let source = r#"
        use example::roots::{Player, HasHp};
        fn hp<T: HasHp>(player: T) -> i32 { player.hp() }
        pub fn main(player: Player) -> i32 { player.heal(5); hp(player) }
        pub fn read(player: Player) -> i32 { hp(player) }
    "#;
    let program = round_trip_program(compile_program(source, Some(&module)));
    let mut modules = kagari_stdlib::modules().unwrap();
    modules.push(module.clone());
    let installation = NativeInstallation::prepare(&modules).unwrap();
    let mut runtime = installation.runtime(RuntimeConfig {
        gc: GcHeapConfig {
            collection_threshold: Some(1),
        },
        ..Default::default()
    });
    let type_id = runtime
        .types()
        .get_by_name("example::roots::Player")
        .unwrap()
        .id;
    let root = runtime
        .register_host_root(HostObjectId(17), type_id, HostSchemaEpoch::new(3))
        .unwrap();
    let value = runtime.gc().alloc_host_root(root).unwrap();
    let value = runtime.gc().root_execution_values(vec![value]).unwrap();
    let owner = runtime.load_program("shared-root", program).unwrap();
    let mut vm = Vm::new(runtime);
    let argument = value.get(vm.runtime().gc(), 0).unwrap();
    vm.runtime_mut()
        .with_host_scope(|scope| {
            assert_eq!(
                Vm::execute_scoped(scope, &owner, "main", &[argument], Default::default())?,
                Value::I32(15)
            );
            Ok(())
        })
        .unwrap();
    assert_eq!(state.load(Ordering::SeqCst), 15);
    let path = vm.runtime().resolve_host_path(&identity).unwrap();
    let view = vm
        .runtime()
        .make_host_path_view(root, path, Default::default())
        .unwrap();
    let view = vm.runtime().gc().alloc_host_path(view).unwrap();
    let view = vm.runtime().gc().root_execution_values(vec![view]).unwrap();
    let view_value = view.get(vm.runtime().gc(), 0).unwrap();
    vm.runtime_mut()
        .with_host_scope(|scope| {
            assert_eq!(
                Vm::execute_scoped(scope, &owner, "main", &[view_value], Default::default())?,
                Value::I32(20)
            );
            Ok(())
        })
        .unwrap();
    assert_eq!(state.load(Ordering::SeqCst), 20);
    let preview_path = vm.runtime().resolve_host_path(&preview).unwrap();
    let readonly = vm
        .runtime()
        .make_host_path_view(root, preview_path, Default::default())
        .unwrap();
    let readonly = vm.runtime().gc().alloc_host_path(readonly).unwrap();
    let readonly = vm
        .runtime()
        .gc()
        .root_execution_values(vec![readonly])
        .unwrap();
    let readonly_value = readonly.get(vm.runtime().gc(), 0).unwrap();
    vm.runtime_mut()
        .with_host_scope(|scope| {
            assert_eq!(
                Vm::execute_scoped(scope, &owner, "read", &[readonly_value], Default::default())?,
                Value::I32(20)
            );
            assert!(
                Vm::execute_scoped(scope, &owner, "main", &[readonly_value], Default::default())
                    .is_err()
            );
            Ok(())
        })
        .unwrap();
    assert_eq!(state.load(Ordering::SeqCst), 20);
    let Value::HostPathView(id) = view.get(vm.runtime().gc(), 0).unwrap() else {
        panic!("view")
    };
    assert_eq!(
        vm.runtime().gc().host_path(id).unwrap().access(),
        PathAccess::ReadWrite
    );
    let scope = vm.runtime().host_scope(&[]).unwrap();
    scope
        .borrows()
        .borrow_unique(HostObjectId(17), type_id)
        .unwrap();
    drop(scope);
    let mut other = installation.runtime(Default::default());
    let foreign = other
        .register_host_root(HostObjectId(17), type_id, HostSchemaEpoch::new(3))
        .unwrap();
    let foreign = vm.runtime().gc().alloc_host_root(foreign).unwrap();
    assert!(
        vm.runtime_mut()
            .with_host_scope(|scope| Vm::execute_scoped(
                scope,
                &owner,
                "main",
                &[foreign],
                Default::default()
            ))
            .is_err()
    );
    assert_eq!(state.load(Ordering::SeqCst), 20);
}

fn assert_external_argument(context: &HostCallContext<'_>, value: &Value, access: PathAccess) {
    let runtime = context.runtime();
    let object = match value {
        Value::HostRoot(id) => runtime.gc().host_root(*id).unwrap().object_id(),
        Value::HostPathView(id) => {
            let view = runtime.gc().host_path(*id).unwrap();
            assert_eq!(view.access(), access);
            view.root().object_id()
        }
        _ => panic!("external argument"),
    };
    assert_eq!(object, HostObjectId(17));
    if access == PathAccess::ReadOnly {
        assert!(
            context
                .borrows()
                .borrow_unique(
                    object,
                    runtime
                        .types()
                        .get_by_name("example::roots::Player")
                        .unwrap()
                        .id
                )
                .is_err()
        );
    }
}
