use super::schema::NominalSchema;
use super::*;
use crate::error::EmbeddingError;
#[cfg(feature = "native")]
use crate::program::native::NativeCompiler;
use crate::test_support::standard::standard_library;
use crate::{context::JitPolicy, program::Compilation};
use kagari_bytecode::{artifact::KbcArtifact, instruction::BinaryOp};
use kagari_common::identity::table::DefinitionTable;
use kagari_runtime::native::types::TypeRef;
use kagari_runtime::{
    host::{HostError, HostPathAdapter, HostPathOperation, PreparedHostPathWrite},
    native::{builder::ModuleBuilder, declarations::FunctionDecl, host, types::Type},
};
use kagari_runtime::{module::entry::ProgramEntry, value::carriers::RootedValue};

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
                context.runtime_mut().collect_garbage().unwrap();
                Ok(ValueView::from(42))
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
        let ty = runtime.types().get_by_name("left::Item").unwrap().id;
        runtime
            .register_host_root(HostObjectId(7), ty, HostSchemaEpoch::new(7))
            .unwrap();
        trace.lock().unwrap().clear();
        let unbound_engine = KagariEngine::with_native_modules(
            Default::default(),
            standard_library()
                .modules()
                .unwrap()
                .into_iter()
                .chain(vec![
                    providers[0].clone(),
                    providers[1].clone(),
                    unbound.clone(),
                ])
                .collect(),
        )
        .unwrap();
        assert!(
            unbound_engine
                .runtime(context.clone())
                .load_program(
                    (Compilation::from_artifact(
                        artifact.clone(),
                        &Default::default(),
                        &Default::default()
                    )
                    .unwrap())
                    .program(),
                    Default::default()
                )
                .is_err()
        );
        assert!(trace.lock().unwrap().is_empty());
        let loaded_program =
            Compilation::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
        let loaded = runtime
            .load_program(loaded_program.program(), Default::default())
            .unwrap();
        runtime.collect_garbage().unwrap();
        let roots = runtime.gc_stats().root_registrations;
        let report = if jit {
            let mut backend = CraneliftBackend::for_host().unwrap();
            let prepared = NativeCompiler::default()
                .prepare(
                    &loaded_program,
                    ProgramEntry::Main,
                    &mut backend,
                    &context.cancellation,
                )
                .and_then(|native_code| (native_code).install(&mut runtime, &loaded))
                .unwrap();
            runtime.execute_prepared(&loaded, ProgramEntry::Main, &[], &context, &prepared)
        } else {
            runtime.execute(&loaded, ProgramEntry::Main, &[], &context)
        }
        .unwrap();
        assert_eq!(report.return_value.as_i32().unwrap(), 42);
        assert_eq!(*trace.lock().unwrap(), ["root", "read"]);
        drop(report);
        assert!(runtime.execution_root().is_none());
        runtime.collect_garbage().unwrap();
        assert_eq!(runtime.gc_stats().root_registrations, roots);
    }
}

fn stats_field(
    definitions: DefinitionTable,
    name: &str,
    ty: Ty,
    access: PathAccess,
) -> DefinitionMetadata<FieldDef> {
    DefinitionMetadata::checked(
        definitions,
        FieldDef {
            name: name.into(),
            ty,
            visibility: Visibility::Public,
            mutable: access == PathAccess::ReadWrite,
            host_policy: Some(HostFieldPolicy {
                readable: true,
                path_access: access,
            }),
        },
        &Default::default(),
    )
    .unwrap()
}

fn stats_schema(hp_access: PathAccess) -> (ModuleBuilder, TypeRef, TypeRef) {
    let mut builder =
        ModuleBuilder::new("game", standard_library().declarations().catalog()).unwrap();
    let policy = HostTypePolicy {
        ownership: HostTypeOwnership::HostRoot,
        reflection: HostReflectionPolicy::Metadata,
        path_access: PathAccess::ReadWrite,
    };
    let mut stats = builder.define_type("Stats").unwrap();
    stats.external(policy).unwrap();
    stats
        .field(stats_field(
            DefinitionContext::new().unwrap().snapshot(),
            "hp",
            Ty::Builtin(BuiltinType::I32),
            hp_access,
        ))
        .unwrap();
    let stats = stats.finish().unwrap();
    let mut player = builder.define_type("Player").unwrap();
    player.external(policy).unwrap();
    for (name, access) in [
        ("baseline", PathAccess::ReadOnly),
        ("current", PathAccess::ReadWrite),
    ] {
        player
            .field(stats_field(
                stats.definitions().clone(),
                name,
                Ty::Host(stats.id()),
                access,
            ))
            .unwrap();
    }
    let player = player.finish().unwrap();
    (builder, player, stats)
}

fn stats_engine(builder: ModuleBuilder) -> KagariEngine {
    let module = builder.finish().unwrap();
    KagariEngine::with_native_modules(
        Default::default(),
        standard_library()
            .modules()
            .unwrap()
            .into_iter()
            .chain([module])
            .collect(),
    )
    .unwrap()
}

#[test]
fn val_baseline_allows_a_committed_write_to_var_hp() {
    let (mut builder, player, stats) = stats_schema(PathAccess::ReadWrite);
    let definitions = player.definitions();
    let segments = [(player.id(), "baseline"), (stats.id(), "hp")].map(|(owner, name)| {
        HostPathSegmentDeclaration::Field(
            definitions
                .lookup_child(owner, DefinitionKind::Field, name, 0)
                .unwrap(),
        )
    });
    let path = builder
        .define_path(
            DefinitionMetadata::checked(
                definitions.clone(),
                HostPathDeclaration {
                    root: player.id(),
                    segments: segments.into(),
                    access: PathAccess::ReadWrite,
                    schema_epoch: 0,
                },
                &Default::default(),
            )
            .unwrap(),
        )
        .unwrap();
    let hp = Arc::new(AtomicI32::new(40));
    let before = hp.clone();
    let read = HostPathAdapter::new()
        .with_read(move |_, _| Ok(ValueView::from(before.load(Ordering::SeqCst))));
    let error = builder.bind_path(&path, read.clone()).unwrap_err();
    assert_eq!(error.code(), "KG_RUNTIME_METADATA_CONFLICT");
    assert!(error.to_string().contains("read/write access"), "{error}");
    let target = hp.clone();
    builder
        .bind_path(
            &path,
            read.with_prepare_write(move |_, _, record| {
                let next = record
                    .new_value
                    .as_i32()
                    .ok_or_else(|| HostError::new("expected hp: i32"))?;
                let target = target.clone();
                Ok(PreparedHostPathWrite::new(move || {
                    target.store(next, Ordering::SeqCst)
                }))
            }),
        )
        .unwrap();
    let make_policy = HostCallPolicy {
        passing: vec![],
        effects: HostFunctionEffects {
            may_allocate: true,
            ..Default::default()
        },
    };
    let make = builder
        .define_function(
            FunctionDecl::new("player")
                .returns(player.apply([]).unwrap())
                .host_policy(make_policy.clone()),
        )
        .unwrap();
    builder
        .bind(
            make,
            host::callback(make_policy, |cx, _| {
                let root = cx.runtime().host().root(HostObjectId(1)).unwrap();
                let value = cx.runtime_mut().host_root_value(root).unwrap();
                cx.import(&value)
                    .map_err(|error| HostError::new(error.to_string()))
            }),
        )
        .unwrap();
    let engine = stats_engine(builder);
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new(
                "write-baseline-hp.kgr",
                include_str!("../../../../../tests/fixtures/embedding/host/write-baseline-hp.kgr"),
            ),
            Default::default(),
        )
        .unwrap();
    let program =
        Compilation::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let ty = runtime.types().get_by_name("game::Player").unwrap().id;
    let root = runtime
        .register_host_root(HostObjectId(1), ty, HostSchemaEpoch::new(0))
        .unwrap();
    let loaded = runtime
        .load_program(program.program(), Default::default())
        .unwrap();
    runtime
        .execute(&loaded, ProgramEntry::Main, &[], &context)
        .unwrap();
    assert_eq!(hp.load(Ordering::SeqCst), 42);
    let records = runtime.host_dirty_paths();
    let [change] = records.as_slice() else {
        panic!("expected one committed HP change")
    };
    assert_eq!(change.root, root);
    assert_eq!(
        change.old_value.as_ref().and_then(RootedValue::as_i32),
        Some(40)
    );
    assert_eq!(change.new_value.as_i32(), Some(42));
}

#[test]
fn val_stats_fields_reject_replacement_at_the_selected_leaf() {
    let (builder, _, _) = stats_schema(PathAccess::ReadOnly);
    let engine = stats_engine(builder);
    let source =
        include_str!("../../../../../tests/fixtures/embedding/host/immutable-stats-fields.kgr");
    let error = engine
        .compile_source(SourceFile::new("immutable-stats-fields.kgr", source))
        .unwrap_err();
    let EmbeddingError::Diagnostics { diagnostics } = error else {
        panic!("expected assignment diagnostics, got {error:?}")
    };
    for (place, field) in [
        ("target.baseline =", "baseline"),
        ("target.current.hp =", "hp"),
        ("target.baseline.hp =", "hp"),
    ] {
        let start = source.find(place).unwrap();
        let end = start + place.trim_end_matches(" =").len();
        assert!(
            diagnostics.iter().any(|d| {
                d.code == "KG_TYPE_INVALID_ASSIGNMENT_TARGET"
                    && d.message.contains(&format!("`val` field `{field}`"))
                    && d.span
                        .is_some_and(|s| start <= s.range.start && s.range.start < end)
            }),
            "missing immutable field diagnostic for {place}: {diagnostics:?}"
        );
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
    let catalog = standard_library().declarations().catalog();
    let mut builder = ModuleBuilder::new("game::objects", catalog).unwrap();
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
    let path = DefinitionMetadata::checked(
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
    .unwrap();
    let trace = Arc::new(Mutex::new(Vec::new()));
    let state = Arc::new(AtomicI32::new(10));
    let make_policy = HostCallPolicy {
        passing: vec![],
        effects: HostFunctionEffects {
            may_allocate: true,
            ..Default::default()
        },
    };
    let make = builder
        .define_function(
            FunctionDecl::new("make")
                .returns(player.apply([]).unwrap())
                .host_policy(make_policy.clone()),
        )
        .unwrap();
    let calls = trace.clone();
    builder
        .bind(
            make,
            host::callback(make_policy, move |cx, _| {
                calls.lock().unwrap().push("make");
                let root = cx.runtime().host().root(HostObjectId(1)).unwrap();
                {
                    let root = cx.runtime_mut().host_root_value(root).unwrap();
                    cx.import(&root)
                }
                .map_err(|error| HostError::new(error.to_string()))
            }),
        )
        .unwrap();
    for (name, result) in [("first", 1), ("second", 2), ("rhs", 3)] {
        let function_policy = HostCallPolicy {
            passing: vec![],
            effects: Default::default(),
        };
        let function = builder
            .define_function(
                FunctionDecl::new(name)
                    .returns(Type::i32())
                    .host_policy(function_policy.clone()),
            )
            .unwrap();
        let calls = trace.clone();
        builder
            .bind(
                function,
                host::callback(function_policy, move |_, _| {
                    calls.lock().unwrap().push(name);
                    Ok(ValueView::from(result))
                }),
            )
            .unwrap();
    }
    let (reads, value) = (trace.clone(), state.clone());
    let (writes, target) = (trace.clone(), state.clone());
    let adapter = HostPathAdapter::new()
        .with_read(move |cx, path| {
            cx.runtime_mut().collect_garbage().unwrap();
            let values = path
                .dynamic_args
                .as_slice()
                .iter()
                .map(|arg| arg.value.as_i32())
                .collect::<Vec<_>>();
            if values != [Some(2), Some(1)] {
                return Err(HostError::new("unexpected indexes"));
            }
            reads.lock().unwrap().push("read");
            Ok(ValueView::from(value.load(Ordering::SeqCst)))
        })
        .with_prepare_write(move |cx, _, record| {
            cx.runtime_mut().collect_garbage().unwrap();
            writes.lock().unwrap().push("prepare");
            let next = record
                .new_value
                .as_i32()
                .ok_or_else(|| HostError::new("expected i32"))?;
            let (writes, target) = (writes.clone(), target.clone());
            Ok(PreparedHostPathWrite::new(move || {
                writes.lock().unwrap().push("commit");
                target.store(next, Ordering::SeqCst);
            }))
        });
    let methods = score_methods(
        &mut builder,
        &player,
        &["first", "second"],
        &[
            NativePathOperationKind::Read,
            NativePathOperationKind::Add,
            NativePathOperationKind::Set,
        ],
    );
    let path = builder.define_path(path).unwrap();
    builder.bind_path(&path, adapter.clone()).unwrap();
    let mut changed = builder.clone();
    let mut other = path.records().clone();
    for segment in &mut other.segments {
        if let HostPathSegmentDeclaration::Index(index) = segment {
            index.slot = 1 - index.slot;
        }
    }
    let other = changed
        .define_path(
            DefinitionMetadata::checked(path.definitions().clone(), other, &Default::default())
                .unwrap(),
        )
        .unwrap();
    changed.bind_path(&other, adapter).unwrap();
    for (method, kind) in methods {
        changed
            .bind_path_operation(method.clone(), &other, kind)
            .unwrap();
        builder.bind_path_operation(method, &path, kind).unwrap();
    }
    let make_engine = |builder: ModuleBuilder| {
        KagariEngine::with_native_modules(
            Default::default(),
            standard_library()
                .modules()
                .unwrap()
                .into_iter()
                .chain([builder.finish().unwrap()])
                .collect(),
        )
        .unwrap()
    };
    let engine = make_engine(builder);
    let source =
        include_str!("../../../../../tests/fixtures/embedding/host/mixed-score-operations.kgr");

    let artifact = engine
        .compile_to_artifact(
            SourceFile::new("multi-index.kgr", source),
            Default::default(),
        )
        .unwrap();
    {
        let decoded =
            KbcArtifact::from_bytes(&artifact.records().to_bytes(artifact.definitions()).unwrap())
                .unwrap();
        let compilation =
            Compilation::from_artifact(decoded, &Default::default(), &Default::default()).unwrap();
        let mut missing = engine.runtime(Default::default());
        let descriptor = missing.resolve_host_path(&path).unwrap();
        missing
            .register_host_path_adapter(descriptor, HostPathAdapter::new())
            .unwrap();
        let error = missing
            .load_program(compilation.program(), Default::default())
            .unwrap_err();
        assert!(error.to_string().contains("read/write access"), "{error}");
        // Both descriptors are present; the same-signature methods select the wrong one.
        let mut wrong = make_engine(changed).runtime(Default::default());
        assert!(wrong.resolve_host_path(&path).is_ok());
        let error = wrong
            .load_program(compilation.program(), Default::default())
            .unwrap_err();
        assert!(
            error.to_string().contains("native binding differs")
                || error.to_string().contains("native declaration differs"),
            "{error}"
        );
        assert!(trace.lock().unwrap().is_empty());
    }
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
            .types()
            .get_by_name("game::objects::Player")
            .unwrap()
            .id;
        let root = runtime
            .register_host_root(HostObjectId(1), player, HostSchemaEpoch::new(0))
            .unwrap();
        assert_eq!(runtime.host().path_descriptors().count(), 1);
        let loaded_program =
            Compilation::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
        let loaded = runtime
            .load_program(loaded_program.program(), Default::default())
            .unwrap();
        let report = if jit {
            let mut backend = CraneliftBackend::for_host().unwrap();
            let prepared = NativeCompiler::default()
                .prepare(
                    &loaded_program,
                    ProgramEntry::Main,
                    &mut backend,
                    &context.cancellation,
                )
                .and_then(|native_code| (native_code).install(&mut runtime, &loaded))
                .unwrap();
            runtime.execute_prepared(&loaded, ProgramEntry::Main, &[], &context, &prepared)
        } else {
            runtime.execute(&loaded, ProgramEntry::Main, &[], &context)
        }
        .unwrap();
        assert_eq!(report.return_value.as_i32().unwrap(), 13);
        assert_eq!(state.load(Ordering::SeqCst), 13);
        assert_eq!(
            *trace.lock().unwrap(),
            [
                "make", "first", "second", "rhs", "read", "prepare", "commit", "read"
            ]
        );
        let dirty = runtime.host_dirty_paths();
        assert_eq!(dirty.len(), 1);
        assert_eq!(dirty[0].root, root);
        assert_eq!(
            dirty[0].descriptor_id,
            runtime.resolve_host_path(&path).unwrap()
        );
        assert_eq!(dirty[0].operation, HostPathOperation::Modify(BinaryOp::Add));
        assert_eq!(
            dirty[0].old_value.as_ref().and_then(RootedValue::as_i32),
            Some(10)
        );
        assert_eq!(dirty[0].new_value.as_i32(), Some(13));
        assert_eq!(
            dirty[0]
                .dynamic_args
                .as_slice()
                .iter()
                .map(|arg| arg.value.as_i32())
                .collect::<Vec<_>>(),
            [Some(2), Some(1)]
        );
        if encoded && !jit {
            runtime
                .execute(&loaded, ProgramEntry::Export("replace"), &[], &context)
                .unwrap();
            assert_eq!(state.load(Ordering::SeqCst), 21);
            let dirty = runtime.host_dirty_paths();
            assert_eq!(dirty.len(), 2);
            assert_eq!(dirty[1].operation, HostPathOperation::Set);
            assert_eq!(
                dirty[1].old_value.as_ref().and_then(RootedValue::as_i32),
                Some(13)
            );
            assert_eq!(dirty[1].new_value.as_i32(), Some(21));
        }
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
            let make_policy = HostCallPolicy {
                passing: vec![],
                effects: HostFunctionEffects {
                    may_allocate: true,
                    ..Default::default()
                },
            };
            let make = schema
                .builder
                .define_function(
                    FunctionDecl::new("make")
                        .returns(schema.item.apply([]).unwrap())
                        .host_policy(make_policy.clone()),
                )
                .unwrap();
            let (calls, times) = (trace.clone(), root_calls.clone());
            schema
                .builder
                .bind(
                    make,
                    host::callback(make_policy, move |cx, _| {
                        calls.lock().unwrap().push("root");
                        let object = if times.fetch_add(1, Ordering::SeqCst) == 0 {
                            7
                        } else {
                            8
                        };
                        let root = cx.runtime().host().root(HostObjectId(object)).unwrap();
                        let root = cx.runtime_mut().host_root_value(root).unwrap();
                        cx.import(&root)
                            .map_err(|error| HostError::new(error.to_string()))
                    }),
                )
                .unwrap();
            let rhs_policy = HostCallPolicy {
                passing: vec![],
                effects: HostFunctionEffects {
                    may_mutate_host_state: true,
                    ..Default::default()
                },
            };
            let rhs = schema
                .builder
                .define_function(
                    FunctionDecl::new("rhs")
                        .returns(Type::i32())
                        .host_policy(rhs_policy.clone()),
                )
                .unwrap();
            let (calls, value, absent) = (trace.clone(), state.clone(), removed.clone());
            schema
                .builder
                .bind(
                    rhs,
                    host::callback(rhs_policy, move |_, _| {
                        calls.lock().unwrap().push("rhs");
                        value.store(after_rhs, Ordering::SeqCst);
                        absent.store(deleted, Ordering::SeqCst);
                        Ok(ValueView::from(rhs_value))
                    }),
                )
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
                            Ok(ValueView::from(value.load(Ordering::SeqCst)))
                        })
                        .with_prepare_write(move |context, _, record| {
                            writes.lock().unwrap().push("prepare");
                            context.runtime_mut().collect_garbage().unwrap();
                            let next = record
                                .new_value
                                .as_i32()
                                .ok_or_else(|| HostError::new("expected i32"))?;
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
                let ty = runtime.types().get_by_name("left::Item").unwrap().id;
                runtime
                    .register_host_root(HostObjectId(7), ty, HostSchemaEpoch::new(0))
                    .unwrap();
                runtime
                    .register_host_root(HostObjectId(8), ty, HostSchemaEpoch::new(0))
                    .unwrap();
                let loaded_program =
                    Compilation::from_artifact(artifact, &Default::default(), &Default::default())
                        .unwrap();
                let loaded = runtime
                    .load_program(loaded_program.program(), Default::default())
                    .unwrap();
                runtime.collect_garbage().unwrap();
                let roots = runtime.gc_stats().root_registrations;
                let result = if jit {
                    let mut backend = CraneliftBackend::for_host().unwrap();
                    let prepared = {
                        let native_code = NativeCompiler::default().prepare(
                            &loaded_program,
                            ProgramEntry::Main,
                            &mut backend,
                            &context.cancellation,
                        );
                        native_code
                            .and_then(|native_code| (native_code).install(&mut runtime, &loaded))
                    }
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
                    let records = runtime.host_dirty_paths();
                    assert_eq!(records.len(), 1);
                    assert_eq!(
                        records[0].old_value.as_ref().and_then(RootedValue::as_i32),
                        Some(after_rhs)
                    );
                    assert_eq!(records[0].new_value.as_i32(), Some(expected));
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
                    assert!(runtime.host_dirty_paths().is_empty());
                }
                assert!(runtime.execution_root().is_none());
                runtime.collect_garbage().unwrap();
                assert_eq!(runtime.gc_stats().root_registrations, roots);
            }
        }
    }
}
