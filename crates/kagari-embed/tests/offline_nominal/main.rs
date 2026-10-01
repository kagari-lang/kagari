use kagari_common::{
    collection::CollectionAccess,
    host_interface::{
        HostFunctionDeclaration, HostInterface, HostParameter, HostPassingStyle,
        type_declaration::{
            HostFieldDeclaration, HostMethodDeclaration, HostTraitImplementationDeclaration,
            HostTraitMethodBinding, HostTypeDeclaration, HostTypeOwnership,
        },
        value_type::HostValueType,
    },
    identity::{DefinitionId, DefinitionKind, DefinitionPathSegment, ModuleIdentity, PackageId},
    source::SourceFile,
    source_database::SourceLayer,
};
use {kagari_bytecode::artifact::KbcArtifact, kagari_embed::context::JitPolicy};

use std::{cell::RefCell, rc::Rc};
use {
    kagari_common::capability::CapabilitySet,
    kagari_runtime::{
        host::{HostFunction, HostObjectId, HostSchemaEpoch, HostTypeRegistration},
        security::LanguageProfile,
        value::Value,
    },
};
use {
    kagari_embed::{
        context::ExecutionContext,
        engine::{
            KagariEngine,
            source::{ArtifactOptions, CompileOptions},
        },
        program::PreparedProgram,
    },
    kagari_runtime::security::HostExposurePolicy,
};

fn interface() -> HostInterface {
    let related = HostTypeDeclaration::new("right.Item");
    let mut item = HostTypeDeclaration::new("left.Item");
    item.ownership = HostTypeOwnership::HostRoot;
    item.path_access = PathAccess::ReadOnly;
    item.fields.push(HostFieldDeclaration::new(
        &item.id,
        "related",
        HostValueType::Opaque(related.id.clone()),
    ));
    let make =
        HostFunctionDeclaration::new("left.make", vec![], HostValueType::Opaque(item.id.clone()));
    let take = HostFunctionDeclaration::new(
        "left.take",
        vec![HostParameter {
            name: "value".into(),
            ty: HostValueType::Opaque(item.id.clone()),
            passing: HostPassingStyle::SharedBorrow,
        }],
        HostValueType::I32,
    );
    HostInterface {
        paths: vec![],
        types: vec![item, related, HostTypeDeclaration::new("unused.Other")],
        functions: vec![make, take],
    }
}

fn assert_source_index_path(field_prefix: bool) {
    use kagari_common::host_interface::{
        path::{HostIndexSegmentDeclaration, HostPathDeclaration, HostPathSegmentDeclaration},
        type_declaration::PathAccess,
    };
    use kagari_runtime::{
        host::{HostError, HostPathAdapter, PreparedHostPathWrite},
        metadata::{AbiFingerprint, TypeKind, TypeRegistration},
    };
    use std::cell::Cell;

    let mut player = HostTypeDeclaration::new("game.Player");
    player.ownership = HostTypeOwnership::HostRoot;
    player.path_access = PathAccess::ReadWrite;
    let collection = if field_prefix {
        let ty = HostValueType::Array(Box::new(HostValueType::I32), CollectionAccess::Mutable);
        let mut field = HostFieldDeclaration::new(&player.id, "scores", ty.clone());
        field.path_access = PathAccess::ReadWrite;
        field.writable = true;
        player.fields.push(field);
        ty
    } else {
        HostValueType::Opaque(player.id.clone())
    };
    let make = HostFunctionDeclaration::new(
        "game.make",
        vec![],
        HostValueType::Opaque(player.id.clone()),
    );
    let index = HostFunctionDeclaration::new("game.index", vec![], HostValueType::I32);
    let rhs = HostFunctionDeclaration::new("game.rhs", vec![], HostValueType::I32);
    let mut segments = Vec::new();
    if field_prefix {
        segments.push(HostPathSegmentDeclaration::Field(
            player.fields[0].id.clone(),
        ));
    }
    segments.push(HostPathSegmentDeclaration::Index(
        HostIndexSegmentDeclaration {
            slot: 0,
            collection,
            index: HostValueType::I32,
            result: HostValueType::I32,
            access: PathAccess::ReadWrite,
        },
    ));
    let path = HostPathDeclaration {
        root: player.id.clone(),
        segments,
        access: PathAccess::ReadWrite,
        schema_epoch: 0,
        capabilities: Default::default(),
    };
    let declarations = HostInterface {
        paths: vec![path.clone()],
        types: vec![player.clone()],
        functions: vec![make.clone(), index.clone(), rhs.clone()],
    };
    let engine = KagariEngine::default();
    engine.set_host_interface(declarations).unwrap();
    let profile = LanguageProfile {
        allow_host_calls: true,
        allow_path_mutation: true,
        allow_jit: true,
        ..Default::default()
    };
    let prefix = if field_prefix { ".scores" } else { "" };
    let source = SourceFile::new(
        "host-index.kgr",
        format!(
            "use game as api; fn main() -> i32 {{ var player = api::make(); player{prefix}[api::index()] += api::rhs(); player{prefix}[1] }}"
        ),
    );
    let artifact = engine
        .compile_to_artifact(
            source,
            CompileOptions {
                language_profile: profile,
            },
            Default::default(),
        )
        .unwrap();
    assert!(
        engine
            .compile_to_artifact(
                SourceFile::new(
                    "invalid-host-index.kgr",
                    format!("use game as api; fn main() -> i32 {{ api::make(){prefix}[true] }}"),
                ),
                CompileOptions {
                    language_profile: profile,
                },
                Default::default(),
            )
            .is_err()
    );
    for (encoded, jit) in [(false, false), (true, false), (true, true)] {
        let artifact = if encoded {
            KbcArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap()
        } else {
            artifact.clone()
        };
        let context = ExecutionContext {
            language_profile: profile,
            capabilities: CapabilitySet {
                host_calls: true,
                path_mutation: true,
                jit: true,
                ..Default::default()
            },
            host_policy: HostExposurePolicy {
                allowed_host_functions: vec![
                    "game.make".into(),
                    "game.index".into(),
                    "game.rhs".into(),
                ],
                allowed_host_types: vec!["game.Player".into()],
                allow_host_path_reads: true,
                allow_host_path_mutation: true,
                ..Default::default()
            },
            jit_policy: if jit {
                JitPolicy::Enabled
            } else {
                JitPolicy::Disabled
            },
            ..Default::default()
        };
        let mut runtime = engine.runtime(context.clone());
        let player_id = runtime
            .register_host_type(HostTypeRegistration::new(player.clone(), "Player"))
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
        let trace = Rc::new(RefCell::new(Vec::new()));
        let state = Rc::new(Cell::new(10));
        let calls = trace.clone();
        runtime
            .register_host_function(HostFunction::new(make.clone(), move |_, _| {
                calls.borrow_mut().push("make");
                Ok(Value::HostRoot(root))
            }))
            .unwrap();
        let calls = trace.clone();
        runtime
            .register_host_function(HostFunction::new(index.clone(), move |_, _| {
                calls.borrow_mut().push("index");
                Ok(Value::I32(1))
            }))
            .unwrap();
        let calls = trace.clone();
        runtime
            .register_host_function(HostFunction::new(rhs.clone(), move |_, _| {
                calls.borrow_mut().push("rhs");
                Ok(Value::I32(2))
            }))
            .unwrap();
        let descriptor = runtime.runtime_mut().register_host_path(&path).unwrap();
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
                        reads.borrow_mut().push("read");
                        Ok(Value::I32(value.get()))
                    })
                    .with_prepare_write(move |_, _, record| {
                        writes.borrow_mut().push("prepare");
                        let Value::I32(next) = record.new_value else {
                            return Err(HostError::new("expected i32"));
                        };
                        let (writes, target) = (writes.clone(), target.clone());
                        Ok(PreparedHostPathWrite::new(move || {
                            writes.borrow_mut().push("commit");
                            target.set(next);
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
            let mut backend = kagari_codegen_cranelift::CraneliftBackend::for_host().unwrap();
            let prepared = runtime
                .prepare_native(
                    &loaded_program,
                    &loaded,
                    "main",
                    &mut backend,
                    &context.cancellation,
                )
                .unwrap();
            runtime.execute_prepared(&loaded, "main", &[], &context, &prepared)
        } else {
            runtime.execute(&loaded, "main", &[], &context)
        }
        .unwrap();
        assert_eq!(report.return_value, Value::I32(12));
        assert_eq!(state.get(), 12);
        assert_eq!(
            *trace.borrow(),
            ["make", "index", "rhs", "read", "prepare", "commit", "read"]
        );
    }
}

mod handles;
mod paths;
mod traits;
