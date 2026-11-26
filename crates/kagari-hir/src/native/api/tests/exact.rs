//! Exact IDs remove spelling selection without becoming prechecked semantic types.
use crate::{
    analysis::AnalysisDatabase,
    hir::ty::{DefinitionExposure, TypeKind},
    imports::environment::{ImportEnvironment, ModuleScope},
    typeck::check::check_signatures,
    types::TypeId,
};
use kagari_common::identity::{
    DefinitionKind, ModuleIdentity, PackageId, map::DefinitionContext, metadata::DefinitionMetadata,
};
use kagari_source::{
    diagnostic::DiagnosticKind,
    package::PackageScope,
    source_database::{SourceDatabase, SourceLayer},
};
use kagari_types::{
    callable::CallableImplementation,
    declaration::{FnDecl, Param, TypeDef, TypeDefKind, module::ModuleDecl},
    scalar::BuiltinType,
    ty::{NominalTy, Ty},
};
use std::sync::Arc;

#[test]
fn exact_type_applications_check_visibility_arity_kind_and_foreign_scope() {
    let identity = |path: &[&str]| ModuleIdentity {
        package: PackageId("exact".into()),
        path: path.iter().map(|part| (*part).into()).collect(),
    };
    let mut sources = SourceDatabase::default();
    for (name, path, text) in [
        (
            "provider",
            vec!["api"],
            include_str!("../../../../tests/fixtures/native/exact-provider.kgr"),
        ),
        (
            "consumer",
            vec!["consumer"],
            include_str!("../../../../tests/fixtures/native/exact-consumer.kgr"),
        ),
    ] {
        sources.bind_module(name, identity(&path)).unwrap();
        sources.set(name, text.into(), SourceLayer::Base).unwrap();
    }
    let context = DefinitionContext::new().unwrap();
    let hidden = context.intern_root(&identity(&["api", "hidden"])).unwrap();
    let secret = context
        .intern_child(hidden, DefinitionKind::Struct, "Secret", 0)
        .unwrap();
    let mut registration = ModuleDecl::new(ModuleIdentity {
        package: PackageId("registration".into()),
        path: vec!["bridge".into()],
    });
    let root = context.intern_root(&registration.identity).unwrap();
    let inspect = context
        .intern_child(root, DefinitionKind::Function, "inspect", 0)
        .unwrap();
    registration.functions.push(FnDecl {
        name: "inspect".into(),
        implementation: CallableImplementation::Native(inspect),
        generic_params: vec![],
        bounds: vec![],
        method_policy: Default::default(),
        host_policy: None,
        params: vec![Param {
            name: "value".into(),
            mutable: false,
            ty: Ty::Struct(NominalTy {
                declaration: secret,
                arguments: vec![],
                associated_types: Default::default(),
            }),
        }],
        return_type: Ty::Builtin(BuiltinType::I32),
    });
    // A lexical type masks the first package alias; the second reaches the facade.
    registration.types.push(TypeDef {
        name: "Masked".into(),
        kind: TypeDefKind::Struct,
        generic_params: vec![],
        bounds: vec![],
        fields: vec![],
        variants: vec![],
    });
    registration.collect_definitions(&context).unwrap();
    let registration_identity = registration.identity.clone();
    let registration = Arc::new(
        DefinitionMetadata::checked(context.snapshot(), registration, &Default::default()).unwrap(),
    );
    let mut db = AnalysisDatabase::new("/exact").unwrap();
    db.set_native_modules(vec![registration.clone()]);
    db.set_import_environment(ImportEnvironment {
        modules: [(
            registration_identity.clone(),
            ModuleScope {
                packages: PackageScope {
                    dependencies: ["Masked", "visible"]
                        .map(|alias| (alias.into(), identity(&[]).package))
                        .into(),
                    ..Default::default()
                },
                dependencies: [identity(&["api", "hidden"])].into(),
                ..Default::default()
            },
        )]
        .into(),
    });
    let snapshot = db
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let file = snapshot
        .files()
        .find(|file| file.source().module_identity() == &identity(&["consumer"]))
        .unwrap();
    assert!(
        file.result().diagnostics().is_empty(),
        "{:?}",
        file.result().diagnostics()
    );
    let original = &file.result().facts;
    let definitions = original.declarations.definitions();
    let api = definitions.lookup_root(&identity(&["api"])).unwrap();
    let hidden = definitions
        .lookup_root(&identity(&["api", "hidden"]))
        .unwrap();
    let public = definitions
        .lookup_child(api, DefinitionKind::Struct, "Public", 0)
        .unwrap();
    let private = definitions
        .lookup_child(api, DefinitionKind::Struct, "Private", 0)
        .unwrap();
    let secret = definitions
        .lookup_child(hidden, DefinitionKind::Struct, "Secret", 0)
        .unwrap();
    let native = snapshot
        .files()
        .find(|file| file.source().module_identity() == &registration_identity)
        .unwrap();
    assert!(
        native.result().diagnostics().is_empty(),
        "{:?}",
        native.result().diagnostics()
    );
    let signature = &native.signatures().facts().functions()[0];
    let reference = native.result().facts.lowered.module.functions[0].params[0].ty;
    assert!(matches!(
        native
            .result()
            .facts
            .lowered
            .module
            .type_ref(reference)
            .kind,
        TypeKind::Definition { exposure: None, .. }
    ));
    let selected = native
        .signatures()
        .facts()
        .type_table()
        .type_ref(reference)
        .unwrap();
    assert_eq!(
        selected.exposure,
        Some(DefinitionExposure {
            module: identity(&["api"]),
            path: vec!["Exposed".into()]
        })
    );
    assert!(
        matches!(&signature.params[0].ty, TypeId::Struct(nominal) if nominal.declaration == secret)
    );
    let external = original
        .typed
        .signatures
        .functions()
        .iter()
        .find(|function| function.name == "from_exposure")
        .unwrap();
    assert_eq!(external.params[0].ty, signature.params[0].ty);
    let published = snapshot.declaration_snapshot();
    let document = published
        .render_native_document(&registration, &Default::default())
        .unwrap();
    assert!(
        document.text.contains("value: visible::api::Exposed"),
        "{}",
        document.text
    );
    let mut offline = AnalysisDatabase::new("/exact").unwrap();
    offline.set_import_environment(published.import_environment().clone());
    sources
        .bind_module(&document.uri, registration_identity.clone())
        .unwrap();
    sources
        .set(&document.uri, document.text, SourceLayer::Overlay)
        .unwrap();
    let editor = offline
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    for owner in [&registration_identity, &identity(&["consumer"])] {
        let file = editor
            .files()
            .find(|file| file.source().module_identity() == owner)
            .unwrap();
        assert!(
            file.result().diagnostics().is_empty(),
            "{:?}",
            file.result().diagnostics()
        );
        let name = if owner == &registration_identity {
            "inspect"
        } else {
            "from_exposure"
        };
        let function = file
            .signatures()
            .facts()
            .functions()
            .iter()
            .find(|f| f.name == name)
            .unwrap();
        let TypeId::Struct(ty) = &function.params[0].ty else {
            panic!("expected the exposed struct")
        };
        assert_eq!(
            file.definitions()
                .resolve(ty.declaration)
                .unwrap()
                .to_path(),
            definitions.resolve(secret).unwrap().to_path()
        );
    }
    sources.close_overlay(&document.uri).unwrap();
    let consumer = definitions.lookup_root(&identity(&["consumer"])).unwrap();
    let function = definitions
        .lookup_child(consumer, DefinitionKind::Function, "accepts", 0)
        .unwrap();
    let foreign = DefinitionContext::new().unwrap();
    let root = foreign.intern_root(&identity(&["api"])).unwrap();
    let stale = foreign
        .intern_child(root, DefinitionKind::Struct, "Public", 0)
        .unwrap();

    let exposed = DefinitionExposure {
        module: identity(&["api"]),
        path: vec!["Exposed".into()],
    };
    let defining = DefinitionExposure {
        module: identity(&["api", "hidden"]),
        path: vec!["Secret".into()],
    };
    let public_route = DefinitionExposure {
        module: identity(&["api"]),
        path: vec!["Public".into()],
    };
    for (declaration, arguments, exposure, valid) in [
        (public, true, None, true),
        (public, false, None, false),
        (private, false, None, false),
        (secret, false, None, true),
        (secret, false, Some(&exposed), true),
        (secret, false, Some(&defining), false),
        (secret, true, Some(&public_route), false),
        (public, false, Some(&exposed), false),
        (function, false, None, false),
        (stale, true, Some(&public_route), false),
    ] {
        let mut lowered = original.lowered.as_ref().clone();
        let function = &lowered.module.functions[0];
        let reference = function.params[0].ty;
        let argument = function.params[1].ty;
        lowered.module.nodes.types[reference.index()].1.kind = TypeKind::Definition {
            declaration,
            exposure: exposure.cloned(),
            // Deliberately not a resolvable spelling: the ID selects the target.
            name: "diagnostic_label_only".into(),
            args: if arguments {
                [argument].into_iter().collect()
            } else {
                Default::default()
            },
            bindings: vec![],
        };
        let checked =
            check_signatures(&lowered, &original.declarations, &Default::default()).unwrap();
        if valid {
            assert!(
                checked.diagnostics().is_empty(),
                "{:?}",
                checked.diagnostics()
            );
            let TypeId::Struct(nominal) = &checked.facts().functions()[0].params[0].ty else {
                panic!("expected checked nominal type");
            };
            let selected = checked.facts().type_table().type_ref(reference).unwrap();
            assert_eq!(
                selected.exposure.as_ref(),
                Some(if declaration == secret {
                    &exposed
                } else {
                    &public_route
                })
            );
            assert_eq!(nominal.declaration, declaration);
            assert_eq!(
                nominal.arguments,
                if arguments {
                    vec![TypeId::from_name("i32").unwrap()]
                } else {
                    vec![]
                }
            );
        } else {
            assert!(
                checked.diagnostics().iter().any(|diagnostic| matches!(
                    diagnostic.kind,
                    DiagnosticKind::UnknownType { .. }
                )),
                "{:?}",
                checked.diagnostics()
            );
            assert_eq!(checked.facts().functions()[0].params[0].ty, TypeId::Error);
            if exposure.is_some() {
                assert!(
                    checked
                        .facts()
                        .type_table()
                        .type_ref(reference)
                        .unwrap()
                        .exposure
                        .is_none(),
                    "invalid route must not become selected exposure evidence"
                );
            }
        }
    }

    // A second public binding makes ID-only exposure selection ambiguous. This
    // must invalidate retained signature facts even though the nominal type itself
    // has not changed; an explicit route can still select the intended binding.
    sources
        .set(
            "provider",
            format!(
                "{}\npub use self::hidden::Secret as Alternate;\n",
                include_str!("../../../../tests/fixtures/native/exact-provider.kgr")
            ),
            SourceLayer::Base,
        )
        .unwrap();
    let changed = db
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let native = changed
        .files()
        .find(|file| file.source().module_identity() == &registration_identity)
        .unwrap();
    assert!(
        native
            .result()
            .diagnostics()
            .iter()
            .any(|diagnostic| matches!(diagnostic.kind, DiagnosticKind::UnknownType { .. })),
        "{:?}",
        native.result().diagnostics()
    );
    let error = changed
        .declaration_snapshot()
        .render_native_document(&registration, &Default::default())
        .unwrap_err();
    assert!(
        error.to_string().contains("no accessible source spelling"),
        "{error}"
    );
    let mut lowered = native.result().facts.lowered.as_ref().clone();
    let reference = lowered.module.functions[0].params[0].ty;
    let TypeKind::Definition { exposure, .. } =
        &mut lowered.module.nodes.types[reference.index()].1.kind
    else {
        panic!("expected structured application");
    };
    *exposure = Some(exposed.clone());
    let checked = check_signatures(
        &lowered,
        &native.result().facts.declarations,
        &Default::default(),
    )
    .unwrap();
    assert!(
        checked.diagnostics().is_empty(),
        "{:?}",
        checked.diagnostics()
    );
    assert_eq!(
        checked
            .facts()
            .type_table()
            .type_ref(reference)
            .unwrap()
            .exposure,
        Some(exposed.clone())
    );

    // A retained route is never authority after its public binding disappears.
    sources
        .set(
            "provider",
            include_str!("../../../../tests/fixtures/native/exact-provider.kgr")
                .replace("as Exposed", "as Alternate"),
            SourceLayer::Base,
        )
        .unwrap();
    let changed = db
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let native = changed
        .files()
        .find(|file| file.source().module_identity() == &registration_identity)
        .unwrap();
    assert!(
        native.result().diagnostics().is_empty(),
        "{:?}",
        native.result().diagnostics()
    );
    let reference = native.result().facts.lowered.module.functions[0].params[0].ty;
    assert_eq!(
        native
            .signatures()
            .facts()
            .type_table()
            .type_ref(reference)
            .unwrap()
            .exposure,
        Some(DefinitionExposure {
            module: identity(&["api"]),
            path: vec!["Alternate".into()]
        })
    );
    let mut lowered = native.result().facts.lowered.as_ref().clone();
    let TypeKind::Definition { exposure, .. } =
        &mut lowered.module.nodes.types[reference.index()].1.kind
    else {
        panic!("expected structured application");
    };
    *exposure = Some(exposed);
    let checked = check_signatures(
        &lowered,
        &native.result().facts.declarations,
        &Default::default(),
    )
    .unwrap();
    assert!(
        checked
            .diagnostics()
            .iter()
            .any(|diagnostic| matches!(diagnostic.kind, DiagnosticKind::UnknownType { .. }))
    );
    assert!(
        checked
            .facts()
            .type_table()
            .type_ref(reference)
            .unwrap()
            .exposure
            .is_none()
    );
}
