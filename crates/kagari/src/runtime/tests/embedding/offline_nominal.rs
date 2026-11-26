#[cfg(feature = "native")]
use crate::program::native::NativeCompiler;
use crate::test_support::standard::standard_library;
use crate::{
    context::{ExecutionContext, JitPolicy},
    engine::{KagariEngine, source::ArtifactOptions},
    error::EmbeddingError,
    program::Compilation,
};
use kagari_bytecode::{
    artifact::KbcArtifact,
    program::{identity, verify_program},
};
use kagari_codegen_cranelift::CraneliftBackend;
use kagari_common::identity::{
    DefinitionKind, DefinitionPath, DefinitionPathSegment, ModuleIdentity, PackageId,
    map::DefinitionContext, metadata::DefinitionMetadata, scoped::DefinitionReferences,
};
use kagari_runtime::{
    host::{HostError, HostObjectId, HostPathAdapter, HostSchemaEpoch, PreparedHostPathWrite},
    module::entry::ProgramEntry,
    native::{
        builder::ModuleBuilder,
        declarations::{FunctionDecl, MethodDecl},
        host,
        types::{FunctionRef, Type, TypeRef},
    },
    value::view::ValueView,
};
use kagari_source::{source::SourceFile, source_database::SourceLayer};

use kagari_types::{
    declaration::{
        FieldDef,
        host::{
            HostCallPolicy, HostFieldPolicy, HostFunctionEffects, HostPassingStyle,
            HostReflectionPolicy, HostTypeOwnership, HostTypePolicy, PathAccess,
        },
        path::{
            HostIndexSegmentDeclaration, HostPathDeclaration, HostPathSegmentDeclaration,
            NativePathOperationKind,
        },
    },
    scalar::BuiltinType,
    ty::Ty,
    visibility::Visibility,
};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicI32, Ordering},
};

fn score_methods(
    builder: &mut ModuleBuilder,
    player: &TypeRef,
    indices: &[&str],
    kinds: &[NativePathOperationKind],
) -> Vec<(FunctionRef, NativePathOperationKind)> {
    builder
        .implement(player.clone(), |group| {
            group.inherent_impl(|methods| {
                kinds
                    .iter()
                    .map(|&kind| {
                        let (name, result) = match kind {
                            NativePathOperationKind::Read => ("read_score", Type::i32()),
                            NativePathOperationKind::Add => ("add_score", Type::i32()),
                            NativePathOperationKind::Set => ("set_score", Type::unit()),
                        };
                        let write = kind != NativePathOperationKind::Read;
                        let mut method = MethodDecl::instance(name);
                        for index in indices {
                            method = method.parameter(*index, Type::i32());
                        }
                        if write {
                            method = method.parameter("rhs", Type::i32());
                        }
                        let method = method.returns(result).host_policy(HostCallPolicy {
                            passing: vec![
                                HostPassingStyle::Owned;
                                1 + indices.len() + usize::from(write)
                            ],
                            effects: HostFunctionEffects {
                                may_call_host_services: true,
                                may_mutate_host_state: write,
                                may_allocate: true,
                                may_trap: true,
                                ..Default::default()
                            },
                        });
                        Ok((methods.define_method(method)?, kind))
                    })
                    .collect()
            })
        })
        .unwrap()
}

fn assert_source_index_path(field_prefix: bool) {
    let catalog = standard_library().declarations().catalog();
    let mut builder = ModuleBuilder::new("game", catalog).unwrap();
    let mut player = builder.define_type("Player").unwrap();
    player
        .external(HostTypePolicy {
            ownership: HostTypeOwnership::HostRoot,
            reflection: HostReflectionPolicy::Hidden,
            path_access: PathAccess::ReadWrite,
        })
        .unwrap();
    let array = Ty::Array(Box::new(Ty::Builtin(BuiltinType::I32)));
    if field_prefix {
        player
            .field(
                DefinitionMetadata::checked(
                    DefinitionContext::new().unwrap().snapshot(),
                    FieldDef {
                        name: "scores".into(),
                        ty: array.clone(),
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
    }
    let player = player.finish().unwrap();
    let mut segments = Vec::new();
    if field_prefix {
        segments.push(HostPathSegmentDeclaration::Field(
            player
                .definitions()
                .lookup_child(player.id(), DefinitionKind::Field, "scores", 0)
                .unwrap(),
        ));
    }
    segments.push(HostPathSegmentDeclaration::Index(
        HostIndexSegmentDeclaration {
            slot: 0,
            collection: if field_prefix {
                array
            } else {
                Ty::Host(player.id())
            },
            index: Ty::Builtin(BuiltinType::I32),
            result: Ty::Builtin(BuiltinType::I32),
            access: PathAccess::ReadWrite,
        },
    ));
    let path = builder
        .define_path(
            DefinitionMetadata::checked(
                player.definitions().clone(),
                HostPathDeclaration {
                    root: player.id(),
                    segments,
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
                let root = cx.runtime_mut().host_root_value(root).unwrap();
                cx.import(&root)
                    .map_err(|error| HostError::new(error.to_string()))
            }),
        )
        .unwrap();
    for (name, result) in [("index", 1), ("rhs", 2)] {
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
    builder
        .bind_path(
            &path,
            HostPathAdapter::new()
                .with_read(move |_, path| {
                    if path.dynamic_args.as_slice()[0].value.as_i32() != Some(1) {
                        return Err(HostError::new("unexpected index"));
                    }
                    reads.lock().unwrap().push("read");
                    Ok(ValueView::from(value.load(Ordering::SeqCst)))
                })
                .with_prepare_write(move |_, _, record| {
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
                }),
        )
        .unwrap();
    for (method, kind) in score_methods(
        &mut builder,
        &player,
        &["index"],
        &[NativePathOperationKind::Read, NativePathOperationKind::Add],
    ) {
        builder.bind_path_operation(method, &path, kind).unwrap();
    }
    let engine = KagariEngine::with_native_modules(
        Default::default(),
        standard_library()
            .modules()
            .unwrap()
            .into_iter()
            .chain(vec![builder.finish().unwrap()])
            .collect(),
    )
    .unwrap();
    let source =
        include_str!("../../../../tests/fixtures/embedding/host/index-score-operations.kgr");
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new("host-index.kgr", source),
            Default::default(),
        )
        .unwrap();
    let error = engine
        .compile_to_artifact(
            SourceFile::new(
                "invalid-host-index.kgr",
                "use game as api; fn main() -> i32 { api::make().read_score(true) }",
            ),
            Default::default(),
        )
        .unwrap_err();
    let EmbeddingError::Diagnostics { diagnostics } = error else {
        panic!("expected an index argument diagnostic, got {error:?}");
    };
    assert!(
        diagnostics
            .iter()
            .any(|item| item.code == "KG_TYPE_ARGUMENT_TYPE_MISMATCH"),
        "{diagnostics:?}"
    );
    for (encoded, jit) in [(false, false), (true, false), (true, true)] {
        trace.lock().unwrap().clear();
        state.store(10, Ordering::SeqCst);
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
        let ty = runtime.types().get_by_name("game::Player").unwrap().id;
        runtime
            .register_host_root(HostObjectId(1), ty, HostSchemaEpoch::new(0))
            .unwrap();
        let program =
            Compilation::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
        let loaded = runtime
            .load_program(program.program(), Default::default())
            .unwrap();
        let report = if jit {
            let mut backend = CraneliftBackend::for_host().unwrap();
            let prepared = NativeCompiler::default()
                .prepare(
                    &program,
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
        assert_eq!(report.return_value.as_i32().unwrap(), 12);
        assert_eq!(state.load(Ordering::SeqCst), 12);
        assert_eq!(
            *trace.lock().unwrap(),
            ["make", "index", "rhs", "read", "prepare", "commit", "read"]
        );
    }
}

mod handles;
mod paths;
mod schema;
mod traits;

fn scoped<T: DefinitionReferences>(
    definitions: &DefinitionContext,
    records: T,
) -> DefinitionMetadata<T> {
    DefinitionMetadata::checked(definitions.snapshot(), records, &Default::default()).unwrap()
}

/// Normalize expected and compiled declarations only where the fixture compares them.
fn adopt_artifact(
    definitions: &DefinitionContext,
    artifact: DefinitionMetadata<KbcArtifact>,
) -> DefinitionMetadata<KbcArtifact> {
    let artifact = artifact
        .import_into(definitions, &Default::default())
        .unwrap()
        .into_records();
    identity::collect(
        &artifact.program.modules.iter().collect::<Vec<_>>(),
        definitions,
        &Default::default(),
    )
    .unwrap();
    verify_program(
        &artifact.program,
        &definitions.snapshot(),
        &Default::default(),
    )
    .unwrap();
    scoped(definitions, artifact)
}
