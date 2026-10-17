use super::schema::NominalSchema;
use super::*;
use crate::{context::JitPolicy, program::PreparedProgram};
use kagari_bytecode::artifact::KbcArtifact;
use kagari_runtime::module::entry::ProgramEntry;
use kagari_runtime::{
    host::{HostCallContext, HostError, HostPathAdapter, PreparedHostPathWrite},
    native::{builder::ModuleBuilder, declarations::FunctionDecl, types::Type},
};

use kagari_types::ty::Ty;
use kagari_types::{
    declaration::{
        FieldDef,
        host::{
            HostCallPolicy, HostFieldPolicy, HostFunctionEffects, HostReflectionPolicy,
            HostTypePolicy,
        },
        path::{
            HostIndexSegmentDeclaration, HostPathDeclaration, HostPathSegmentDeclaration,
            HostVirtualSegmentDeclaration,
        },
    },
    scalar::BuiltinType,
    visibility::Visibility,
};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicI32, AtomicUsize, Ordering},
};

#[test]
fn source_field_chains_use_offline_contracts_and_evaluate_the_root_once() {
    let mut schema = NominalSchema::with_count(PathAccess::ReadOnly);
    let trace = Arc::new(Mutex::new(Vec::new()));
    schema.make(trace.clone(), "root");
    let unbound = schema.builder.clone().finish().unwrap();
    let path = schema.count_path(PathAccess::ReadOnly, 7);
    let definitions = DefinitionContext::new().unwrap();
    let path = path.import_into(&definitions, &Default::default()).unwrap();
    let calls = trace.clone();
    schema
        .builder
        .bind_path(
            &path,
            HostPathAdapter::new().with_read(move |context, _| {
                calls.lock().unwrap().push("read");
                context.runtime().collect_garbage().unwrap();
                Ok(Value::I32(42))
            }),
        )
        .unwrap();
    let expected = schema.required_types(&definitions);
    let providers = schema.providers.clone();
    let engine = schema.engine();

    let artifact = engine
        .compile_to_artifact(
            SourceFile::new(
                "field.kgr",
                "use left as api; fn main() -> i32 { api::make().related.count }",
            ),
            Default::default(),
        )
        .unwrap();
    let artifact = adopt_artifact(&definitions, artifact);
    let required = &artifact.records().program.modules[artifact.records().program.root.index()];
    assert_eq!(required.path_declarations, vec![path.records().clone()]);
    assert_eq!(required.external_types.len(), expected.len());
    for schema in &expected {
        assert!(required.external_types.contains(schema));
    }
    for (encoded, jit) in [(false, false), (true, false), (true, true)] {
        let artifact = if encoded {
            KbcArtifact::from_bytes(&artifact.records().to_bytes(artifact.definitions()).unwrap())
                .unwrap()
        } else {
            artifact.clone()
        };
        let context = ExecutionContext {
            jit_policy: if jit {
                JitPolicy::Enabled
            } else {
                JitPolicy::Disabled
            },
            ..Default::default()
        };
        let mut runtime = engine.runtime(context.clone());
        let ty = runtime
            .runtime()
            .types()
            .get_by_name("left::Item")
            .unwrap()
            .id;
        runtime
            .runtime_mut()
            .register_host_root(HostObjectId(7), ty, HostSchemaEpoch::new(7))
            .unwrap();
        trace.lock().unwrap().clear();
        let unbound_engine = KagariEngine::with_native_modules(
            Default::default(),
            vec![providers[0].clone(), providers[1].clone(), unbound.clone()],
        )
        .unwrap();
        assert!(
            unbound_engine
                .runtime(context.clone())
                .load_program(
                    &PreparedProgram::from_artifact(
                        artifact.clone(),
                        &Default::default(),
                        &Default::default()
                    )
                    .unwrap(),
                    Default::default()
                )
                .is_err()
        );
        assert!(trace.lock().unwrap().is_empty());
        let loaded_program =
            PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default())
                .unwrap();
        let loaded = runtime
            .load_program(&loaded_program, Default::default())
            .unwrap();
        let report = if jit {
            let mut backend = CraneliftBackend::for_host().unwrap();
            let prepared = runtime
                .prepare_native(
                    &loaded_program,
                    &loaded,
                    ProgramEntry::Main,
                    &mut backend,
                    &context.cancellation,
                )
                .unwrap();
            runtime.execute_prepared(&loaded, ProgramEntry::Main, &[], &context, &prepared)
        } else {
            runtime.execute(&loaded, ProgramEntry::Main, &[], &context)
        }
        .unwrap();
        assert_eq!(
            report
                .return_value
                .value(runtime.runtime().gc())
                .expect("retained execution result"),
            Value::I32(42)
        );
        assert_eq!(*trace.lock().unwrap(), ["root", "read"]);
        drop(report);
        assert_eq!(runtime.runtime().gc().active_roots(), 0);
    }
}

#[test]
fn source_host_index_paths_capture_index_before_rhs_across_backends() {
    assert_source_index_path(false);
}

#[test]
fn source_host_field_index_paths_skip_intermediate_reads() {
    assert_source_index_path(true);
}

#[test]
fn source_multi_index_virtual_and_trailing_field_use_one_host_path() {
    let catalog = kagari_stdlib::declarations::StandardDeclarations::default()
        .catalog()
        .unwrap();
    let mut builder = ModuleBuilder::new("game::objects", &catalog).unwrap();
    let policy = HostTypePolicy {
        ownership: HostTypeOwnership::HostRoot,
        reflection: HostReflectionPolicy::Metadata,
        path_access: PathAccess::ReadWrite,
    };
    let mut player = builder.define_type("Player").unwrap();
    player.external(policy).unwrap();
    let player = player.finish().unwrap();
    let mut child = builder.define_type("Child").unwrap();
    child.external(policy).unwrap();
    child
        .field(
            DefinitionMetadata::checked(
                player.definitions().clone(),
                FieldDef {
                    name: "score".into(),
                    ty: Ty::Builtin(BuiltinType::I32),
                    visibility: Visibility::Public,
                    mutable: true,
                    host_policy: Some(HostFieldPolicy {
                        readable: true,
                        path_access: PathAccess::ReadWrite,
                    }),
                },
                &Default::default(),
            )
            .unwrap(),
        )
        .unwrap();
    let child = child.finish().unwrap();
    let definitions = child.definitions().clone();
    let score = definitions
        .lookup_child(child.id(), DefinitionKind::Field, "score", 0)
        .unwrap();
    let path = builder
        .define_path(
            DefinitionMetadata::checked(
                definitions,
                HostPathDeclaration {
                    root: player.id(),
                    segments: vec![
                        HostPathSegmentDeclaration::Index(HostIndexSegmentDeclaration {
                            slot: 1,
                            collection: Ty::Host(player.id()),
                            index: Ty::Builtin(BuiltinType::I32),
                            result: Ty::Host(child.id()),
                            access: PathAccess::ReadWrite,
                        }),
                        HostPathSegmentDeclaration::Index(HostIndexSegmentDeclaration {
                            slot: 0,
                            collection: Ty::Host(child.id()),
                            index: Ty::Builtin(BuiltinType::I32),
                            result: Ty::Host(child.id()),
                            access: PathAccess::ReadWrite,
                        }),
                        HostPathSegmentDeclaration::Virtual(HostVirtualSegmentDeclaration {
                            name: "selected".into(),
                            result: Ty::Host(child.id()),
                            access: PathAccess::ReadWrite,
                        }),
                        HostPathSegmentDeclaration::Field(score),
                    ],
                    access: PathAccess::ReadWrite,
                    schema_epoch: 0,
                },
                &Default::default(),
            )
            .unwrap(),
        )
        .unwrap();
    let trace = Arc::new(Mutex::new(Vec::new()));
    let state = Arc::new(AtomicI32::new(10));
    let make = builder
        .define_function(
            FunctionDecl::new("make")
                .returns(player.apply([]).unwrap())
                .host_policy(HostCallPolicy {
                    passing: vec![],
                    effects: HostFunctionEffects {
                        may_allocate: true,
                        ..Default::default()
                    },
                }),
        )
        .unwrap();
    let calls = trace.clone();
    builder
        .bind(make, move |cx: &HostCallContext<'_>, _: &[Value]| {
            calls.lock().unwrap().push("make");
            let root = cx.runtime().host().root(HostObjectId(1)).unwrap();
            cx.runtime()
                .gc()
                .alloc_host_root(root)
                .map_err(|error| HostError::new(error.to_string()))
        })
        .unwrap();
    for (name, result) in [("first", 1), ("second", 2), ("rhs", 3)] {
        let function = builder
            .define_function(FunctionDecl::new(name).returns(Type::i32()).host_policy(
                HostCallPolicy {
                    passing: vec![],
                    effects: Default::default(),
                },
            ))
            .unwrap();
        let calls = trace.clone();
        builder
            .bind(function, move |_: &HostCallContext<'_>, _: &[Value]| {
                calls.lock().unwrap().push(name);
                Ok(Value::I32(result))
            })
            .unwrap();
    }
    let (reads, value) = (trace.clone(), state.clone());
    let (writes, target) = (trace.clone(), state.clone());
    builder
        .bind_path(
            &path,
            HostPathAdapter::new()
                .with_read(move |cx, path| {
                    cx.runtime().collect_garbage().unwrap();
                    let values = path
                        .dynamic_args
                        .as_slice()
                        .iter()
                        .map(|arg| arg.value)
                        .collect::<Vec<_>>();
                    if values != [Value::I32(2), Value::I32(1)] {
                        return Err(HostError::new("unexpected indexes"));
                    }
                    reads.lock().unwrap().push("read");
                    Ok(Value::I32(value.load(Ordering::SeqCst)))
                })
                .with_prepare_write(move |cx, _, record| {
                    cx.runtime().collect_garbage().unwrap();
                    writes.lock().unwrap().push("prepare");
                    let Value::I32(next) = record.new_value else {
                        return Err(HostError::new("expected i32"));
                    };
                    let (writes, target) = (writes.clone(), target.clone());
                    Ok(PreparedHostPathWrite::new(move || {
                        writes.lock().unwrap().push("commit");
                        target.store(next, Ordering::SeqCst);
                    }))
                }),
        )
        .unwrap();
    let module = builder.finish().unwrap();
    let engine = KagariEngine::with_native_modules(Default::default(), vec![module]).unwrap();
    let artifact = engine.compile_to_artifact(SourceFile::new("multi-index.kgr",
        "use game::objects as api; fn main() -> i32 { var player = api::make(); player[api::first()][api::second()].selected.score += api::rhs(); player[1][2].selected.score }"), Default::default()).unwrap();
    let root_module = &artifact.records().program.modules[artifact.records().program.root.index()];
    assert_eq!(root_module.path_declarations.len(), 1);
    assert_eq!(root_module.external_types.len(), 2);
    for (encoded, jit) in [(false, false), (true, false), (true, true)] {
        state.store(10, Ordering::SeqCst);
        trace.lock().unwrap().clear();
        let artifact = if encoded {
            KbcArtifact::from_bytes(&artifact.records().to_bytes(artifact.definitions()).unwrap())
                .unwrap()
        } else {
            artifact.clone()
        };
        let context = ExecutionContext {
            jit_policy: if jit {
                JitPolicy::Enabled
            } else {
                JitPolicy::Disabled
            },
            ..Default::default()
        };
        let mut runtime = engine.runtime(context.clone());
        let player = runtime
            .runtime()
            .types()
            .get_by_name("game::objects::Player")
            .unwrap()
            .id;
        runtime
            .runtime_mut()
            .register_host_root(HostObjectId(1), player, HostSchemaEpoch::new(0))
            .unwrap();
        assert_eq!(runtime.runtime().host().path_descriptors().count(), 1);
        let loaded_program =
            PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default())
                .unwrap();
        let loaded = runtime
            .load_program(&loaded_program, Default::default())
            .unwrap();
        let report = if jit {
            let mut backend = CraneliftBackend::for_host().unwrap();
            let prepared = runtime
                .prepare_native(
                    &loaded_program,
                    &loaded,
                    ProgramEntry::Main,
                    &mut backend,
                    &context.cancellation,
                )
                .unwrap();
            runtime.execute_prepared(&loaded, ProgramEntry::Main, &[], &context, &prepared)
        } else {
            runtime.execute(&loaded, ProgramEntry::Main, &[], &context)
        }
        .unwrap();
        assert_eq!(
            report.return_value.value(runtime.runtime().gc()).unwrap(),
            Value::I32(13)
        );
        assert_eq!(state.load(Ordering::SeqCst), 13);
        assert_eq!(
            *trace.lock().unwrap(),
            [
                "make", "first", "second", "rhs", "read", "prepare", "commit", "read"
            ]
        );
        let dirty = runtime.runtime().host_dirty_paths();
        assert_eq!(dirty.len(), 1);
        assert_eq!(dirty[0].old_value, Some(Value::I32(10)));
        assert_eq!(dirty[0].new_value, Value::I32(13));
    }
}

#[test]
fn source_host_writes_commit_after_rhs_and_preserve_completed_rhs_effects_on_failure() {
    for rebind in [false, true] {
        for (op, rhs_value, after_rhs, deleted, expected) in [
            ("=", 2, 100, false, Some(2)),
            ("+=", 2, 100, false, Some(102)),
            ("/=", 0, 100, false, None),
            ("+=", 1, i32::MAX, false, None),
            ("+=", 2, 100, true, None),
        ] {
            let text = if rebind {
                format!(
                    "use left as api; fn main() {{ var target = api::make(); target.related.count {op} if true {{ target = api::make(); api::rhs() }} else {{ 0 }}; }}"
                )
            } else {
                format!(
                    "use left as api; fn main() {{ api::make().related.count {op} api::rhs(); }}"
                )
            };
            let mut schema = NominalSchema::with_count(PathAccess::ReadWrite);
            let path = schema.count_path(PathAccess::ReadWrite, 0);
            let trace = Arc::new(Mutex::new(Vec::new()));
            let state = Arc::new(AtomicI32::new(10));
            let removed = Arc::new(AtomicBool::new(false));
            let root_calls = Arc::new(AtomicUsize::new(0));
            let make = schema
                .builder
                .define_function(
                    FunctionDecl::new("make")
                        .returns(schema.item.apply([]).unwrap())
                        .host_policy(HostCallPolicy {
                            passing: vec![],
                            effects: HostFunctionEffects {
                                may_allocate: true,
                                ..Default::default()
                            },
                        }),
                )
                .unwrap();
            let (calls, times) = (trace.clone(), root_calls.clone());
            schema
                .builder
                .bind(make, move |cx: &HostCallContext<'_>, _: &[Value]| {
                    calls.lock().unwrap().push("root");
                    let object = if times.fetch_add(1, Ordering::SeqCst) == 0 {
                        7
                    } else {
                        8
                    };
                    let root = cx.runtime().host().root(HostObjectId(object)).unwrap();
                    Ok(cx.runtime().gc().alloc_host_root(root).unwrap())
                })
                .unwrap();
            let rhs = schema
                .builder
                .define_function(FunctionDecl::new("rhs").returns(Type::i32()).host_policy(
                    HostCallPolicy {
                        passing: vec![],
                        effects: HostFunctionEffects {
                            may_mutate_host_state: true,
                            ..Default::default()
                        },
                    },
                ))
                .unwrap();
            let (calls, value, absent) = (trace.clone(), state.clone(), removed.clone());
            schema
                .builder
                .bind(rhs, move |_: &HostCallContext<'_>, _: &[Value]| {
                    calls.lock().unwrap().push("rhs");
                    value.store(after_rhs, Ordering::SeqCst);
                    absent.store(deleted, Ordering::SeqCst);
                    Ok(Value::I32(rhs_value))
                })
                .unwrap();
            let (reads, value, absent) = (trace.clone(), state.clone(), removed.clone());
            let (writes, target) = (trace.clone(), state.clone());
            schema
                .builder
                .bind_path(
                    &path,
                    HostPathAdapter::new()
                        .with_read(move |cx, path| {
                            assert_eq!(
                                path.root,
                                cx.runtime().host().root(HostObjectId(7)).unwrap()
                            );
                            reads.lock().unwrap().push("read");
                            if absent.load(Ordering::SeqCst) {
                                return Err(HostError::new("target removed by RHS"));
                            }
                            Ok(Value::I32(value.load(Ordering::SeqCst)))
                        })
                        .with_prepare_write(move |context, _, record| {
                            writes.lock().unwrap().push("prepare");
                            context.runtime().collect_garbage().unwrap();
                            let Value::I32(next) = record.new_value else {
                                return Err(HostError::new("expected i32"));
                            };
                            let (writes, target) = (writes.clone(), target.clone());
                            Ok(PreparedHostPathWrite::new(move || {
                                writes.lock().unwrap().push("commit");
                                target.store(next, Ordering::SeqCst);
                            }))
                        }),
                )
                .unwrap();
            let engine = schema.engine();
            let artifact = engine
                .compile_to_artifact(
                    SourceFile::new("write.kgr", text.clone()),
                    Default::default(),
                )
                .unwrap();
            for (encoded, jit) in [(false, false), (true, false), (true, true)] {
                let artifact = if encoded {
                    KbcArtifact::from_bytes(
                        &artifact.records().to_bytes(artifact.definitions()).unwrap(),
                    )
                    .unwrap()
                } else {
                    artifact.clone()
                };
                let context = ExecutionContext {
                    jit_policy: if jit {
                        JitPolicy::Enabled
                    } else {
                        JitPolicy::Disabled
                    },
                    ..Default::default()
                };
                let mut runtime = engine.runtime(context.clone());
                trace.lock().unwrap().clear();
                state.store(10, Ordering::SeqCst);
                removed.store(false, Ordering::SeqCst);
                root_calls.store(0, Ordering::SeqCst);
                let ty = runtime
                    .runtime()
                    .types()
                    .get_by_name("left::Item")
                    .unwrap()
                    .id;
                runtime
                    .runtime_mut()
                    .register_host_root(HostObjectId(7), ty, HostSchemaEpoch::new(0))
                    .unwrap();
                runtime
                    .runtime_mut()
                    .register_host_root(HostObjectId(8), ty, HostSchemaEpoch::new(0))
                    .unwrap();
                let loaded_program = PreparedProgram::from_artifact(
                    artifact,
                    &Default::default(),
                    &Default::default(),
                )
                .unwrap();
                let loaded = runtime
                    .load_program(&loaded_program, Default::default())
                    .unwrap();
                let result = if jit {
                    let mut backend = CraneliftBackend::for_host().unwrap();
                    let prepared = runtime
                        .prepare_native(
                            &loaded_program,
                            &loaded,
                            ProgramEntry::Main,
                            &mut backend,
                            &context.cancellation,
                        )
                        .unwrap();
                    runtime.execute_prepared(&loaded, ProgramEntry::Main, &[], &context, &prepared)
                } else {
                    runtime.execute(&loaded, ProgramEntry::Main, &[], &context)
                };
                if let Some(expected) = expected {
                    result.unwrap();
                    assert_eq!(state.load(Ordering::SeqCst), expected);
                    assert_eq!(
                        *trace.lock().unwrap(),
                        if rebind {
                            vec!["root", "root", "rhs", "read", "prepare", "commit"]
                        } else {
                            vec!["root", "rhs", "read", "prepare", "commit"]
                        }
                    );
                    let records = runtime.runtime().host_dirty_paths();
                    assert_eq!(records.len(), 1);
                    assert_eq!(records[0].old_value, Some(Value::I32(after_rhs)));
                    assert_eq!(records[0].new_value, Value::I32(expected));
                } else {
                    assert!(result.is_err());
                    assert_eq!(state.load(Ordering::SeqCst), after_rhs);
                    assert_eq!(
                        *trace.lock().unwrap(),
                        if rebind {
                            vec!["root", "root", "rhs", "read"]
                        } else {
                            vec!["root", "rhs", "read"]
                        }
                    );
                    assert!(runtime.runtime().host_dirty_paths().is_empty());
                }
                assert_eq!(runtime.runtime().gc().active_roots(), 0);
            }
        }
    }
}
