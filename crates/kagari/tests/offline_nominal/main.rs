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
        HostError, HostFunction, HostObjectId, HostPathAdapter, HostSchemaEpoch,
        HostTypeRegistration, PreparedHostPathWrite,
    },
    metadata::{AbiFingerprint, TypeKind, TypeRegistration},
    module::entry::ProgramEntry,
    value::Value,
};
use kagari_source::{source::SourceFile, source_database::SourceLayer};
use kagari_types::{
    declaration::host::{HostPassingStyle, HostTypeOwnership, PathAccess},
    host_interface::{
        HostFunctionDeclaration, HostInterface, HostParameter,
        path::{HostIndexSegmentDeclaration, HostPathDeclaration, HostPathSegmentDeclaration},
        type_declaration::{
            HostFieldDeclaration, HostMethodDeclaration, HostTraitImplementationDeclaration,
            HostTraitMethodBinding, HostTypeDeclaration,
        },
        value_type::HostValueType,
    },
    ty::Ty,
};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicI32, Ordering},
};

fn interface(definitions: &DefinitionContext) -> HostInterface {
    let related = HostTypeDeclaration::new(definitions, "right.Item").unwrap();
    let mut item = HostTypeDeclaration::new(definitions, "left.Item").unwrap();
    item.ownership = HostTypeOwnership::HostRoot;
    item.path_access = PathAccess::ReadOnly;
    item.fields.push(
        HostFieldDeclaration::new(
            definitions,
            item.id,
            "related",
            HostValueType::Opaque(related.id),
        )
        .unwrap(),
    );
    let make = HostFunctionDeclaration::new(
        definitions,
        "left.make",
        vec![],
        HostValueType::Opaque(item.id),
    )
    .unwrap();
    let take = HostFunctionDeclaration::new(
        definitions,
        "left.take",
        vec![HostParameter {
            name: "value".into(),
            ty: HostValueType::Opaque(item.id),
            passing: HostPassingStyle::SharedBorrow,
        }],
        HostValueType::I32,
    )
    .unwrap();
    HostInterface {
        paths: vec![],
        types: vec![
            item,
            related,
            HostTypeDeclaration::new(definitions, "unused.Other").unwrap(),
        ],
        functions: vec![make, take],
    }
}

fn assert_source_index_path(field_prefix: bool) {
    let definitions = DefinitionContext::new().unwrap();
    let mut player = HostTypeDeclaration::new(&definitions, "game.Player").unwrap();
    player.ownership = HostTypeOwnership::HostRoot;
    player.path_access = PathAccess::ReadWrite;
    let collection = if field_prefix {
        let ty = HostValueType::Array(Box::new(HostValueType::I32));
        let mut field =
            HostFieldDeclaration::new(&definitions, player.id, "scores", ty.clone()).unwrap();
        field.path_access = PathAccess::ReadWrite;
        field.writable = true;
        player.fields.push(field);
        ty
    } else {
        HostValueType::Opaque(player.id)
    };
    let make = HostFunctionDeclaration::new(
        &definitions,
        "game.make",
        vec![],
        HostValueType::Opaque(player.id),
    )
    .unwrap();
    let index =
        HostFunctionDeclaration::new(&definitions, "game.index", vec![], HostValueType::I32)
            .unwrap();
    let rhs =
        HostFunctionDeclaration::new(&definitions, "game.rhs", vec![], HostValueType::I32).unwrap();
    let mut segments = Vec::new();
    if field_prefix {
        segments.push(HostPathSegmentDeclaration::Field(player.fields[0].id));
    }
    segments.push(HostPathSegmentDeclaration::Index(
        HostIndexSegmentDeclaration {
            slot: 0,
            collection: Ty::from_host_type(&collection),
            index: Ty::from_host_type(&HostValueType::I32),
            result: Ty::from_host_type(&HostValueType::I32),
            access: PathAccess::ReadWrite,
        },
    ));
    let path = HostPathDeclaration {
        root: player.id,
        segments,
        access: PathAccess::ReadWrite,
        schema_epoch: 0,
    };
    let declarations = HostInterface {
        paths: vec![path.clone()],
        types: vec![player.clone()],
        functions: vec![make.clone(), index.clone(), rhs.clone()],
    };
    let engine = KagariEngine::default();
    engine
        .set_host_interface(scoped(&definitions, declarations))
        .unwrap();

    let prefix = if field_prefix { ".scores" } else { "" };
    let source = SourceFile::new(
        "host-index.kgr",
        format!(
            "use game as api; fn main() -> i32 {{ var player = api::make(); player{prefix}[api::index()] += api::rhs(); player{prefix}[1] }}"
        ),
    );
    let artifact = engine
        .compile_to_artifact(source, Default::default())
        .unwrap();
    assert!(
        engine
            .compile_to_artifact(
                SourceFile::new(
                    "invalid-host-index.kgr",
                    format!("use game as api; fn main() -> i32 {{ api::make(){prefix}[true] }}"),
                ),
                Default::default(),
            )
            .is_err()
    );
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
        let player_id = runtime
            .register_host_type(HostTypeRegistration::new(
                scoped(&definitions, player.clone()),
                "Player",
            ))
            .unwrap();
        if runtime.runtime().types().id_by_name("i32").is_none() {
            runtime
                .runtime()
                .types()
                .register(TypeRegistration {
                    abi_fingerprint: AbiFingerprint(10),
                    ..TypeRegistration::new("i32", TypeKind::Primitive)
                })
                .unwrap();
        }
        let root = runtime
            .runtime_mut()
            .register_host_root(HostObjectId(1), player_id, HostSchemaEpoch::new(0))
            .unwrap();
        let trace = Arc::new(Mutex::new(Vec::new()));
        let state = Arc::new(AtomicI32::new(10));
        let calls = trace.clone();
        runtime
            .register_host_function(HostFunction::new(
                scoped(&definitions, make.clone()),
                move |cx, _| {
                    calls.lock().unwrap().push("make");
                    Ok(cx.runtime().gc().alloc_host_root(root).unwrap())
                },
            ))
            .unwrap();
        let calls = trace.clone();
        runtime
            .register_host_function(HostFunction::new(
                scoped(&definitions, index.clone()),
                move |_, _| {
                    calls.lock().unwrap().push("index");
                    Ok(Value::I32(1))
                },
            ))
            .unwrap();
        let calls = trace.clone();
        runtime
            .register_host_function(HostFunction::new(
                scoped(&definitions, rhs.clone()),
                move |_, _| {
                    calls.lock().unwrap().push("rhs");
                    Ok(Value::I32(2))
                },
            ))
            .unwrap();
        let descriptor = runtime
            .runtime_mut()
            .register_host_path(&scoped(&definitions, path.clone()))
            .unwrap();
        let (reads, value) = (trace.clone(), state.clone());
        let (writes, target) = (trace.clone(), state.clone());
        runtime
            .runtime_mut()
            .register_host_path_adapter(
                descriptor,
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
