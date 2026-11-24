use crate::{AnalyzerQueries, query_tests::test_analysis};
use kagari_common::identity::map::*;
use kagari_common::identity::metadata::*;
use kagari_hir::analysis::*;
// Retained identity prefixes, fixed-ID remapping and analysis-local binding ownership.
use kagari_common::identity::{ModuleIdentity, PackageId};
use kagari_hir::declarations::DeclarationId;
use kagari_source::source_database::{SourceDatabase, SourceLayer};

fn snapshot(db: &mut AnalysisDatabase, sources: &SourceDatabase) -> AnalysisSnapshot {
    db.snapshot(sources.snapshot(), &Default::default())
        .unwrap()
}

#[test]
fn definitions_are_module_owned_but_bindings_are_analysis_and_body_owned() {
    let text = "fn same(value: i32) -> i32 { val n = value; n } fn use_it() -> i32 { same(1) }";
    let module = |name: &str| ModuleIdentity {
        package: PackageId("game".into()),
        path: vec![name.into()],
    };
    let mut sources = SourceDatabase::default();
    let a = sources.bind_module("a.kgr", module("a")).unwrap();
    let b = sources.bind_module("b.kgr", module("b")).unwrap();
    sources
        .set("a.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    sources
        .set("b.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut db = test_analysis();
    let first = snapshot(&mut db, &sources);
    let function_a = first
        .file(a)
        .unwrap()
        .queries()
        .definition_at(text.rfind("same(1)").unwrap())
        .unwrap()
        .clone();
    let function_b = first
        .file(b)
        .unwrap()
        .queries()
        .definition_at(text.rfind("same(1)").unwrap())
        .unwrap();
    assert_ne!(function_a.id, function_b.id);
    let binding_a = first
        .file(a)
        .unwrap()
        .queries()
        .definition_at(text.find("n }").unwrap())
        .unwrap()
        .clone();
    let DeclarationId::Binding(binding) = &binding_a.id else {
        panic!("local binding")
    };
    let DeclarationId::Definition(owner) = &function_a.id else {
        panic!("function definition")
    };
    let definitions = first
        .file(a)
        .unwrap()
        .result()
        .facts()
        .declarations
        .definitions();
    assert_eq!(binding.body, *owner);
    assert!(definitions.resolve(binding.body).is_ok());
    let independent = snapshot(&mut test_analysis(), &sources);
    assert!(independent.definitions().resolve(binding.body).is_err());
    let binding_metadata = DefinitionMetadata::checked(
        definitions.clone(),
        binding_a.id.clone(),
        &Default::default(),
    )
    .unwrap();
    assert!(
        DefinitionMetadata::checked(
            independent.definitions().clone(),
            binding_a.id.clone(),
            &Default::default(),
        )
        .is_err()
    );
    let destination = DefinitionContext::new().unwrap();
    let imported = binding_metadata
        .import_into(&destination, &Default::default())
        .unwrap();
    let DeclarationId::Binding(imported_binding) = imported.records() else {
        panic!("imported local binding");
    };
    assert_ne!(imported_binding.body, binding.body);
    assert_eq!(imported_binding.analysis, binding.analysis);
    assert_eq!(
        imported
            .definitions()
            .resolve(imported_binding.body)
            .unwrap()
            .to_path(),
        definitions.resolve(binding.body).unwrap().to_path()
    );
    assert_eq!(
        first.definitions().resolve(binding.body).unwrap().to_path(),
        definitions.resolve(*owner).unwrap().to_path()
    );
    assert_eq!(
        first
            .declaration(&function_b.id)
            .unwrap()
            .source_location()
            .unwrap()
            .file,
        b
    );
    let unchanged = snapshot(&mut db, &sources);
    let same_source_binding = unchanged
        .file(a)
        .unwrap()
        .queries()
        .definition_at(text.find("n }").unwrap())
        .unwrap();
    let DeclarationId::Binding(fresh_binding) = &same_source_binding.id else {
        panic!("local binding in fresh analysis");
    };
    assert_eq!(fresh_binding.body, binding.body);
    assert_ne!(fresh_binding.analysis, binding.analysis);
    assert_eq!(
        same_source_binding.source_location(),
        binding_a.source_location()
    );
    assert!(unchanged.declaration(&binding_a.id).is_none());
    assert_eq!(
        unchanged.declaration(&same_source_binding.id),
        Some(same_source_binding)
    );

    let edited = text.replace("val n = value", "val n = value + 1");
    sources
        .set("a.kgr", edited.clone(), SourceLayer::Overlay)
        .unwrap();
    let second = snapshot(&mut db, &sources);
    let new_function = second.declaration(&function_a.id).unwrap();
    assert_ne!(
        new_function.source_location().unwrap().revision,
        function_a.source_location().unwrap().revision
    );
    assert_eq!(
        new_function.source_location().unwrap().file,
        function_a.source_location().unwrap().file
    );
    assert!(second.declaration(&binding_a.id).is_none());
    assert_eq!(first.declaration(&binding_a.id), Some(&binding_a));
    let new_binding = second
        .file(a)
        .unwrap()
        .queries()
        .definition_at(edited.find("n }").unwrap())
        .unwrap();
    assert_ne!(new_binding.id, binding_a.id);

    sources.bind_module("a.kgr", module("renamed")).unwrap();
    let rebound = snapshot(&mut db, &sources);
    assert!(rebound.declaration(&function_a.id).is_none());
    assert!(rebound.declaration(&function_b.id).is_some());
}
