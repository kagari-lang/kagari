use super::*;
use kagari_common::{cancellation::CancellationToken, identity::map::DefinitionContext};
use kagari_source::source::SourceFile;
use kagari_stdlib::catalog;
use kagari_syntax::parser::parse_declarations;

#[test]
fn native_views_preserve_canonical_text_and_scoped_navigation() {
    let left = catalog::shared().unwrap();
    let right = catalog::declarations().unwrap();
    let right = right.into_iter().map(Arc::new).collect::<Vec<_>>();
    assert_ne!(left[0].definitions().id(), right[0].definitions().id());
    for (left_module, right_module) in left.iter().zip(&right) {
        let left_source = declaration_source(left_module, &left).unwrap();
        let right_source = declaration_source(right_module, &right).unwrap();
        assert_eq!(left_source.uri, right_source.uri);
        assert_eq!(left_source.text, right_source.text);
        let source = SourceFile::new(&left_source.uri, &left_source.text);
        let parsed = parse_declarations(&source, Default::default(), &Default::default()).unwrap();
        assert!(
            parsed.diagnostics().is_empty(),
            "{:?}",
            parsed.diagnostics()
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
    let providers = catalog::shared().unwrap();
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
