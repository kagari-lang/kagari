//! Compile against common native declarations without callbacks or a runtime.
//! Run with `cargo run -p kagari --example offline_compile`.
use kagari::BytecodeArtifact;
use kagari::engine::KagariEngine;
use kagari_analyzer::AnalyzerQueries;
use kagari_bytecode::{
    artifact::{ArtifactBuildOptions, ArtifactSectionId, KBC_ARTIFACT_FORMAT_VERSION, KbcArtifact},
    native_input::PortableMir,
};
use kagari_common::identity::{
    DefinitionKind, ModuleIdentity, PackageId, map::DefinitionContext, metadata::DefinitionMetadata,
};
use kagari_compiler::{bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir};
use kagari_hir::compile_input::program::CheckedCompilation;
use kagari_hir::{analysis::AnalysisDatabase, declarations::DeclarationId};
use kagari_mir::codec::encode_program;
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use kagari_stdlib::StandardLibrary;
use kagari_types::{
    callable::{CallableImplementation, MethodPolicy},
    declaration::{
        FieldDef, FnDecl, Param, TypeDeclaration, TypeDef, TypeDefKind,
        host::{
            HostCallPolicy, HostFieldPolicy, HostFunctionEffects, HostPassingStyle,
            HostReflectionPolicy, HostTypeOwnership, HostTypePolicy, PathAccess,
        },
        module::{ImplDecl, ModuleDecl},
        path::{
            HostIndexSegmentDeclaration, HostPathDeclaration, HostPathSegmentDeclaration,
            HostVirtualSegmentDeclaration, NativePathOperation,
            NativePathOperationKind::{Add, Read, Set},
        },
    },
    scalar::BuiltinType,
    ty::Ty,
    visibility::Visibility,
};
use std::{error::Error, sync::Arc};

fn field(name: &str, ty: Ty, access: PathAccess) -> FieldDef {
    FieldDef {
        name: name.into(),
        ty,
        visibility: Visibility::Public,
        mutable: access == PathAccess::ReadWrite,
        host_policy: Some(HostFieldPolicy {
            readable: true,
            path_access: access,
        }),
    }
}

fn declarations() -> Result<DefinitionMetadata<ModuleDecl>, Box<dyn Error>> {
    let definitions = DefinitionContext::new()?;
    let mut module = ModuleDecl::new(ModuleIdentity {
        package: PackageId("demo".into()),
        path: vec![],
    });
    let root = definitions.intern_root(&module.identity)?;
    let player = definitions.intern_child(root, DefinitionKind::Struct, "Player", 0)?;
    let child = definitions.intern_child(root, DefinitionKind::Struct, "Child", 0)?;
    let score = definitions.intern_child(player, DefinitionKind::Field, "score", 0)?;
    let scores = definitions.intern_child(player, DefinitionKind::Field, "scores", 0)?;
    let child_score = definitions.intern_child(child, DefinitionKind::Field, "score", 0)?;
    let array = Ty::Array(Box::new(Ty::Builtin(BuiltinType::I32)));
    let policy = HostTypePolicy {
        ownership: HostTypeOwnership::HostRoot,
        reflection: HostReflectionPolicy::Hidden,
        path_access: PathAccess::ReadWrite,
    };
    module.types = vec![
        TypeDef {
            name: "Player".into(),
            kind: TypeDefKind::Host(policy),
            generic_params: vec![],
            bounds: vec![],
            variants: vec![],
            fields: vec![
                field(
                    "score",
                    Ty::Builtin(BuiltinType::I32),
                    PathAccess::ReadWrite,
                ),
                field("scores", array.clone(), PathAccess::ReadOnly),
            ],
        },
        TypeDef {
            name: "Child".into(),
            kind: TypeDefKind::Host(HostTypePolicy {
                ownership: HostTypeOwnership::Opaque,
                path_access: PathAccess::None,
                ..policy
            }),
            generic_params: vec![],
            bounds: vec![],
            variants: vec![],
            fields: vec![field(
                "score",
                Ty::Builtin(BuiltinType::I32),
                PathAccess::ReadOnly,
            )],
        },
    ];
    module.documentation.insert(
        player,
        "Host-owned player metadata, available without business services.".into(),
    );
    let echo = definitions.intern_child(root, DefinitionKind::Function, "echo", 0)?;
    module.functions.push(FnDecl {
        name: "echo".into(),
        implementation: CallableImplementation::Native(echo),
        method_policy: MethodPolicy::default(),
        generic_params: vec![],
        bounds: vec![],
        params: vec![Param {
            name: "value".into(),
            ty: array.clone(),
            mutable: false,
        }],
        return_type: array.clone(),
        host_policy: Some(HostCallPolicy {
            passing: vec![HostPassingStyle::Owned],
            effects: Default::default(),
        }),
    });

    let path_declaration = HostPathDeclaration {
        root: player,
        segments: vec![HostPathSegmentDeclaration::Field(score)],
        access: PathAccess::ReadWrite,
        schema_epoch: 0,
    };
    let index_declaration = HostPathDeclaration {
        root: player,
        segments: vec![HostPathSegmentDeclaration::Index(
            HostIndexSegmentDeclaration {
                slot: 0,
                collection: Ty::Host(player),
                index: Ty::Builtin(BuiltinType::I32),
                result: Ty::Builtin(BuiltinType::I32),
                access: PathAccess::ReadOnly,
            },
        )],
        access: PathAccess::ReadOnly,
        schema_epoch: 0,
    };
    let field_index_declaration = HostPathDeclaration {
        root: player,
        segments: vec![
            HostPathSegmentDeclaration::Field(scores),
            HostPathSegmentDeclaration::Index(HostIndexSegmentDeclaration {
                slot: 0,
                collection: array.clone(),
                index: Ty::Builtin(BuiltinType::I32),
                result: Ty::Builtin(BuiltinType::I32),
                access: PathAccess::ReadOnly,
            }),
        ],
        access: PathAccess::ReadOnly,
        schema_epoch: 0,
    };
    let nested_declaration = HostPathDeclaration {
        root: player,
        segments: vec![
            HostPathSegmentDeclaration::Index(HostIndexSegmentDeclaration {
                slot: 0,
                collection: Ty::Host(player),
                index: Ty::Builtin(BuiltinType::I32),
                result: Ty::Host(child),
                access: PathAccess::ReadOnly,
            }),
            HostPathSegmentDeclaration::Index(HostIndexSegmentDeclaration {
                slot: 1,
                collection: Ty::Host(child),
                index: Ty::Builtin(BuiltinType::I32),
                result: Ty::Host(child),
                access: PathAccess::ReadOnly,
            }),
            HostPathSegmentDeclaration::Virtual(HostVirtualSegmentDeclaration {
                name: "selected".into(),
                result: Ty::Host(child),
                access: PathAccess::ReadOnly,
            }),
            HostPathSegmentDeclaration::Field(child_score),
        ],
        access: PathAccess::ReadOnly,
        schema_epoch: 0,
    };
    let implementation = definitions.intern_child(root, DefinitionKind::Impl, "", 0)?;
    let mut methods = Vec::new();
    for (name, arguments, kind, path) in [
        ("read_score", &[][..], Read, &path_declaration),
        ("set_score", &["next"][..], Set, &path_declaration),
        ("add_score", &["amount"][..], Add, &path_declaration),
        ("read_index", &["index"][..], Read, &index_declaration),
        (
            "read_scores_at",
            &["index"][..],
            Read,
            &field_index_declaration,
        ),
        (
            "read_nested_score",
            &["first", "second"][..],
            Read,
            &nested_declaration,
        ),
    ] {
        let method = definitions.intern_child(implementation, DefinitionKind::Method, name, 0)?;
        let mut params = vec![Param {
            name: "self".into(),
            ty: Ty::Host(player),
            mutable: false,
        }];
        params.extend(arguments.iter().map(|&name| Param {
            name: name.into(),
            ty: Ty::Builtin(BuiltinType::I32),
            mutable: false,
        }));
        methods.push(FnDecl {
            name: name.into(),
            implementation: CallableImplementation::Native(method),
            method_policy: MethodPolicy::default(),
            generic_params: vec![],
            bounds: vec![],
            return_type: Ty::Builtin(if kind == Set {
                BuiltinType::Unit
            } else {
                BuiltinType::I32
            }),
            host_policy: Some(HostCallPolicy {
                passing: vec![HostPassingStyle::Owned; params.len()],
                effects: HostFunctionEffects {
                    may_mutate_host_state: kind != Read,
                    may_trap: true,
                    ..Default::default()
                },
            }),
            params,
        });
        module.path_operations.insert(
            method,
            NativePathOperation {
                path: path.clone(),
                kind,
            },
        );
    }
    module.implementations.push(ImplDecl {
        generic_params: vec![],
        bounds: vec![],
        trait_type: None,
        for_type: Ty::Host(player),
        methods,
    });
    module.paths = vec![
        path_declaration,
        index_declaration,
        field_index_declaration,
        nested_declaration,
    ];
    let retained = module.collect_definitions(&definitions)?;
    Ok(DefinitionMetadata::checked(
        retained,
        module,
        &Default::default(),
    )?)
}

fn main() -> Result<(), Box<dyn Error>> {
    let builder = KagariEngine::builder().unwrap();
    let library = StandardLibrary::new(builder.declarations()).unwrap();
    // A binding provider shares its ordinary declaration records with tooling.
    // This does not construct a NativeModule, callback, host object or Runtime.
    let offline = declarations()?;
    let definitions = DefinitionContext::new()?;
    let offline = offline.import_into(&definitions, &Default::default())?;
    let module = offline.records();
    let player = module.definition(DefinitionKind::Struct, "Player", offline.definitions())?;
    let expected_player = offline.definitions().resolve(player)?.to_path();
    let schemas = module
        .types
        .iter()
        .map(|definition| TypeDeclaration {
            declaration: module
                .definition(
                    DefinitionKind::Struct,
                    &definition.name,
                    offline.definitions(),
                )
                .unwrap(),
            definition: definition.clone(),
        })
        .collect::<Vec<_>>();
    for declaration in module.native_declarations(offline.definitions())? {
        declaration.validate_path_operation(&schemas, offline.definitions())?;
    }
    let expected_paths = module.paths.clone();
    let path = expected_paths[0].contract(&schemas, offline.definitions())?;
    println!(
        "offline field path fingerprint: {:016x}",
        path.fingerprint(offline.definitions())?
    );
    let mut analysis = AnalysisDatabase::default();
    let mut providers = library.declarations().declarations().to_vec();
    providers.push(Arc::new(offline));
    analysis.set_native_modules(providers);
    let mut sources = SourceDatabase::default();
    let mut root = None;
    for (name, text) in [
        (
            "api",
            "pub use demo::echo; pub use demo::Player; pub use demo as service;",
        ),
        ("main", include_str!("offline_compile/main.kgr")),
    ] {
        let path = format!("mem://{name}");
        sources
            .bind_module(
                &path,
                ModuleIdentity {
                    package: PackageId("build".into()),
                    path: vec![name.into()],
                },
            )
            .expect("bind the example module");
        root = Some(
            sources
                .set(&path, text.into(), SourceLayer::Base)
                .expect("register example source"),
        );
    }
    let root = root.expect("entry source");
    let signatures = analysis
        .signatures(sources.snapshot(), &Default::default())
        .expect("cold signatures");
    let signature = signatures.file(root).unwrap();
    let text = signature.source().text();
    let annotation = text.find("value: api::Player").unwrap() + "value: api::".len();
    let declaration = signature.queries().definition_at(annotation).unwrap();
    assert_eq!(declaration.name, "Player");
    let DeclarationId::Definition(player) = declaration.id else {
        panic!("external type identity")
    };
    assert_eq!(
        signature.definitions().resolve(player)?.to_path(),
        expected_player
    );
    assert!(signature.queries().definition_at(annotation - 2).is_none());
    println!("offline type navigation is available before body analysis");
    let query = analysis
        .snapshot(sources.snapshot(), &Default::default())
        .expect("cold analysis");
    let source = query.file(root).unwrap();
    let text = source.source().text();
    let field = text.find("value.score").unwrap();
    assert!(source.queries().field_at(field).is_none());
    assert!(source.queries().field_at(field + "value".len()).is_none());
    let score = source.queries().field_at(field + "value.".len()).unwrap();
    let score = source.definitions().resolve(score)?.to_path();
    assert_eq!(score.module.package.0, "demo");
    assert_eq!(
        score
            .path
            .iter()
            .map(|part| part.name.as_str())
            .collect::<Vec<_>>(),
        ["Player", "score"]
    );
    let method = text.find(".read_nested_score(").unwrap();
    assert!(source.queries().definition_at(method).is_none());
    assert_eq!(
        source.queries().definition_at(method + 1).unwrap().name,
        "read_nested_score"
    );
    println!("offline field and method navigation select their terminal names");
    let checked = query
        .check_program(root, &Default::default())
        .expect("checked offline declarations");
    let ir = lower_program_to_mir(
        &CheckedCompilation::from_program(&checked, &Default::default()).unwrap(),
        &Default::default(),
    )
    .expect("MIR lowering");
    let program = lower_program_to_bytecode(&ir, &Default::default()).expect("bytecode lowering");
    let artifact = KbcArtifact::from_verified(
        program,
        ArtifactBuildOptions {
            portable_mir: Some(PortableMir {
                bytes: encode_program(&ir, &Default::default())?,
            }),
            ..Default::default()
        },
        &Default::default(),
    )?;
    let function_records: usize = artifact
        .records()
        .program
        .modules
        .iter()
        .map(|module| module.functions.len())
        .sum();
    assert_eq!(
        artifact
            .records()
            .tables
            .sections
            .iter()
            .find(|section| section.id == ArtifactSectionId::Functions)
            .unwrap()
            .record_count,
        function_records
    );
    let decoded =
        BytecodeArtifact::from_bytes(&artifact.records().to_bytes(artifact.definitions())?)?;
    let decoded = decoded.import_into(&definitions, &Default::default())?;
    decoded.records().validate_for_loader(
        &Default::default(),
        &definitions,
        &Default::default(),
    )?;
    assert_eq!(
        decoded.records().header.format_version,
        KBC_ARTIFACT_FORMAT_VERSION
    );
    assert_eq!(
        decoded.records().header.module_identity,
        decoded.records().program.modules[decoded.records().program.root.index()].identity
    );
    assert_eq!(
        decoded.records().tables.sections,
        artifact.records().tables.sections
    );
    let root = &decoded.records().program.modules[decoded.records().program.root.index()];
    assert_eq!(root.path_declarations.len(), expected_paths.len());
    for path in expected_paths {
        assert!(root.path_declarations.contains(&path));
    }
    assert_eq!(root.external_types.len(), schemas.len());
    for schema in schemas {
        assert!(root.external_types.contains(&schema));
    }
    let imports: usize = decoded
        .records()
        .program
        .modules
        .iter()
        .map(|module| module.native_imports.len())
        .sum();
    println!(
        "compiled {imports} common native applications and {function_records} executable records; artifact is {} bytes",
        artifact.records().to_bytes(artifact.definitions())?.len()
    );
    println!(
        "bounded artifact decoding retains exact shared type/path requirements without a runtime"
    );
    Ok(())
}
