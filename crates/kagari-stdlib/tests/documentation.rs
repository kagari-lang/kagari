use kagari_common::identity::DefinitionKind;
use kagari_hir::analysis::AnalysisDatabase;
use kagari_source::{source::SourceFile, source_database::SourceSnapshot};
use kagari_stdlib::modules;
use kagari_types::declaration::module::ModuleDecl;
use std::{collections::BTreeSet, sync::Arc};

#[test]
fn standard_documentation_covers_the_registered_api_and_examples_analyze() {
    let providers: Vec<_> = modules()
        .unwrap()
        .iter()
        .map(|module| Arc::new(module.to_declaration().unwrap()))
        .collect();
    let mut examples = BTreeSet::new();
    let mut traits = 0;
    for module in &providers {
        assert!(
            !module.module_documentation.is_empty(),
            "{}",
            module.identity
        );
        for contract in &module.traits {
            traits += 1;
            let owner = module.definition(DefinitionKind::Trait, &contract.name);
            assert!(module.documentation[&owner].contains("```kgr"), "{owner:?}");
            for method in &contract.methods {
                assert!(
                    !module.documentation[&ModuleDecl::method_id(&owner, &method.name)].is_empty()
                );
            }
            for member in &contract.associated_types {
                assert!(!module.documentation[&member.declaration].is_empty());
            }
        }
        for text in
            std::iter::once(&module.module_documentation).chain(module.documentation.values())
        {
            for block in text.split("```kgr\n").skip(1) {
                examples.insert(block.split("```").next().unwrap().to_string());
            }
        }
    }
    assert_eq!(traits, 40);
    let mut baseline = AnalysisDatabase::default();
    baseline.set_native_modules(providers);
    let native = baseline
        .snapshot(SourceSnapshot::default(), &Default::default())
        .unwrap();
    for declaration in native.declaration_snapshot().files() {
        let file = native.file(declaration.source().id()).unwrap();
        assert!(
            file.result().diagnostics().is_empty(),
            "{:?}",
            file.result().diagnostics()
        );
    }
    drop(native);
    let mut failures = Vec::new();
    for (index, example) in examples.iter().enumerate() {
        let source = Arc::new(SourceFile::new(
            format!("standard-example-{index}.kgr"),
            example,
        ));
        // Start from the sealed provider inputs, never another example's sources.
        let snapshot = baseline
            .clone()
            .snapshot(
                SourceSnapshot::single_file(source.clone()),
                &Default::default(),
            )
            .unwrap();
        let analyzed = snapshot.file(source.id()).unwrap().result();
        if !analyzed.diagnostics().is_empty() {
            failures.push(format!("{example}\n{:?}", analyzed.diagnostics()));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}
