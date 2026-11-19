use crate::{
    hir::item::module::ImportLeaf,
    imports::{
        LocalName, ModuleGraph, ModuleOrderError, NamespaceId, ResolvedTarget, SourceUnit,
        catalog::{LookupContext, LookupResult, NamespaceResult},
        tests::{analyze, insert},
    },
    lower::lower_module,
    resolver::resolved::DeclarationNames,
};
use kagari_common::{
    identity::{
        ModuleIdentity, map::DefinitionContext, metadata::DefinitionMetadata,
        table::DefinitionTableError,
    },
    span::Span,
};
use kagari_source::{
    diagnostic::DiagnosticKind, source::SourceFile, source_database::SourceDatabase,
};
use kagari_syntax::parser::{ParseOptions, parse};
use kagari_types::declaration::names::NameNamespace;
use std::sync::Arc;

#[test]
fn module_handles_require_the_owning_context_prefix_and_actual_namespace() {
    let lowered = lower_module(&SourceFile::new("same", "pub fn value() {}"));
    let context = DefinitionContext::new().unwrap();
    let graph = ModuleGraph::build([&lowered], &context, &Default::default()).unwrap();
    let foreign = ModuleGraph::build(
        [&lowered],
        &DefinitionContext::new().unwrap(),
        &Default::default(),
    )
    .unwrap();
    let unit = SourceUnit::of(&lowered, &graph.catalog.definitions);
    let foreign_unit = SourceUnit::of(&lowered, &foreign.catalog.definitions);
    assert_eq!(unit.file, foreign_unit.file);
    assert_eq!(unit.revision, foreign_unit.revision);
    assert_eq!(unit.arena, foreign_unit.arena);
    assert_ne!(unit.module, foreign_unit.module);
    assert!(!foreign_unit.matches(&lowered, &graph.catalog.definitions));
    let late = context
        .intern_module(&ModuleIdentity::single_file("late"))
        .unwrap();
    let ctx = LookupContext::new(&unit.module);
    for (handle, error) in [
        (foreign_unit.module, DefinitionTableError::ForeignTable),
        (late, DefinitionTableError::InvalidIndex),
    ] {
        assert!(graph.node(&handle).is_none());
        assert_eq!(
            graph.reachable_order(&handle, &Default::default()),
            Err(ModuleOrderError::Identity(error))
        );
        let prefix = NamespaceId::InstalledPrefix(handle);
        assert_eq!(
            graph
                .catalog
                .lookup_member(
                    &ctx,
                    &prefix,
                    "value",
                    NameNamespace::Value,
                    &Default::default()
                )
                .unwrap(),
            LookupResult::StaleSource
        );
        assert_eq!(
            graph
                .catalog
                .namespace_of(
                    &ctx,
                    &ResolvedTarget::Namespace(prefix),
                    &Default::default()
                )
                .unwrap(),
            NamespaceResult::StaleSource
        );
        assert_eq!(
            graph
                .catalog
                .lookup_member(
                    &LookupContext::new(&handle),
                    &NamespaceId::Module(unit),
                    "value",
                    NameNamespace::Value,
                    &Default::default()
                )
                .unwrap(),
            LookupResult::StaleSource
        );
    }
    assert!(!graph.catalog.valid(&foreign_unit));
    let imports = graph.imports_for(&unit).unwrap().clone();
    let names = DeclarationNames {
        items: imports.scope.clone(),
        imports,
        catalog: graph.catalog.clone(),
    };
    assert!(
        DefinitionMetadata::checked(
            graph.catalog.definitions.clone(),
            names.clone(),
            &Default::default()
        )
        .is_ok()
    );
    assert!(
        DefinitionMetadata::checked(
            foreign.catalog.definitions.clone(),
            names.clone(),
            &Default::default()
        )
        .is_err()
    );
    let mixed = DeclarationNames {
        catalog: foreign.catalog.clone(),
        ..names
    };
    assert!(
        DefinitionMetadata::checked(
            graph.catalog.definitions.clone(),
            mixed,
            &Default::default()
        )
        .is_err()
    );
    // Registering an identity is not the same as adding a graph/source namespace.
    let rebuilt = ModuleGraph::build([&lowered], &context, &Default::default()).unwrap();
    assert!(rebuilt.catalog.definitions.module_identity(late).is_ok());
    assert!(rebuilt.node(&late).is_none());
    assert_eq!(
        rebuilt.reachable_order(&late, &Default::default()),
        Err(ModuleOrderError::Missing(ModuleIdentity::single_file(
            "late"
        )))
    );
}

#[test]
fn recovered_import_leaves_preserve_names_and_physical_sites() {
    for (text, path, syntax_error, leaf) in [
        ("use ;", "", Some(DiagnosticKind::ExpectedUseTree), ""),
        (
            "use a as ;",
            "a",
            Some(DiagnosticKind::ExpectedUseAlias),
            "a as",
        ),
        (
            "use a::;",
            "a",
            Some(DiagnosticKind::ExpectedUseTree),
            "a::",
        ),
        ("use {};", "", None, "{}"),
        ("use a::{};", "a", None, "a::{}"),
    ] {
        let source = SourceFile::new("recovery", text);
        let parsed = parse(&source, ParseOptions::default(), &Default::default())
            .expect("fresh cancellation token");
        assert_eq!(
            parsed
                .diagnostics()
                .iter()
                .map(|d| d.kind.clone())
                .collect::<Vec<_>>(),
            syntax_error.into_iter().collect::<Vec<_>>(),
            "{text}"
        );
        for diagnostic in parsed.diagnostics().iter() {
            let span = diagnostic.span.unwrap();
            assert_eq!(span.start, text.len() - 1);
            assert_eq!(span.end, text.len());
        }
        let lowered = lower_module(&source);
        let [import] = lowered.module.imports.as_slice() else {
            panic!("one recovered leaf expected for {text}");
        };
        assert_eq!(
            import.segments,
            if path.is_empty() { vec![] } else { vec![path] }
        );
        assert_eq!(import.local_name(), (!path.is_empty()).then_some(path));
        assert!(matches!(import.kind, ImportLeaf::Named { alias: None }));
        assert_eq!(import.root_span, import.span);
        assert_eq!(&text[import.span.start..import.span.end], leaf);
        if text == "use ;" {
            assert_eq!(import.span, Span::new(4, 4));
        }

        let graph = ModuleGraph::build(
            [&lowered],
            &DefinitionContext::new().unwrap(),
            &Default::default(),
        )
        .unwrap();
        let facts = graph
            .imports_for(&SourceUnit::of(&lowered, &graph.catalog.definitions))
            .unwrap();
        let [directive] = facts.directives.as_slice() else {
            panic!("one recovered directive expected for {text}");
        };
        assert_eq!(directive.span, source.span(import.span));
        assert_eq!(directive.root_span, source.span(import.root_span));
        assert_eq!(
            facts
                .diagnostics
                .iter()
                .map(|d| d.kind.clone())
                .collect::<Vec<_>>(),
            [DiagnosticKind::UnknownName { name: path.into() }]
        );
        assert_eq!(facts.diagnostics[0].span, Some(import.span));
        for namespace in NameNamespace::ALL {
            assert!(facts.scope.lookup(path, namespace).is_none());
        }
    }
}

#[test]
fn source_less_reexport_resolves_without_physical_import_sites() {
    let mut sources = SourceDatabase::default();
    let text = "use core::option::Some;";
    let root = insert(&mut sources, "root", text);
    let snapshot = analyze(&sources);
    let graph = snapshot.module_graph();
    let owner = kagari_stdlib::namespaces::type_owner("Option");
    let owner = graph.catalog.definitions.lookup_module(&owner).unwrap();
    let provider = graph.node(&owner).unwrap();
    let analysis = snapshot.file(provider.file).unwrap();
    let lowered = &analysis.result().facts().lowered;
    assert!(lowered.source_surface().is_none());
    let name = LocalName::new("Some").unwrap();
    let reexport = provider
        .imports
        .directives
        .iter()
        .find(|directive| directive.segments == ["self", "Option", "Some"])
        .expect("structured re-export of Option::Some");
    assert_eq!(reexport.span, None);
    assert_eq!(reexport.root_span, None);
    let target = reexport.resolution.target().expect("resolved re-export");
    assert!(matches!(target, ResolvedTarget::Source(source) if source.unit.module == owner));
    let [binding] = provider.imports.scope.entries[&name][NameNamespace::Value]
        .strong
        .as_slice()
    else {
        panic!("one re-export binding");
    };
    assert_eq!(binding.resolution.target(), Some(target));
    assert_eq!(binding.location, None);
    let declared = LocalName::new("Option").unwrap();
    let [declaration] = provider.imports.scope.entries[&declared][NameNamespace::Type]
        .strong
        .as_slice()
    else {
        panic!("one declaration binding");
    };
    assert!(declaration.resolution.target().is_some());
    assert_eq!(declaration.location, None);

    let imported = snapshot.file(root).unwrap();
    let [written] = imported
        .result()
        .facts()
        .names
        .imports
        .directives
        .as_slice()
    else {
        panic!("one written import");
    };
    assert_eq!(written.resolution.target(), Some(target));
    let site = imported.source().span(Span::new(4, text.len() - 1));
    assert!(site.is_some());
    assert_eq!(written.span, site);
    assert_eq!(written.root_span, site);
    assert!(imported.result().diagnostics().is_empty());
}

#[test]
fn unused_leaf_imports_keep_facade_and_implementation_dependencies() {
    let mut sources = SourceDatabase::default();
    insert(&mut sources, "library", "pub fn value() -> i32 { 1 }");
    insert(&mut sources, "facade", "pub use pkg::library::value;");
    let root = insert(&mut sources, "root", "use pkg::facade::value; fn main() {}");
    let snapshot = analyze(&sources);
    let imports = &snapshot.file(root).unwrap().result().facts().names.imports;
    for name in ["facade", "library"] {
        assert!(
            imports.directives[0]
                .direct_dependencies
                .iter()
                .any(|module| snapshot.module_graph().catalog.identity(*module).path == [name])
        );
        assert!(
            imports.dependencies.iter().any(|module| snapshot
                .module_graph()
                .catalog
                .identity(*module)
                .path
                == [name])
        );
    }
    assert!(snapshot.check_program(root, &Default::default()).is_ok());
}

#[test]
fn duplicate_logical_modules_keep_source_units_and_reject_absolute_lookup() {
    let left = lower_module(&SourceFile::new(
        "same",
        "fn left() -> i32 { 1 } use missing::one;",
    ));
    let right = lower_module(&SourceFile::new(
        "same",
        "fn right() -> i32 { 2 } use missing::two;",
    ));
    let graph = ModuleGraph::build(
        [&left, &right],
        &DefinitionContext::new().unwrap(),
        &Default::default(),
    )
    .unwrap();
    for (lowered, own, other) in [(&left, "left", "right"), (&right, "right", "left")] {
        let unit = SourceUnit::of(lowered, &graph.catalog.definitions);
        let facts = graph.imports_for(&unit).unwrap();
        assert_eq!(facts.directives[0].id.unit, unit);
        assert!(facts.scope.lookup(own, NameNamespace::Value).is_some());
        assert!(facts.scope.lookup(other, NameNamespace::Value).is_none());
        let table = &graph.catalog.namespaces[&NamespaceId::Module(unit)];
        assert!(Arc::ptr_eq(&facts.scope, &table.names));
        assert!(facts.diagnostics.iter().any(|d| matches!(
            d.kind,
            kagari_source::diagnostic::DiagnosticKind::DuplicateDeclaration { .. }
        )));
    }
    assert!(matches!(
        graph.reachable_order(
            &graph
                .catalog
                .definitions
                .lookup_module(left.source.module_identity())
                .unwrap(),
            &Default::default()
        ),
        Err(ModuleOrderError::InvalidImports(_))
    ));
    let importer = graph
        .catalog
        .definitions
        .lookup_module(left.source.module_identity())
        .unwrap();
    let ctx = LookupContext::new(&importer);
    let path = [
        left.source.module_identity().package.0.clone(),
        "left".into(),
    ];
    assert!(matches!(
        graph
            .catalog
            .absolute(&ctx, &path, NameNamespace::Type, &Default::default())
            .unwrap(),
        LookupResult::Ambiguous(_)
    ));
}

#[test]
fn relowered_same_revision_targets_are_stale_and_strong_collisions_do_not_filter_to_a_winner() {
    let source = SourceFile::new(
        "same",
        "pub struct Data {} pub fn value() -> i32 { 1 } fn value() -> i32 { 2 }",
    );
    let first = lower_module(&source);
    let second = lower_module(&source);
    let definitions = DefinitionContext::new().unwrap();
    let importer = definitions
        .intern_module(&ModuleIdentity::single_file("other"))
        .unwrap();
    let old = ModuleGraph::build([&first], &definitions, &Default::default()).unwrap();
    let new = ModuleGraph::build([&second], &definitions, &Default::default()).unwrap();
    let ctx = LookupContext::new(&importer);
    let ns = NamespaceId::Module(SourceUnit::of(&first, &old.catalog.definitions));
    let LookupResult::Found(hit) = old
        .catalog
        .lookup_member(&ctx, &ns, "Data", NameNamespace::Type, &Default::default())
        .unwrap()
    else {
        panic!("public type")
    };
    assert_eq!(
        new.catalog
            .namespace_of(&ctx, &hit.target, &Default::default())
            .unwrap(),
        NamespaceResult::StaleSource
    );
    assert_eq!(
        new.catalog
            .lookup_member(&ctx, &ns, "Data", NameNamespace::Type, &Default::default())
            .unwrap(),
        LookupResult::StaleSource
    );
    assert!(matches!(
        old.catalog
            .lookup_member(
                &ctx,
                &ns,
                "value",
                NameNamespace::Value,
                &Default::default()
            )
            .unwrap(),
        LookupResult::Ambiguous(_)
    ));
    let facts = old
        .imports_for(&SourceUnit::of(&first, &old.catalog.definitions))
        .unwrap();
    assert!(
        facts
            .scope
            .entries
            .contains_key(&LocalName::new("value").unwrap())
    );
}
