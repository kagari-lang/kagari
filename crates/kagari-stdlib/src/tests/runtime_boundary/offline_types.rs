//! Cold nominal schemas and explicit trait contracts use common declarations and KBC.
use crate::declarations::StandardDeclarations;
use kagari_bytecode::{artifact::KbcArtifact, program::BytecodeProgram};
use kagari_common::identity::{
    DefinitionKind, ModuleIdentity, PackageId, map::DefinitionContext, metadata::DefinitionMetadata,
};
use kagari_compiler::{bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir};
use kagari_hir::analysis::AnalysisDatabase;
use kagari_runtime::{
    Runtime,
    error::{RuntimeError, RuntimeErrorKind},
    host::{HostCallContext, HostError},
    metadata::TypeKind,
    native::{builder::ModuleBuilder, declarations::MethodDecl, module::NativeModule, types::Type},
    value::Value,
};
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use kagari_types::{
    callable::{CallableImplementation, MethodPolicy},
    declaration::{
        FieldDef, FnDecl, Param, TypeDef, TypeDefKind, fingerprint,
        host::{
            HostCallPolicy, HostFieldPolicy, HostFunctionEffects, HostPassingStyle,
            HostReflectionPolicy, HostTypeOwnership, HostTypePolicy, PathAccess,
        },
        module::{ImplDecl, ModuleDecl},
    },
    language::binding,
    scalar::BuiltinType,
    ty::{NominalTy, Ty},
    visibility::Visibility,
};
use std::{collections::BTreeMap, sync::Arc};

fn field(name: &str, ty: Ty, writable: bool) -> FieldDef {
    FieldDef {
        name: name.into(),
        ty,
        visibility: Visibility::Public,
        mutable: writable,
        host_policy: Some(HostFieldPolicy {
            readable: true,
            path_access: if writable {
                PathAccess::ReadWrite
            } else {
                PathAccess::ReadOnly
            },
        }),
    }
}

fn policy() -> HostTypePolicy {
    HostTypePolicy {
        ownership: HostTypeOwnership::HostRoot,
        reflection: HostReflectionPolicy::Metadata,
        path_access: PathAccess::ReadWrite,
    }
}

fn declarations() -> DefinitionMetadata<ModuleDecl> {
    let context = DefinitionContext::new().unwrap();
    let mut module = ModuleDecl::new(ModuleIdentity {
        package: PackageId("model".into()),
        path: vec![],
    });
    let root = context.intern_root(&module.identity).unwrap();
    let player = context
        .intern_child(root, DefinitionKind::Struct, "Player", 0)
        .unwrap();
    let team = context
        .intern_child(root, DefinitionKind::Struct, "Team", 0)
        .unwrap();
    module.types = vec![
        TypeDef {
            name: "Player".into(),
            kind: TypeDefKind::Host(policy()),
            generic_params: vec![],
            bounds: vec![],
            variants: vec![],
            fields: vec![
                field("team", Ty::Host(team), false),
                field("score", Ty::Builtin(BuiltinType::I32), true),
            ],
        },
        TypeDef {
            name: "Team".into(),
            kind: TypeDefKind::Host(policy()),
            generic_params: vec![],
            bounds: vec![],
            variants: vec![],
            fields: vec![field(
                "members",
                Ty::Array(Box::new(Ty::Host(player))),
                false,
            )],
        },
    ];
    let owner = context
        .intern_child(root, DefinitionKind::Impl, "", 0)
        .unwrap();
    let method = context
        .intern_child(owner, DefinitionKind::Method, "add_score", 0)
        .unwrap();
    module.implementations.push(ImplDecl {
        generic_params: vec![],
        bounds: vec![],
        trait_type: None,
        for_type: Ty::Host(player),
        methods: vec![FnDecl {
            name: "add_score".into(),
            implementation: CallableImplementation::Native(method),
            method_policy: MethodPolicy::default(),
            generic_params: vec![],
            bounds: vec![],
            params: vec![
                Param {
                    name: "self".into(),
                    ty: Ty::Host(player),
                    mutable: false,
                },
                Param {
                    name: "amount".into(),
                    ty: Ty::Builtin(BuiltinType::I32),
                    mutable: false,
                },
            ],
            return_type: Ty::Enum(NominalTy {
                declaration: context.intern(&binding::result_declaration()).unwrap(),
                arguments: vec![
                    Ty::Builtin(BuiltinType::I32),
                    Ty::Builtin(BuiltinType::String),
                ],
                associated_types: BTreeMap::new(),
            }),
            host_policy: Some(HostCallPolicy {
                passing: vec![HostPassingStyle::UniqueBorrow, HostPassingStyle::Owned],
                effects: HostFunctionEffects {
                    may_mutate_host_state: true,
                    ..Default::default()
                },
            }),
        }],
    });
    module.documentation.insert(method, "Add score.".into());
    module.collect_definitions(&context).unwrap();
    DefinitionMetadata::checked(context.snapshot(), module, &Default::default()).unwrap()
}

fn bound(declarations: DefinitionMetadata<ModuleDecl>) -> Result<NativeModule, RuntimeError> {
    let mut builder = ModuleBuilder::from_declaration(
        declarations,
        &StandardDeclarations::default().catalog().unwrap(),
        BTreeMap::new(),
    )?;
    let method = builder.declared_method(0, "add_score")?;
    assert!(builder.declared_method(1, "add_score").is_err());
    assert!(builder.declared_method(0, "missing").is_err());
    builder.bind(
        method,
        |_: &HostCallContext<'_>, _: &[Value]| -> Result<Value, HostError> {
            panic!("metadata linking cannot invoke callbacks")
        },
    )?;
    builder.finish()
}

fn program(
    declaration: DefinitionMetadata<ModuleDecl>,
    text: &str,
) -> DefinitionMetadata<BytecodeProgram> {
    // Compilation sees immutable schemas only; all runtime fixtures load decoded bytes.
    let mut modules = StandardDeclarations::default()
        .declarations()
        .unwrap()
        .to_vec();
    modules.push(Arc::new(declaration));
    let mut analysis = AnalysisDatabase::default();
    analysis.set_native_modules(modules);
    let mut sources = SourceDatabase::default();
    let root = sources
        .set("offline-types.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let snapshot = analysis
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let checked = snapshot.check_program(root, &Default::default()).unwrap();
    let mir = lower_program_to_mir(&checked, &Default::default()).unwrap();
    let bytecode = lower_program_to_bytecode(&mir, &Default::default())
        .map(|program| program.into_unverified())
        .unwrap();
    let context = DefinitionContext::new().unwrap();
    let bytecode = bytecode.import_into(&context, &Default::default()).unwrap();
    let artifact = KbcArtifact::from_program(
        bytecode.into_records(),
        Default::default(),
        &context,
        &Default::default(),
    )
    .unwrap();
    let decoded =
        KbcArtifact::from_bytes(&artifact.to_bytes(&context.snapshot()).unwrap()).unwrap();
    let decoded = decoded.import_into(&context, &Default::default()).unwrap();
    decoded
        .records()
        .validate_for_loader(&Default::default(), &context, &Default::default())
        .unwrap();
    let (definitions, artifact) = decoded.into_parts();
    DefinitionMetadata::checked(definitions, artifact.program, &Default::default()).unwrap()
}

fn install(module: NativeModule) -> Runtime {
    let mut runtime = Runtime::default();
    let mut modules = crate::modules().unwrap();
    modules.push(module);
    NativeModule::install_all(&modules, &mut runtime).unwrap();
    runtime
}

fn trait_module(implementation: Option<HostPassingStyle>) -> NativeModule {
    let mut module =
        ModuleBuilder::new("model", &StandardDeclarations::default().catalog().unwrap()).unwrap();
    let mut counter = module.define_type("Counter").unwrap();
    counter.external(policy()).unwrap();
    let counter = counter.finish().unwrap();
    let mut readable = module.define_trait("Readable").unwrap();
    readable
        .define_method(MethodDecl::instance("get").returns(Type::i32()))
        .unwrap();
    let readable = readable.finish().unwrap();
    if let Some(passing) = implementation {
        module
            .implement(counter, |group| {
                group.trait_impl(readable.apply([])?, |methods| {
                    methods.host_policy(
                        "get",
                        HostCallPolicy {
                            passing: vec![passing],
                            effects: Default::default(),
                        },
                    )?;
                    methods.bind("get", |_: &HostCallContext<'_>, _: &[Value]| {
                        Ok(Value::I32(1))
                    })
                })
            })
            .unwrap();
    }
    module.finish().unwrap()
}

#[test]
fn host_trait_table_requires_bound_method_callbacks_before_linking() {
    let module = trait_module(Some(HostPassingStyle::SharedBorrow));
    let contract = module.to_declaration();
    let unbound = ModuleBuilder::from_declaration(
        contract.clone(),
        &StandardDeclarations::default().catalog().unwrap(),
        BTreeMap::new(),
    )
    .unwrap();
    assert!(unbound.finish().is_err());
    let program = program(
        contract,
        "use model::Counter; use model::Readable; pub fn read(c: Counter) -> i32 { c.get() }",
    );
    let mut missing = install(trait_module(None));
    assert!(missing.load_program("trait", program.clone()).is_err());
    assert_eq!(missing.modules().loaded_count(), 0);
    let mut mismatched = install(trait_module(Some(HostPassingStyle::UniqueBorrow)));
    assert!(mismatched.load_program("trait", program.clone()).is_err());
    assert_eq!(mismatched.modules().loaded_count(), 0);
    install(module).load_program("trait", program).unwrap();
}

#[test]
fn offline_members_generate_runtime_metadata_and_resolve_mutual_references() {
    let declaration = declarations();
    let module = bound(declaration.clone()).unwrap();
    let player_id = declaration
        .records()
        .definition(DefinitionKind::Struct, "Player", declaration.definitions())
        .unwrap();
    let mut runtime = install(module);
    let player = runtime.types().get_by_name("model::Player").unwrap();
    let team = runtime.types().get_by_name("model::Team").unwrap();
    assert_eq!(player.fields[0].ty, team.id);
    assert_eq!(
        runtime.types().get(player.fields[1].ty).unwrap().name,
        "i32"
    );
    assert_eq!(player.methods[0].params[0].ty, player.id);
    assert_eq!(player.methods[0].params[1].ty, player.fields[1].ty);
    assert_eq!(
        runtime
            .types()
            .get(player.methods[0].return_type)
            .unwrap()
            .kind,
        TypeKind::Enum
    );
    let cancel = Default::default();
    assert_eq!(
        player.fields[1].abi_fingerprint.0,
        fingerprint::field(
            player_id,
            &declaration.records().types[0],
            1,
            &Ty::Builtin(BuiltinType::I32),
            declaration.definitions(),
            &cancel
        )
        .unwrap()
    );
    let program = program(
        declaration,
        "use model::Player; pub fn add(p: Player) { p.add_score(1); }",
    );
    runtime.load_program("mutual", program).unwrap();
}

#[test]
fn unresolved_members_and_duplicate_batches_publish_no_metadata() {
    let original = declarations();
    for change in 0..3 {
        let (definitions, mut module) = original.clone().into_parts();
        match change {
            0 => {
                module.types.pop();
            }
            1 => module.types.push(module.types[0].clone()),
            _ => {
                let duplicate = module.types[1].fields[0].clone();
                module.types[1].fields.push(duplicate);
            }
        }
        let invalid =
            DefinitionMetadata::checked(definitions, module, &Default::default()).unwrap();
        assert!(bound(invalid).is_err());
    }
    let valid = bound(original).unwrap();
    let mut runtime = install(valid.clone());
    let before = runtime.types().len();
    assert_eq!(
        valid.install(&mut runtime).unwrap_err().kind(),
        RuntimeErrorKind::MetadataConflict
    );
    assert_eq!(runtime.types().len(), before);
    assert!(runtime.types().get_by_name("model::Player").is_some());
    assert!(runtime.types().get_by_name("model::Team").is_some());
}

#[test]
fn type_contract_changes_reject_artifacts_before_publication_but_documentation_does_not() {
    let original = declarations();
    let module = bound(original.clone()).unwrap();
    let mut runtime = install(module);
    let source = "use model::Player; pub fn add(p: Player) { p.add_score(1); }";
    let baseline = program(original.clone(), source);
    for change in 0..4 {
        let (definitions, mut declaration) = original.clone().into_parts();
        match change {
            0 => declaration.types[0].fields[1].ty = Ty::Builtin(BuiltinType::I64),
            1 => {
                declaration.types[0].fields[1].mutable = false;
                declaration.types[0].fields[1]
                    .host_policy
                    .as_mut()
                    .unwrap()
                    .path_access = PathAccess::ReadOnly;
            }
            2 => {
                declaration.implementations[0].methods[0]
                    .host_policy
                    .as_mut()
                    .unwrap()
                    .passing[0] = HostPassingStyle::SharedBorrow
            }
            _ => {
                declaration.implementations[0].methods[0]
                    .host_policy
                    .as_mut()
                    .unwrap()
                    .effects
                    .may_mutate_host_state = false
            }
        }
        let changed =
            DefinitionMetadata::checked(definitions, declaration, &Default::default()).unwrap();
        // Each candidate is independently valid and has matching local callbacks;
        // rejection therefore checks the already installed contract, not bad syntax.
        bound(changed.clone()).unwrap();
        assert!(
            runtime
                .load_program("types", program(changed, source))
                .is_err()
        );
        assert_eq!(runtime.modules().loaded_count(), 0);
    }
    let (definitions, mut documented) = original.into_parts();
    let root = definitions.lookup_root(&documented.identity).unwrap();
    let player = documented
        .definition(DefinitionKind::Struct, "Player", &definitions)
        .unwrap();
    let field = definitions
        .lookup_child(player, DefinitionKind::Field, "team", 0)
        .unwrap();
    let owner = documented.implementation_id(0, &definitions).unwrap();
    let method = ModuleDecl::method_id(owner, "add_score", &definitions).unwrap();
    for id in [root, player, field, method] {
        documented.documentation.insert(id, "Updated docs.".into());
    }
    let documented =
        DefinitionMetadata::checked(definitions, documented, &Default::default()).unwrap();
    let documented = program(documented, source);
    let bytes = |program: DefinitionMetadata<BytecodeProgram>| {
        let context = DefinitionContext::new().unwrap();
        let program = program.import_into(&context, &Default::default()).unwrap();
        KbcArtifact::from_program(
            program.into_records(),
            Default::default(),
            &context,
            &Default::default(),
        )
        .unwrap()
        .to_bytes(&context.snapshot())
        .unwrap()
    };
    assert_eq!(bytes(baseline), bytes(documented.clone()));
    runtime.load_program("types", documented).unwrap();
}
