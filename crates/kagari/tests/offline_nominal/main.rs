use kagari::{
    context::{ExecutionContext, JitPolicy},
    engine::{KagariEngine, source::ArtifactOptions},
    program::PreparedProgram,
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
    host::{
        HostCallContext, HostError, HostObjectId, HostPathAdapter, HostSchemaEpoch,
        PreparedHostPathWrite,
    },
    module::entry::ProgramEntry,
    native::{builder::ModuleBuilder, declarations::FunctionDecl, types::Type},
    value::Value,
};
use kagari_source::{source::SourceFile, source_database::SourceLayer};

use kagari_types::{
    declaration::{
        FieldDef,
        host::{
            HostCallPolicy, HostFieldPolicy, HostFunctionEffects, HostPassingStyle,
            HostReflectionPolicy, HostTypeOwnership, HostTypePolicy, PathAccess,
        },
        path::{HostIndexSegmentDeclaration, HostPathDeclaration, HostPathSegmentDeclaration},
    },
    scalar::BuiltinType,
    ty::Ty,
    visibility::Visibility,
};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicI32, Ordering},
};

fn assert_source_index_path(field_prefix: bool) {
    let catalog = kagari_stdlib::declarations::StandardDeclarations::default()
        .catalog()
        .unwrap();
    let mut builder = ModuleBuilder::new("game", &catalog).unwrap();
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
            Ok(cx.runtime().gc().alloc_host_root(root).unwrap())
        })
        .unwrap();
    for (name, result) in [("index", 1), ("rhs", 2)] {
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
                .with_read(move |_, path| {
                    if path.dynamic_args.as_slice()[0].value != Value::I32(1) {
                        return Err(HostError::new("unexpected index"));
                    }
                    reads.lock().unwrap().push("read");
                    Ok(Value::I32(value.load(Ordering::SeqCst)))
                })
                .with_prepare_write(move |_, _, record| {
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
    let engine =
        KagariEngine::with_native_modules(Default::default(), vec![builder.finish().unwrap()])
            .unwrap();
    let prefix = if field_prefix { ".scores" } else { "" };
    let artifact = engine.compile_to_artifact(SourceFile::new("host-index.kgr", format!(
        "use game as api; fn main() -> i32 {{ var player = api::make(); player{prefix}[api::index()] += api::rhs(); player{prefix}[1] }}"
    )), Default::default()).unwrap();
    assert!(
        engine
            .compile_to_artifact(
                SourceFile::new(
                    "invalid-host-index.kgr",
                    format!("use game as api; fn main() -> i32 {{ api::make(){prefix}[true] }}")
                ),
                Default::default()
            )
            .is_err()
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
        let ty = runtime
            .runtime()
            .types()
            .get_by_name("game::Player")
            .unwrap()
            .id;
        runtime
            .runtime_mut()
            .register_host_root(HostObjectId(1), ty, HostSchemaEpoch::new(0))
            .unwrap();
        let program =
            PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default())
                .unwrap();
        let loaded = runtime.load_program(&program, Default::default()).unwrap();
        let report = if jit {
            let mut backend = CraneliftBackend::for_host().unwrap();
            let prepared = runtime
                .prepare_native(
                    &program,
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
            Value::I32(12)
        );
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
    )
    .unwrap();
    verify_program(&artifact.program, &definitions.snapshot()).unwrap();
    scoped(definitions, artifact)
}
