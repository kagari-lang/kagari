use super::*;
use crate::tests::standard::standard_modules;
use crate::{
    analysis::{AnalysisDatabase, declarations::DeclarationSnapshot, error::AnalysisError},
    imports::{
        DirectiveResolution, ResolvedTarget,
        environment::{ImportEnvironment, ModuleScope},
    },
    native::adoption::adopt,
};
use kagari_common::identity::{DefinitionPath, PackageId};
use kagari_common::{cancellation::CancellationToken, identity::map::DefinitionContext};
use kagari_source::source::SourceFile;
use kagari_source::{
    package::{PackageEnvironment, PackageScope},
    source_database::{CapturedSource, SourceDatabase},
};
use kagari_syntax::{
    ast::visit::NodeRef,
    parser::{ParseOptions, parse},
};
use kagari_types::declaration::names::{ExportName, NameNamespace};

#[test]
fn native_views_preserve_canonical_text_and_scoped_navigation() {
    let left = standard_modules();
    let cancel = CancellationToken::default();
    let context = DefinitionContext::new().unwrap();
    // Catalog projections share one sealed scope; import explicitly to test another scope.
    let right = adopt(&left, &context, &cancel).unwrap();
    assert_ne!(left[0].definitions().id(), right[0].definitions().id());
    for (left_module, right_module) in left.iter().zip(&right) {
        let left_source = declaration_source(left_module, &left).unwrap();
        let right_source = declaration_source(right_module, &right).unwrap();
        assert_eq!(left_source.uri, right_source.uri);
        assert_eq!(left_source.text, right_source.text);
        let source = SourceFile::new(&left_source.uri, &left_source.text);
        let parsed = parse(&source, ParseOptions::default(), &Default::default()).unwrap();
        assert!(
            parsed.diagnostics().is_empty(),
            "{:?}",
            parsed.diagnostics()
        );
        let module = parsed.module();
        let mut actual = NodeRef::ModuleBody(module.body())
            .descendants()
            .filter_map(|node| match node {
                NodeRef::FnDef(function) => Some((
                    function.name_text().unwrap(),
                    function.is_native(),
                    function.body().is_some(),
                )),
                NodeRef::MethodDef(function) => Some((
                    function.name_text().unwrap(),
                    function.is_native(),
                    function.body().is_some(),
                )),
                _ => None,
            })
            .collect::<Vec<_>>();
        let records = left_module.records();
        for ty in &records.types {
            let id = records
                .definition(
                    ty.kind.definition_kind(),
                    &ty.name,
                    left_module.definitions(),
                )
                .unwrap();
            let site = &left_source.sites[&id];
            assert_eq!(
                &left_source.text[site.name_span.start..site.name_span.end],
                ty.name
            );
            let header = &left_source.text[site.span.start..site.name_span.start];
            assert_eq!(
                header == "pub native host type ",
                matches!(ty.kind, TypeDefKind::Host(_))
            );
        }
        let mut expected = records
            .functions
            .iter()
            .chain(records.traits.iter().flat_map(|item| &item.methods))
            .chain(
                records
                    .implementations
                    .iter()
                    .flat_map(|item| &item.methods),
            )
            .map(|function| {
                (
                    function.name.clone(),
                    matches!(function.implementation, CallableImplementation::Native(_)),
                    matches!(function.implementation, CallableImplementation::Script),
                )
            })
            .collect::<Vec<_>>();
        actual.sort();
        expected.sort();
        assert_eq!(
            actual, expected,
            "Script definitions must retain their ordinary bodies"
        );
        let sites = |source: &DeclarationSource| {
            source
                .sites
                .iter()
                .map(|(id, site)| {
                    let path = source.definitions.resolve(*id).unwrap().to_path();
                    let name = &source.text[site.name_span.start..site.name_span.end];
                    assert!(!name.is_empty());
                    (
                        path,
                        (
                            site.span,
                            site.name_span,
                            site.generics.clone(),
                            site.parameters.clone(),
                            site.bounds
                                .iter()
                                .map(|bound| (bound.target, bound.constraints.clone()))
                                .collect::<Vec<_>>(),
                        ),
                    )
                })
                .collect::<BTreeMap<_, _>>()
        };
        assert_eq!(sites(&left_source), sites(&right_source));
    }
}

#[test]
fn native_view_retains_its_prefix_and_rejects_missing_implicit_names() {
    let cancel = CancellationToken::default();
    let context = DefinitionContext::new().unwrap();
    let providers = standard_modules();
    let original = providers
        .iter()
        .find(|provider| !provider.records().functions.is_empty())
        .unwrap();
    let imported = original.import_into(&context, &cancel).unwrap();
    let definitions = imported.records().collect_definitions(&context).unwrap();
    let imported =
        DefinitionMetadata::checked(definitions, imported.into_records(), &cancel).unwrap();
    let source = declaration_source(&imported, &providers).unwrap();
    let root = context
        .intern_root(&ModuleIdentity::single_file("later"))
        .unwrap();
    assert!(source.definitions.resolve(root).is_err());
    assert!(
        source
            .sites
            .keys()
            .all(|id| source.definitions.resolve(*id).is_ok())
    );
    let mut records = imported.records().clone();
    records.functions[0].name = "absent_registered_name".into();
    let malformed =
        DefinitionMetadata::checked(imported.definitions().clone(), records, &cancel).unwrap();
    assert!(declaration_source(&malformed, &providers).is_err());
}

/// Independent oracle: read the canonical owner from each resolved source unit,
/// not the path text or any correspondence retained by the renderer.
fn selected(
    snapshot: &DeclarationSnapshot,
    importer: &ModuleIdentity,
) -> BTreeMap<NameNamespace, DefinitionPath> {
    let file = snapshot
        .files()
        .find(|f| f.source().module_identity() == importer)
        .unwrap();
    assert!(file.diagnostics().is_empty(), "{:?}", file.diagnostics());
    let mut result = BTreeMap::new();
    for directive in &file.names().imports.directives {
        let DirectiveResolution::Named(outcomes) = directive.resolution else {
            panic!("expected a named export")
        };
        for (category, outcome) in outcomes.iter() {
            if let Some(target @ ResolvedTarget::Source(source)) = outcome.target() {
                let declarations = snapshot.file(source.unit.file).unwrap().declarations();
                let id = declarations
                    .definition(target.resolved(Some(&source.unit)))
                    .unwrap();
                assert!(
                    result
                        .insert(
                            category,
                            declarations.definitions().resolve(*id).unwrap().to_path()
                        )
                        .is_none()
                );
            }
        }
    }
    result
}

#[test]
fn publication_resolves_token_by_importer_and_preserves_both_namespace_slots() {
    let cancel = Default::default();
    let owner = |name: &str| ModuleIdentity {
        package: PackageId(name.into()),
        path: vec![],
    };
    let left = owner("left");
    let right = owner("right");
    let context = DefinitionContext::new().unwrap();
    let token = |owner| {
        context
            .intern_child(
                context.intern_root(owner).unwrap(),
                DefinitionKind::Struct,
                "Token",
                0,
            )
            .unwrap()
    };
    let kind = context
        .intern_child(
            context.intern_root(&left).unwrap(),
            DefinitionKind::Enum,
            "Kind",
            0,
        )
        .unwrap();
    let variant = context
        .intern_child(kind, DefinitionKind::Variant, "Token", 0)
        .unwrap();
    let registration = |name: &str, ty, value: bool| {
        let mut module = ModuleDecl::new(ModuleIdentity {
            package: PackageId("app".into()),
            path: vec![name.into()],
        });
        module
            .exports
            .insert(ExportName::new(NameNamespace::Type, "Selected"), ty);
        if value {
            module
                .exports
                .insert(ExportName::new(NameNamespace::Value, "Selected"), variant);
        }
        let defs = module.collect_definitions(&context).unwrap();
        Arc::new(DefinitionMetadata::checked(defs, module, &cancel).unwrap())
    };
    let dual = registration("dual", token(&left), true);
    let split = registration("split", token(&right), true);
    let type_only = registration("type_only", token(&left), false);
    let capture = |owner: &ModuleIdentity, text: &str| CapturedSource {
        key: owner.to_string(),
        display_name: owner.to_string(),
        module: owner.clone(),
        text: text.into(),
    };
    let owners = || {
        vec![
            capture(
                &left,
                include_str!("../../../tests/fixtures/native/token_namespaces.kgr"),
            ),
            capture(&right, "pub struct Token {}"),
            capture(&owner("app"), ""),
        ]
    };
    let mut sources = SourceDatabase::default();
    let package_scope = |lib: &ModuleIdentity, alt: &ModuleIdentity| PackageScope {
        dependencies: [
            ("lib".into(), lib.package.clone()),
            ("alt".into(), alt.package.clone()),
        ]
        .into(),
        ..Default::default()
    };
    let environment = |app| {
        PackageEnvironment::new(
            [
                (left.package.clone(), Default::default()),
                (right.package.clone(), Default::default()),
                (PackageId("app".into()), app),
            ]
            .into(),
        )
        .unwrap()
    };
    let previous = sources
        .replace_captured(owners(), environment(package_scope(&right, &left)), &cancel)
        .unwrap();
    let packages = environment(package_scope(&left, &right));
    let source = sources
        .replace_captured(owners(), packages.clone(), &cancel)
        .unwrap();
    let mut db = AnalysisDatabase::default();
    db.set_native_modules(vec![dual.clone(), split.clone()]);
    let previous = db.declarations(previous, &cancel).unwrap();
    let first = db.declarations(source.clone(), &cancel).unwrap();
    assert!(
        previous
            .render_native_document(&split, &cancel)
            .unwrap()
            .text
            .contains("pub use lib::Token as Selected;")
    );
    let explicit = ModuleScope {
        packages: package_scope(&right, &left),
        ..Default::default()
    };
    db.set_import_environment(ImportEnvironment {
        modules: [(split.records().identity.clone(), explicit)].into(),
    });
    let snapshot = db.declarations(source.clone(), &cancel).unwrap();
    let mut inputs = owners();
    assert!(
        first
            .render_native_document(&split, &cancel)
            .unwrap()
            .text
            .contains("pub use alt::Token as Selected;")
    );
    for (module, lines) in [
        (&dual, vec!["pub use lib::Token as Selected;"]),
        (
            &split,
            vec![
                "pub use lib::Token as Selected;",
                "pub use alt::Kind::Token as Selected;",
            ],
        ),
    ] {
        let view = snapshot.render_native_document(module, &cancel).unwrap();
        assert_eq!(
            view.text
                .lines()
                .filter(|l| l.starts_with("pub use"))
                .collect::<Vec<_>>(),
            lines
        );
        inputs.push(capture(&module.records().identity, &view.text));
    }
    db.set_native_modules(vec![type_only.clone()]);
    db.set_import_environment(Default::default());
    let rejected = db.declarations(source, &cancel).unwrap();
    let stopped = CancellationToken::default();
    stopped.cancel();
    assert!(matches!(
        rejected.render_native_document(&type_only, &stopped),
        Err(AnalysisError::Cancelled)
    ));
    let error = rejected
        .render_native_document(&type_only, &cancel)
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("cannot express export Type Selected"),
        "{error}"
    );
    // Zero registrations and no namespace attachments. Check the emitted leaves
    // themselves, including the one leaf which really exports two identities.
    let mut offline = AnalysisDatabase::default();
    offline.set_import_environment(snapshot.import_environment().clone());
    let docs = sources
        .replace_captured(inputs, packages.clone(), &cancel)
        .unwrap();
    let editor = offline.declarations(docs, &cancel).unwrap();
    for module in [&dual, &split] {
        let expected = module
            .records()
            .exports
            .iter()
            .map(|(name, id)| {
                (
                    name.namespace,
                    module.definitions().resolve(*id).unwrap().to_path(),
                )
            })
            .collect();
        for snapshot in [&snapshot, &editor] {
            assert_eq!(selected(snapshot, &module.records().identity), expected);
        }
    }

    // A unique public type in the correct package and namespace is not enough:
    // the registration names a struct, not this trait with the identical spelling.
    let mut replaced = owners();
    replaced
        .iter_mut()
        .find(|doc| doc.module == right)
        .unwrap()
        .text = "pub trait Token {}".into();
    db.set_native_modules(vec![split]);
    let source = sources
        .replace_captured(replaced, packages, &cancel)
        .unwrap();
    let error = db.declarations(source, &cancel).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("export Type Selected does not resolve"),
        "{error}"
    );
}
