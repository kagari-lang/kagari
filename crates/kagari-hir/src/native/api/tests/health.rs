use crate::{
    analysis::AnalysisDatabase,
    compile_input::program::{AdmissionError, CheckedCompilation},
    types::TypeId,
};
use kagari_common::identity::{
    DefinitionKind, ModuleIdentity, PackageId, map::DefinitionContext, metadata::DefinitionMetadata,
};
use kagari_source::{
    diagnostic::DiagnosticKind,
    source_database::{SourceDatabase, SourceLayer},
};
use kagari_types::{
    callable::CallableImplementation,
    declaration::{
        FnDecl, Param, TypeDef, TypeDefKind,
        host::{
            HostCallPolicy, HostPassingStyle, HostReflectionPolicy, HostTypeOwnership,
            HostTypePolicy, PathAccess,
        },
        module::{ImplDecl, ModuleDecl},
    },
    scalar::BuiltinType,
    ty::{NominalTy, Ty},
};
use std::sync::Arc;

const HEALTH: &str = include_str!("../../../../tests/fixtures/native/health.kgr");

fn player(
    owner: &ModuleIdentity,
    provider: &ModuleIdentity,
) -> Arc<DefinitionMetadata<ModuleDecl>> {
    let context = DefinitionContext::new().unwrap();
    let root = context.intern_root(provider).unwrap();
    let player = context
        .intern_child(root, DefinitionKind::Struct, "Player", 0)
        .unwrap();
    let implementation = context
        .intern_child(root, DefinitionKind::Impl, "", 0)
        .unwrap();
    let hp = context
        .intern_child(implementation, DefinitionKind::Method, "hp", 0)
        .unwrap();
    let owner_root = context.intern_root(owner).unwrap();
    let health = context
        .intern_child(owner_root, DefinitionKind::Trait, "Health", 0)
        .unwrap();
    let mut module = ModuleDecl::new(provider.clone());
    module.dependencies.insert(owner.clone());
    module.types.push(TypeDef {
        name: "Player".into(),
        kind: TypeDefKind::Host(HostTypePolicy {
            ownership: HostTypeOwnership::HostRoot,
            reflection: HostReflectionPolicy::Metadata,
            path_access: PathAccess::ReadOnly,
        }),
        generic_params: vec![],
        bounds: vec![],
        fields: vec![],
        variants: vec![],
    });
    module.implementations.push(ImplDecl {
        generic_params: vec![],
        bounds: vec![],
        trait_type: Some(NominalTy {
            declaration: health,
            arguments: vec![],
            associated_types: Default::default(),
        }),
        for_type: Ty::Host(player),
        methods: vec![FnDecl {
            name: "hp".into(),
            implementation: CallableImplementation::Native(hp),
            params: vec![Param {
                name: "self".into(),
                mutable: false,
                ty: Ty::Host(player),
            }],
            return_type: Ty::Builtin(BuiltinType::I32),
            generic_params: vec![],
            bounds: vec![],
            method_policy: Default::default(),
            host_policy: Some(HostCallPolicy {
                passing: vec![HostPassingStyle::SharedBorrow],
                effects: Default::default(),
            }),
        }],
    });
    let definitions = module.collect_definitions(&context).unwrap();
    Arc::new(DefinitionMetadata::checked(definitions, module, &Default::default()).unwrap())
}

#[test]
fn host_impl_checks_the_visible_source_trait_on_both_routes() {
    let owner = ModuleIdentity {
        package: PackageId("api".into()),
        path: vec![],
    };
    let provider = ModuleIdentity {
        package: PackageId("host".into()),
        path: vec![],
    };
    let cancel = Default::default();
    let mut structured = AnalysisDatabase::default();
    let registration = player(&owner, &provider);
    structured.set_native_modules(vec![registration.clone()]);
    let mut sources = SourceDatabase::default();
    sources.bind_module("api.kgr", owner.clone()).unwrap();
    sources
        .set("api.kgr", HEALTH.into(), SourceLayer::Base)
        .unwrap();
    let declarations = structured
        .declarations(sources.snapshot(), &cancel)
        .unwrap();
    let document = declarations
        .render_native_document(&registration, &cancel)
        .unwrap();
    let mut editor = AnalysisDatabase::default();
    editor.set_import_environment(declarations.import_environment().clone());
    let mut documents = SourceDatabase::default();
    documents.bind_module("api.kgr", owner.clone()).unwrap();
    documents.bind_module("host.kgr", provider.clone()).unwrap();
    documents
        .set("host.kgr", document.text, SourceLayer::Base)
        .unwrap();

    for (db, inputs, registered) in [
        (&mut structured, &mut sources, true),
        (&mut editor, &mut documents, false),
    ] {
        for (text, expected) in [
            (HEALTH.to_owned(), None),
            (
                HEALTH.replace("-> i32", "-> String"),
                Some(DiagnosticKind::TraitMethodMismatch {
                    trait_name: "Health".into(),
                    method_name: "hp".into(),
                    reason: "return type expected `String`, found `i32`".into(),
                }),
            ),
            (
                HEALTH.replace("hp(self)", "hp(player: Self)"),
                Some(DiagnosticKind::TraitMethodMismatch {
                    trait_name: "Health".into(),
                    method_name: "hp".into(),
                    reason: "receiver parameter differs".into(),
                }),
            ),
            (
                HEALTH.replace("-> i32;", "-> i32 where Self: Marker;"),
                Some(DiagnosticKind::TraitMethodMismatch {
                    trait_name: "Health".into(),
                    method_name: "hp".into(),
                    reason: "generic bound differs".into(),
                }),
            ),
            (
                HEALTH.replace("pub trait Health", "trait Health"),
                Some(DiagnosticKind::ImportNotPublic {
                    path: "api::Health".into(),
                }),
            ),
        ] {
            let api = inputs.set("api.kgr", text, SourceLayer::Overlay).unwrap();
            let snapshot = db.snapshot(inputs.snapshot(), &cancel).unwrap();
            let owner_diagnostics = snapshot.file(api).unwrap().result().diagnostics();
            assert!(owner_diagnostics.is_empty(), "{owner_diagnostics:?}");
            let host = snapshot
                .files()
                .find(|file| file.source().module_identity() == &provider)
                .unwrap();
            let diagnostics = host.result().diagnostics();
            let program = snapshot.check_program(host.source().id(), &cancel);
            if let Some(expected) = expected {
                assert!(
                    matches!(diagnostics, [d] if d.kind == expected),
                    "{diagnostics:?}"
                );
                assert!(program.is_err());
            } else {
                assert!(diagnostics.is_empty(), "{diagnostics:?}");
                let program = program.unwrap();
                let facts = host.result().facts();
                let implementation = facts
                    .declarations
                    .impl_identity(facts.lowered.module.impls[0].id)
                    .unwrap();
                let checked = facts
                    .aggregates
                    .implementation_signature(implementation)
                    .unwrap();
                let TypeId::Host(player) = checked.for_type else {
                    panic!("expected Host receiver")
                };
                let health = host
                    .definitions()
                    .resolve(checked.trait_type.declaration)
                    .unwrap();
                assert_eq!(health.module(), &owner);
                assert_eq!(health.last().unwrap().name, "Health");
                let compilation = CheckedCompilation::from_program(&program, &cancel);
                if !registered {
                    assert!(
                        matches!(compilation, Err(AdmissionError::MissingHostSchema(path)) if path == host.definitions().resolve(player).unwrap().to_path())
                    );
                    continue;
                }
                let compilation = compilation.unwrap();
                let module = compilation
                    .modules()
                    .iter()
                    .find(|module| module.source().module_identity() == &provider)
                    .unwrap();
                let header = module
                    .implementation(module.headers().implementations[0].id)
                    .unwrap();
                let interface = header.interface.unwrap();
                let selected = module
                    .environment()
                    .definitions()
                    .resolve(interface.declaration)
                    .unwrap();
                assert_eq!(selected.module(), &owner);
                assert_eq!(selected.last().unwrap().name, "Health");
            }
        }
    }

    // A public owner present in the same graph is still not authority: a
    // replacement scope can withdraw the dependency recorded by the registration.
    let mut unreachable = declarations.import_environment().clone();
    unreachable
        .modules
        .insert(provider.clone(), Default::default());
    structured.set_import_environment(unreachable);
    sources
        .set("api.kgr", HEALTH.into(), SourceLayer::Overlay)
        .unwrap();
    let snapshot = structured.snapshot(sources.snapshot(), &cancel).unwrap();
    let host = snapshot
        .files()
        .find(|file| file.source().module_identity() == &provider)
        .unwrap();
    let diagnostics = host.result().diagnostics();
    assert!(
        matches!(diagnostics, [d] if matches!(&d.kind, DiagnosticKind::ImportNotPublic { path } if path == "api::Health")),
        "{diagnostics:?}"
    );
}
