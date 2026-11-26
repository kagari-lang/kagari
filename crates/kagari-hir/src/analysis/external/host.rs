//! Host identity/access semantics must not turn into owning Native objects offline.

use super::foundation::publish;
use crate::tests::standard::standard_modules;
use crate::{
    analysis::AnalysisDatabase,
    compile_input::program::{AdmissionError, CheckedCompilation},
    hir::expr::ExprKind,
    resolver::resolved::ResolvedName,
    typeck::table::CallTarget,
    types::TypeId,
};
use kagari_common::identity::{
    DefinitionKind, ModuleIdentity, PackageId, map::DefinitionContext, metadata::DefinitionMetadata,
};
use kagari_source::{
    diagnostic::{DiagnosticKind, Severity},
    source_database::{SourceDatabase, SourceLayer},
};
use kagari_types::{
    declaration::{
        FieldDef, TypeDef, TypeDefKind,
        host::{
            HostFieldPolicy, HostReflectionPolicy, HostTypeOwnership, HostTypePolicy, PathAccess,
        },
        module::ModuleDecl,
        native::NativeStorageLayout,
        path::{HostIndexSegmentDeclaration, HostPathDeclaration, HostPathSegmentDeclaration},
    },
    language::{Protocol, binding},
    scalar::BuiltinType,
    ty::Ty,
    visibility::Visibility,
};
use std::sync::Arc;

fn provider() -> Arc<DefinitionMetadata<ModuleDecl>> {
    let context = DefinitionContext::new().unwrap();
    let mut module = ModuleDecl::new(ModuleIdentity {
        package: PackageId("host".into()),
        path: vec![],
    });
    module.types.push(TypeDef {
        name: "Player".into(),
        kind: TypeDefKind::Host(HostTypePolicy {
            ownership: HostTypeOwnership::HostRoot,
            reflection: HostReflectionPolicy::Metadata,
            path_access: PathAccess::ReadWrite,
        }),
        generic_params: vec![],
        bounds: vec![],
        variants: vec![],
        fields: [
            ("hp", true, Visibility::Public, PathAccess::ReadWrite),
            ("rank", false, Visibility::Public, PathAccess::ReadOnly),
            ("hidden", false, Visibility::Private, PathAccess::ReadOnly),
            ("scores", false, Visibility::Public, PathAccess::None),
        ]
        .into_iter()
        .map(|(name, mutable, visibility, path_access)| FieldDef {
            name: name.into(),
            mutable,
            visibility,
            ty: if name == "scores" {
                Ty::Array(Box::new(Ty::Builtin(BuiltinType::I32)))
            } else {
                Ty::Builtin(BuiltinType::I32)
            },
            host_policy: Some(HostFieldPolicy {
                readable: path_access != PathAccess::None,
                path_access,
            }),
        })
        .collect(),
    });
    let definitions = module.collect_definitions(&context).unwrap();
    let root = module
        .definition(DefinitionKind::Struct, "Player", &definitions)
        .unwrap();
    for (name, access) in [
        ("hp", PathAccess::ReadWrite),
        ("rank", PathAccess::ReadOnly),
    ] {
        module.paths.push(HostPathDeclaration {
            root,
            segments: vec![HostPathSegmentDeclaration::Field(
                definitions
                    .lookup_child(root, DefinitionKind::Field, name, 0)
                    .unwrap(),
            )],
            access,
            schema_epoch: 7,
        });
    }
    // A registered index adapter is not a Kagari Index implementation.
    module.paths.push(HostPathDeclaration {
        root,
        segments: vec![HostPathSegmentDeclaration::Index(
            HostIndexSegmentDeclaration {
                slot: 0,
                collection: Ty::Host(root),
                index: Ty::Builtin(BuiltinType::I32),
                result: Ty::Builtin(BuiltinType::I32),
                access: PathAccess::ReadOnly,
            },
        )],
        access: PathAccess::ReadOnly,
        schema_epoch: 7,
    });
    Arc::new(DefinitionMetadata::checked(definitions, module, &Default::default()).unwrap())
}

#[test]
fn host_and_opaque_receivers_keep_their_protocol_and_identity_rules_on_both_routes() {
    let cancel = Default::default();
    let api = ModuleIdentity {
        package: PackageId("game".into()),
        path: vec!["api".into()],
    };
    let client = include_str!("../../../tests/fixtures/native/host_protocol_identity.kgr");
    for kind in [
        TypeDefKind::NativeStorage(NativeStorageLayout::Opaque),
        TypeDefKind::Host(HostTypePolicy {
            ownership: HostTypeOwnership::HostRoot,
            reflection: HostReflectionPolicy::Metadata,
            path_access: PathAccess::ReadOnly,
        }),
    ] {
        let mut provider = ModuleDecl::new(api.clone());
        provider.types.push(TypeDef {
            name: "Player".into(),
            kind,
            generic_params: vec![],
            bounds: vec![],
            fields: vec![],
            variants: vec![],
        });
        let definitions = provider
            .collect_definitions(&DefinitionContext::new().unwrap())
            .unwrap();
        let mut db = AnalysisDatabase::default();
        db.set_native_modules(vec![Arc::new(
            DefinitionMetadata::checked(definitions, provider, &cancel).unwrap(),
        )]);
        let mut sources = SourceDatabase::default();
        // Both receivers are local to this impl's package; orphan rejection would mask the distinction.
        sources
            .bind_module(
                "compat.kgr",
                ModuleIdentity {
                    package: api.package.clone(),
                    path: vec!["compat".into()],
                },
            )
            .unwrap();
        let root = sources
            .set("compat.kgr", client.into(), SourceLayer::Base)
            .unwrap();
        let direct = db.snapshot(sources.snapshot(), &cancel).unwrap();
        let mut editor = publish(direct.declaration_snapshot(), &mut sources);
        let offline = editor.snapshot(sources.snapshot(), &cancel).unwrap();
        for snapshot in [direct, offline] {
            let file = snapshot.file(root).unwrap();
            let diagnostics = file.result().diagnostics();
            if matches!(kind, TypeDefKind::Host(_)) {
                assert_eq!(diagnostics.len(), 2, "{diagnostics:?}");
                assert!(diagnostics.iter().any(|d| matches!(
                    &d.kind,
                    DiagnosticKind::InvalidTraitImpl { trait_name, type_name, reason }
                    if trait_name == "PartialEq" && type_name == "Player"
                        && reason == "this language protocol does not support Host receivers"
                )));
                assert!(diagnostics.iter().any(|d| matches!(
                    &d.kind,
                    DiagnosticKind::BinaryOperandTypeMismatch { operator: "===", lhs, rhs, .. }
                    if lhs == "Player" && rhs == "Player"
                )));
                assert!(snapshot.check_program(root, &cancel).is_err());
            } else {
                assert!(diagnostics.is_empty(), "{diagnostics:?}");
                snapshot.check_program(root, &cancel).unwrap();
                let facts = file.result().facts();
                let header = facts
                    .declarations
                    .impl_identity(facts.lowered.module.impls[0].id)
                    .unwrap();
                let implementation = facts.aggregates.implementation_signature(header).unwrap();
                assert!(matches!(implementation.for_type, TypeId::NativeObject(_)));
                assert_eq!(
                    Some(implementation.trait_type.declaration),
                    Protocol::PartialEq.definition(file.definitions())
                );
            }
        }
    }
}

#[test]
fn host_async_capture_and_await_typecheck_on_both_routes() {
    let cancel = Default::default();
    let host = provider();
    let mut sources = SourceDatabase::default();
    let root = sources
        .set(
            "client.kgr",
            include_str!("../../../tests/fixtures/analysis/external/host_async.kgr").into(),
            SourceLayer::Base,
        )
        .unwrap();
    let mut direct = AnalysisDatabase::default();
    direct.set_native_modules(vec![host.clone()]);
    let declarations = direct.declarations(sources.snapshot(), &cancel).unwrap();
    let document = declarations.render_native_document(&host, &cancel).unwrap();
    let structured = direct.snapshot(sources.snapshot(), &cancel).unwrap();

    let mut editor = AnalysisDatabase::default();
    editor.set_import_environment(declarations.import_environment().clone());
    sources
        .bind_module(&document.uri, host.records().identity.clone())
        .unwrap();
    sources
        .set(&document.uri, document.text, SourceLayer::Base)
        .unwrap();
    let parsed = editor.snapshot(sources.snapshot(), &cancel).unwrap();
    for (snapshot, registered) in [(structured, true), (parsed, false)] {
        let file = snapshot.file(root).unwrap();
        let diagnostics = file.result().diagnostics();
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let program = snapshot.check_program(root, &cancel).unwrap();
        // Generated declarations describe the field but do not install its adapter.
        if registered {
            CheckedCompilation::from_program(&program, &cancel).unwrap();
        }

        let facts = file.result().facts();
        let factory = facts
            .typed
            .signatures()
            .functions()
            .iter()
            .find(|f| f.name == "combined_hp")
            .unwrap();
        assert!(matches!(factory.params[0].ty, TypeId::Host(_)));
        let TypeId::NativeObject(future) = &factory.return_type else {
            panic!("expected a cold Future")
        };
        let i32_ty = TypeId::Builtin(BuiltinType::I32);
        assert_eq!(
            Some(future.declaration),
            file.definitions().lookup(&binding::future_declaration())
        );
        assert_eq!(future.arguments, [TypeId::Builtin(BuiltinType::I32)]);
        let nodes = &facts.lowered.module.nodes;
        let types = facts.typed.type_table();
        let (closure, _) = nodes
            .expressions()
            .find(|(_, expr)| matches!(expr.kind, ExprKind::Closure { is_async: true, .. }))
            .unwrap();
        assert_eq!(
            facts.names.closure_captures(closure),
            &[ResolvedName::Param(factory.params[0].id)]
        );
        assert_eq!(
            types.expr_type(closure),
            Some(TypeId::Function {
                params: vec![factory.params[1].ty.clone()],
                result: Box::new(factory.return_type.clone()),
            })
        );
        let awaits: Vec<_> = nodes
            .expressions()
            .filter_map(|(id, expr)| match expr.kind {
                ExprKind::Await { expr } => Some((types.expr_type(expr), types.expr_type(id))),
                _ => None,
            })
            .collect();
        assert_eq!(
            awaits,
            vec![(Some(factory.return_type.clone()), Some(i32_ty)); 2]
        );
    }
}

#[test]
fn host_exports_preserve_identity_paths_and_access_restrictions() {
    let mut providers = standard_modules();
    providers.push(provider());
    let mut direct = AnalysisDatabase::new("/project").unwrap();
    direct.set_native_modules(providers);
    let mut sources = SourceDatabase::new("/project").unwrap();
    for (text, valid) in [
        (
            include_str!("../../../tests/fixtures/analysis/external/host_client.kgr"),
            true,
        ),
        (
            include_str!("../../../tests/fixtures/analysis/external/host_invalid.kgr"),
            false,
        ),
    ] {
        let root = sources
            .set("main.kgr", text.into(), SourceLayer::Overlay)
            .unwrap();
        sources
            .replace_generated(vec![], &Default::default())
            .unwrap();
        let direct_snapshot = direct
            .snapshot(sources.snapshot(), &Default::default())
            .unwrap();
        let mut offline = publish(direct_snapshot.declaration_snapshot(), &mut sources);
        let offline_snapshot = offline
            .snapshot(sources.snapshot(), &Default::default())
            .unwrap();
        let direct_file = direct_snapshot.file(root).unwrap();
        let offline_file = offline_snapshot.file(root).unwrap();
        let errors = |file: &crate::analysis::FileAnalysis| {
            file.result()
                .diagnostics()
                .iter()
                .filter(|diagnostic| diagnostic.severity == Severity::Error)
                .map(|diagnostic| (diagnostic.kind.clone(), diagnostic.span))
                .collect::<Vec<_>>()
        };
        assert_eq!(errors(direct_file), errors(offline_file));
        assert_eq!(
            errors(offline_file).is_empty(),
            valid,
            "{:?}",
            errors(offline_file)
        );
        let facts = offline_file.result().facts();
        let function = facts
            .typed
            .signatures()
            .functions()
            .iter()
            .find(|function| {
                if valid {
                    function.name == "update"
                } else {
                    function.name == "denied"
                }
            })
            .unwrap();
        let TypeId::Host(player) = function.params[0].ty else {
            panic!("expected the declared Player Host type")
        };
        let view = offline_file.definitions().resolve(player).unwrap();
        assert_eq!(view.module().package.0, "host");
        assert_eq!(view.last().unwrap().name, "Player");
        if valid {
            for (snapshot, registered) in [(&direct_snapshot, true), (&offline_snapshot, false)] {
                let file = snapshot.file(root).unwrap();
                let facts = file.result().facts();
                let (site, receiver) = facts
                    .lowered
                    .module
                    .nodes
                    .expressions()
                    .find_map(|(id, expr)| match expr.kind {
                        ExprKind::Index { receiver, .. } => Some((id, receiver)),
                        _ => None,
                    })
                    .unwrap();
                let types = facts.typed.type_table();
                let scores = types.expr_field(receiver).unwrap();
                assert_eq!(
                    file.definitions()
                        .resolve(*scores)
                        .unwrap()
                        .last()
                        .unwrap()
                        .name,
                    "scores"
                );
                let call = types
                    .call_resolution(site)
                    .expect("ordinary array index application");
                assert_eq!(call.receiver, Some(receiver));
                assert!(
                    matches!(call.target, CallTarget::TraitMethod { interface, .. } if Some(interface.declaration) == Protocol::Index.definition(file.definitions()))
                );
                let program = snapshot.check_program(root, &Default::default()).unwrap();
                if !registered {
                    assert!(matches!(
                        CheckedCompilation::from_program(&program, &Default::default()),
                        Err(AdmissionError::MissingHostSchema(path)) if path == view.to_path()
                    ));
                    continue;
                }
                let compilation =
                    CheckedCompilation::from_program(&program, &Default::default()).unwrap();
                let provider = compilation
                    .modules()
                    .iter()
                    .find(|module| module.source().module_identity().package.0 == "host")
                    .unwrap();
                let [schema] = provider.registered_host_schemas() else {
                    panic!("one registered Host owner")
                };
                let definitions = provider.environment().definitions();
                assert_eq!(
                    definitions
                        .resolve(schema.declaration)
                        .unwrap()
                        .last()
                        .unwrap()
                        .name,
                    "Player"
                );
                let hp = definitions
                    .lookup_child(schema.declaration, DefinitionKind::Field, "hp", 0)
                    .unwrap();
                assert_eq!(
                    provider.registered_host_paths()[0].segments,
                    [HostPathSegmentDeclaration::Field(hp)]
                );
                for path in provider.registered_host_paths() {
                    assert_eq!(path.root, schema.declaration);
                    path.contract(provider.registered_host_schemas(), definitions)
                        .unwrap();
                }
            }
        } else {
            let diagnostics = offline_file.result().diagnostics();

            assert!(
                diagnostics
                    .iter()
                    .any(|d| matches!(&d.kind, DiagnosticKind::InvalidIndexTarget { type_name } if type_name == "Player")
                        && d.span.is_some_and(|span| &text[span.start..span.end] == "player[0]")),
                "{diagnostics:?}"
            );
        }
    }
}
