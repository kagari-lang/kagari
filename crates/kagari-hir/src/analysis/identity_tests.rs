use super::*;
use crate::declarations::DeclarationId;
use kagari_common::{
    identity::{DefinitionKind, ModuleIdentity, PackageId},
    source_database::{SourceDatabase, SourceLayer},
};

fn snapshot(db: &mut AnalysisDatabase, sources: &SourceDatabase) -> AnalysisSnapshot {
    db.snapshot(
        sources.snapshot(),
        LanguageFeatureProfile::default(),
        &Default::default(),
    )
    .unwrap()
}

#[test]
fn scope_queries_and_navigation_share_resolver_shadowing_and_match_bindings() {
    let text = "fn main(value: i32) -> i32 { var n = value; val n = n + 1; match n { bound => bound + n, _ => n } }";
    let mut sources = SourceDatabase::default();
    let file = sources
        .set("scope.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let snapshot = snapshot(&mut AnalysisDatabase::default(), &sources);
    let analysis = snapshot.file(file).unwrap();
    assert!(
        analysis.result().diagnostics().is_empty(),
        "{:?}",
        analysis.result().diagnostics()
    );
    let initializer = text.find("n + 1").unwrap();
    let outer = analysis.definition_at(initializer).unwrap();
    assert_eq!(
        &text[outer.location.range.start..outer.location.range.end],
        "n"
    );
    assert_eq!(outer.location.range.start, text.find("var n").unwrap() + 4);
    let visible = analysis.visible_bindings(initializer);
    assert_eq!(
        visible
            .iter()
            .find(|b| b.declaration.name == "n")
            .unwrap()
            .declaration,
        *outer
    );
    let bound = text.find("bound + n").unwrap();
    let pattern = analysis.definition_at(bound).unwrap();
    assert_eq!(
        &text[pattern.location.range.start..pattern.location.range.end],
        "bound"
    );
    let visible = analysis.visible_bindings(bound);
    assert_eq!(
        visible
            .iter()
            .map(|b| b.declaration.name.as_str())
            .collect::<Vec<_>>(),
        ["bound", "n", "value"]
    );
    assert_eq!(visible[0].declaration, *pattern);
    let inner = analysis.definition_at(bound + "bound + ".len()).unwrap();
    assert_ne!(outer.id, inner.id);
    assert_eq!(visible[1].declaration, *inner);
    let sibling = text.find("_ => n").unwrap() + "_ => ".len();
    assert_eq!(
        analysis
            .visible_bindings(sibling)
            .iter()
            .map(|b| b.declaration.name.as_str())
            .collect::<Vec<_>>(),
        ["n", "value"]
    );
    assert_eq!(analysis.definition_at(sibling).unwrap(), inner);
    assert_eq!(pattern.location.file, file);
    assert_eq!(pattern.location.revision, analysis.source().revision());
}

#[test]
fn assignment_navigation_resolves_the_retained_target_and_broken_neighbors_survive() {
    let text = "fn bad() { missing() } fn good(value: i32) -> i32 { var n = value; n += value; n }";
    let mut sources = SourceDatabase::default();
    let file = sources
        .set("target.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let snapshot = snapshot(&mut AnalysisDatabase::default(), &sources);
    let analysis = snapshot.file(file).unwrap();
    assert!(!analysis.result().diagnostics().is_empty());
    assert!(
        analysis
            .definition_at(text.find("missing()").unwrap())
            .is_none()
    );
    let assignment = analysis.definition_at(text.find("n +=").unwrap()).unwrap();
    let read = analysis.definition_at(text.rfind("n }").unwrap()).unwrap();
    assert_eq!(assignment, read);
    let value = analysis
        .definition_at(text.rfind("value;").unwrap())
        .unwrap();
    assert_eq!(
        &text[value.location.range.start..value.location.range.end],
        "value"
    );
    assert_eq!(
        value.location.range.start,
        text.find("good(value").unwrap() + 5
    );
}

#[test]
fn function_scopes_do_not_inherit_module_initialization_bindings() {
    let text = "val outside = 1; struct P { val field: i32 } fn good(value: i32) -> i32 { value }";
    let mut sources = SourceDatabase::default();
    let file = sources
        .set("init-scope.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let snapshot = snapshot(&mut AnalysisDatabase::default(), &sources);
    let analysis = snapshot.file(file).unwrap();
    assert!(
        analysis
            .visible_bindings(text.find("field:").unwrap())
            .is_empty()
    );
    assert!(
        analysis
            .visible_bindings(text.find("good(").unwrap())
            .is_empty()
    );
    let offset = text.rfind("value }").unwrap();
    let bindings = analysis.visible_bindings(offset);
    assert_eq!(
        bindings
            .iter()
            .map(|binding| binding.declaration.name.as_str())
            .collect::<Vec<_>>(),
        ["value"]
    );
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
    let mut db = AnalysisDatabase::default();
    let first = snapshot(&mut db, &sources);
    let function_a = first
        .file(a)
        .unwrap()
        .definition_at(text.rfind("same(1)").unwrap())
        .unwrap()
        .clone();
    let function_b = first
        .file(b)
        .unwrap()
        .definition_at(text.rfind("same(1)").unwrap())
        .unwrap();
    assert_ne!(function_a.id, function_b.id);
    let binding_a = first
        .file(a)
        .unwrap()
        .definition_at(text.find("n }").unwrap())
        .unwrap()
        .clone();
    let DeclarationId::Binding(binding) = &binding_a.id else {
        panic!("local binding")
    };
    let DeclarationId::Definition(owner) = &function_a.id else {
        panic!("function definition")
    };
    assert_eq!(&binding.body, owner);
    assert_eq!(first.declaration(&function_b.id).unwrap().location.file, b);
    let unchanged = snapshot(&mut db, &sources);
    assert_eq!(unchanged.declaration(&binding_a.id), Some(&binding_a));

    let edited = text.replace("val n = value", "val n = value + 1");
    sources
        .set("a.kgr", edited.clone(), SourceLayer::Overlay)
        .unwrap();
    let second = snapshot(&mut db, &sources);
    let new_function = second.declaration(&function_a.id).unwrap();
    assert_ne!(new_function.location.revision, function_a.location.revision);
    assert_eq!(new_function.location.file, function_a.location.file);
    assert!(second.declaration(&binding_a.id).is_none());
    assert_eq!(first.declaration(&binding_a.id), Some(&binding_a));
    let new_binding = second
        .file(a)
        .unwrap()
        .definition_at(edited.find("n }").unwrap())
        .unwrap();
    assert_ne!(new_binding.id, binding_a.id);

    // A different profile is a different semantic analysis, even at the same text revision.
    let profile = LanguageFeatureProfile {
        allow_host_calls: true,
        ..Default::default()
    };
    let third = db
        .snapshot(sources.snapshot(), profile, &Default::default())
        .unwrap();
    assert!(third.declaration(&new_binding.id).is_none());
    assert!(third.declaration(&function_a.id).is_some());
    sources.bind_module("a.kgr", module("renamed")).unwrap();
    let rebound = snapshot(&mut db, &sources);
    assert!(rebound.declaration(&function_a.id).is_none());
    assert!(rebound.declaration(&function_b.id).is_some());
}

#[test]
fn declaration_paths_distinguish_kinds_duplicates_and_method_owners() {
    let text = "struct Same { val n: i32 } fn Same() -> i32 { 1 } fn Same() -> i32 { 2 } trait A { fn get(self) -> i32; } trait B { fn get(self) -> i32; } impl A for Same { fn get(self) -> i32 { self.n } }";
    let source = SourceFile::new("definitions.kgr", text);
    let result = crate::analyze_source(&source, Default::default());
    assert!(!result.diagnostics().is_empty());
    let facts = result.facts();
    let declarations = facts
        .declarations
        .iter()
        .filter(|decl| matches!(decl.id, DeclarationId::Definition(_)))
        .collect::<Vec<_>>();
    let ids = declarations
        .iter()
        .map(|decl| &decl.id)
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(declarations.len(), ids.len());
    assert_eq!(
        declarations
            .iter()
            .filter(|decl| decl.name == "get")
            .count(),
        3
    );
    let same = declarations
        .iter()
        .filter(|decl| decl.name == "Same")
        .map(|decl| {
            let DeclarationId::Definition(id) = &decl.id else {
                unreachable!()
            };
            (id.path[0].kind, id.path[0].occurrence)
        })
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(
        same,
        [
            (DefinitionKind::Struct, 0),
            (DefinitionKind::Function, 0),
            (DefinitionKind::Function, 1)
        ]
        .into_iter()
        .collect()
    );
}
