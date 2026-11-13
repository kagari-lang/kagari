use super::source_free_vm;
use crate::tests::execution::native_boundary::{compile_program, support::round_trip_program};
use kagari_runtime::{
    Runtime, RuntimeConfig,
    error::RuntimeErrorKind,
    execution::memory::GcHeapConfig,
    host::{
        HostCallContext, HostError, HostObjectId, HostPathAdapter, HostPathDescriptorId,
        HostSchemaEpoch, PreparedHostPathWrite,
    },
    native::{
        binding::{Codec, NativeBinding},
        builder::ModuleBuilder,
        context::NativeCall,
        conversion::context::ConversionContext,
        declarations::{FunctionDecl, MethodDecl},
        host,
        installation::NativeInstallation,
        types::Type,
    },
    session::{DeterministicInputs, ExecutionPhase, TraceValue},
    value::{kind::ObjectKind, view::ValueView},
};

#[cfg(test)]
use crate::tests::support::bytecode::load_bytecode;

use kagari_common::identity::metadata::DefinitionMetadata;
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
    let catalog = crate::declarations::StandardDeclarations::default()
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
            .bind(
                echo.clone(),
                NativeBinding::new(vec![Codec::Value], Codec::Value, |cx| cx.argument(0))
            )
            .is_err()
    );
    builder
        .bind(
            echo.clone(),
            host::callback(|cx, args| {
                cx.runtime_mut().collect_garbage().unwrap();
                let text = cx.read_string(&args[0], str::to_owned).unwrap();
                let time = cx.execution_time_millis().unwrap();
                cx.allocate_string(format!("{text}:{time}"))
                    .map_err(|error| HostError::new(error.to_string()))
            }),
        )
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
            .bind(
                function,
                host::callback(move |cx, _| {
                    entered.fetch_add(1, Ordering::Relaxed);
                    match name {
                        "configuration" => Ok(ValueView::from(7i32)),
                        "fail" => Err(HostError::new("service failed")),
                        "cancel" => {
                            cx.runtime().execution_options().cancellation.cancel();
                            Err(HostError::new("failure after cancellation"))
                        }
                        _ => panic!("service unwind"),
                    }
                }),
            )
            .unwrap();
    }
    let missing_policy = builder
        .define_function(FunctionDecl::new("unclassified"))
        .unwrap();
    assert!(
        builder
            .bind(
                missing_policy.clone(),
                host::callback(|_, _| Ok(ValueView::unit()))
            )
            .is_err()
    );
    // A rejected binding does not consume the declaration; complete it normally.
    builder
        .bind(missing_policy, |_: &mut NativeCall<'_>| Ok(()))
        .unwrap();
    let module = builder.finish().unwrap();
    let (mut vm, owner) = source_free_vm(
        include_str!("../../../../../tests/fixtures/native/host-policy-trace.kgr"),
        &module,
    );
    let _owner_program = owner;
    let owner = vm.select_program(&_owner_program).unwrap();
    {
        let mut options = vm.execution_options();
        options.record_host_calls = true;
        options.inputs = DeterministicInputs {
            unix_time_millis: 123,
            random_seed: 9,
        };
        let mut session = vm.begin_execution(&owner, options).unwrap();
        let session_options = session.runtime().execution_options();
        let report = session
            .runtime_mut()
            .execute_module(&owner, "run", session_options.clone())
            .unwrap();
        assert_eq!(
            ConversionContext::new(session.runtime_mut(), &owner)
                .unwrap()
                .decode::<String>(&report.return_value)
                .unwrap(),
            "hello:123"
        );
        assert_eq!(session.host_scope_count(), 0);
        let error = session
            .runtime_mut()
            .execute_module(&owner, "fail", session_options.clone())
            .unwrap_err();
        assert!(
            matches!(&error, error if error.kind() == RuntimeErrorKind::HostCallFailure && error.to_string() == "host call failed: service failed"),
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
        let mut options = vm.execution_options();
        options.phase = ExecutionPhase::CandidateInitialization;
        let mut session = vm.begin_execution(&owner, options).unwrap();
        let session_options = session.runtime().execution_options();
        let before = entered.load(Ordering::Relaxed);
        let report = session
            .runtime_mut()
            .execute_module(&owner, "configuration", session_options.clone())
            .unwrap();
        assert_eq!(report.return_value.as_i32(), Some(7));
        let error = session
            .runtime_mut()
            .execute_module(&owner, "fail", session_options.clone())
            .unwrap_err();
        assert!(
            matches!(&error, error if error.kind() == RuntimeErrorKind::ExecutionPhaseViolation)
        );
        assert_eq!(entered.load(Ordering::Relaxed), before + 1);
        assert_eq!(session.host_scope_count(), 0);
    }
    {
        let mut session = vm.begin_execution(&owner, vm.execution_options()).unwrap();
        let session_options = session.runtime().execution_options();
        let error = session
            .runtime_mut()
            .execute_module(&owner, "cancel", session_options.clone())
            .unwrap_err();
        assert!(matches!(&error, error if error.kind() == RuntimeErrorKind::Cancelled));
        assert_eq!(session.host_scope_count(), 0);
    }
    let unwind = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        vm.execute_module(&owner, "unwind", vm.execution_options())
    }));
    assert!(unwind.is_err());
    assert!(!vm.is_quarantined());
    assert_eq!(vm.resources().counters().current_call_depth, 0);
    assert!(
        vm.execute_module(&owner, "run", vm.execution_options())
            .is_ok()
    );
}

#[test]
fn shared_root_registration_runs_native_methods_and_explicit_traits_source_free() {
    let catalog = crate::declarations::StandardDeclarations::default()
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
                let root = context
                    .runtime_mut()
                    .host_root_value(path.root)
                    .map_err(|error| HostError::new(error.to_string()))?;
                context
                    .import(&root)
                    .map_err(|error| HostError::new(error.to_string()))
            }),
        )
        .unwrap();
    builder
        .bind_path(
            &identity,
            HostPathAdapter::new()
                .with_read(|context, path| {
                    let root = context
                        .runtime_mut()
                        .host_root_value(path.root)
                        .map_err(|error| HostError::new(error.to_string()))?;
                    context
                        .import(&root)
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
            let heal_identity = identity.clone();
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
                    host::callback(move |context, values| {
                        context.runtime_mut().collect_garbage().unwrap();
                        let [value, amount] = values else {
                            panic!("root and amount")
                        };
                        let amount = amount.as_i32().expect("checked i32 amount");
                        let path = context.runtime().resolve_host_path(&heal_identity).unwrap();
                        assert_external_argument(context, value, PathAccess::ReadWrite, path);
                        Ok(ValueView::from(
                            (edited.fetch_add(amount as usize, Ordering::SeqCst) + amount as usize)
                                as i32,
                        ))
                    }),
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
                    host::callback(move |context, values| {
                        context.runtime_mut().collect_garbage().unwrap();
                        let [value] = values else { panic!("root") };
                        let path = context.runtime().resolve_host_path(&identity).unwrap();
                        assert_external_argument(context, value, PathAccess::ReadOnly, path);
                        if value.kind() == Some(ObjectKind::HostPath) {
                            let root = context.retain(value).unwrap();
                            let nested = context
                                .runtime_mut()
                                .make_host_path_view(&root, path, &[])
                                .unwrap();
                            let nested = context.import(&nested).unwrap();
                            assert_eq!(
                                context.inspect_path(&nested).unwrap().access,
                                PathAccess::ReadOnly
                            );
                            assert!(
                                context
                                    .runtime_mut()
                                    .set_host_path(&root, path, &[], &root)
                                    .is_err()
                            );
                        }
                        Ok(ValueView::from(read.load(Ordering::SeqCst) as i32))
                    }),
                )
            })
        })
        .unwrap();
    let module = builder.finish().unwrap();
    let source = include_str!("../../../../../tests/fixtures/native/host-root-methods.kgr");
    let program = round_trip_program(compile_program(source, Some(&module)));
    let mut modules = crate::modules().unwrap();
    modules.push(module.clone());
    let installation = NativeInstallation::prepare(&modules).unwrap();
    let mut runtime = installation.runtime(RuntimeConfig {
        gc: GcHeapConfig {
            collection_threshold: Some(1),
            ..Default::default()
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
    let value = runtime.host_root_value(root).unwrap();
    let owner = load_bytecode(&mut runtime, "shared-root", program).unwrap();
    let _owner_program = owner;
    let mut vm = runtime;
    vm.with_host_scope(|scope| {
        let argument = scope.import(&value)?;
        assert_eq!(
            Runtime::execute_scoped(
                scope,
                &_owner_program,
                "main",
                &[argument],
                &Default::default()
            )?
            .as_i32(),
            Some(15)
        );
        Ok(())
    })
    .unwrap();
    assert_eq!(state.load(Ordering::SeqCst), 15);
    let path = vm.resolve_host_path(&identity).unwrap();
    let view = vm.make_host_path_view(&value, path, &[]).unwrap();
    vm.with_host_scope(|scope| {
        let view_value = scope.import(&view)?;
        assert_eq!(
            Runtime::execute_scoped(
                scope,
                &_owner_program,
                "main",
                &[view_value],
                &Default::default()
            )?
            .as_i32(),
            Some(20)
        );
        Ok(())
    })
    .unwrap();
    assert_eq!(state.load(Ordering::SeqCst), 20);
    let preview_path = vm.resolve_host_path(&preview).unwrap();
    let readonly = vm.make_host_path_view(&value, preview_path, &[]).unwrap();
    vm.with_host_scope(|scope| {
        let readonly_value = scope.import(&readonly)?;
        assert_eq!(
            Runtime::execute_scoped(
                scope,
                &_owner_program,
                "read",
                &[readonly_value],
                &Default::default()
            )?
            .as_i32(),
            Some(20)
        );
        assert!(
            Runtime::execute_scoped(
                scope,
                &_owner_program,
                "main",
                &[readonly_value],
                &Default::default()
            )
            .is_err()
        );
        Ok(())
    })
    .unwrap();
    assert_eq!(state.load(Ordering::SeqCst), 20);
    // The hp callback already checks a narrowed argument. A later unique call
    // through the original path proves that the caller's capability was preserved.
    vm.with_host_scope(|scope| {
        let original = scope.import(&view)?;
        assert_eq!(
            Runtime::execute_scoped(
                scope,
                &_owner_program,
                "main",
                &[original],
                &Default::default()
            )?
            .as_i32(),
            Some(25)
        );
        Ok(())
    })
    .unwrap();
    assert_eq!(state.load(Ordering::SeqCst), 25);
    let scope = vm.host_scope(&[]).unwrap();
    scope
        .borrows()
        .borrow_unique(HostObjectId(17), type_id)
        .unwrap();
    drop(scope);
    let mut other = installation.runtime(Default::default());
    let foreign = other
        .register_host_root(HostObjectId(17), type_id, HostSchemaEpoch::new(3))
        .unwrap();
    assert_eq!(
        vm.host_root_value(foreign).unwrap_err().kind(),
        RuntimeErrorKind::TypedPathValidation
    );
    let foreign = other.host_root_value(foreign).unwrap();
    assert!(
        vm.with_host_scope(|scope| {
            let foreign = scope.import(&foreign)?;
            Runtime::execute_scoped(
                scope,
                &_owner_program,
                "main",
                &[foreign],
                &Default::default(),
            )
            .map(|_| ())
        })
        .is_err()
    );
    assert_eq!(state.load(Ordering::SeqCst), 25);
}

fn assert_external_argument(
    context: &mut HostCallContext<'_>,
    value: &ValueView<'_>,
    access: PathAccess,
    identity: HostPathDescriptorId,
) {
    let facts = if value.kind() == Some(ObjectKind::HostPath) {
        let facts = context.inspect_path(value).unwrap();
        assert_eq!(facts.access, access);
        facts
    } else {
        assert_eq!(value.kind(), Some(ObjectKind::HostRoot));
        let root = context.retain(value).unwrap();
        let path = context
            .runtime_mut()
            .make_host_path_view(&root, identity, &[])
            .unwrap();
        let path = context.import(&path).unwrap();
        context.inspect_path(&path).unwrap()
    };
    assert_eq!(facts.root.object_id(), HostObjectId(17));
    if access == PathAccess::ReadOnly {
        let type_id = context
            .runtime()
            .types()
            .get_by_name("example::roots::Player")
            .unwrap()
            .id;
        assert_eq!(
            context
                .borrows()
                .borrow_unique(HostObjectId(17), type_id)
                .unwrap_err()
                .kind(),
            RuntimeErrorKind::HostBorrowConflict
        );
    }
}
