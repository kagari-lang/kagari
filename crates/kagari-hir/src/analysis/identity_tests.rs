use super::*;
use crate::{declarations::DeclarationId, tests::test_analysis};
use kagari_common::{
    identity::{DefinitionKind, ModuleIdentity, PackageId},
    span::Span,
};
use kagari_source::{
    diagnostic::DiagnosticKind,
    source_database::{SourceDatabase, SourceLayer},
};
use kagari_stdlib::catalog as foundation_catalog;

fn snapshot(db: &mut AnalysisDatabase, sources: &SourceDatabase) -> AnalysisSnapshot {
    db.snapshot(sources.snapshot(), &Default::default())
        .unwrap()
}

#[test]
fn oversized_source_module_identities_are_rejected_without_panicking() {
    let text = "fn run(value: i32) -> i32 { val saved = value; saved }";
    let mut source = SourceFile::new("deep.kgr", text);
    for _ in 1..64 {
        source = SourceFile::inline_module(
            &source,
            "nested",
            text.into(),
            SourceFile::new("child.kgr", "").id(),
            Span::new(0, text.len()),
        );
    }
    assert!(
        crate::analyze_source(&source, foundation_catalog::shared())
            .expect("installed declaration analysis")
            .into_codegen()
            .is_ok()
    );
    source = SourceFile::inline_module(
        &source,
        "overflow",
        text.into(),
        SourceFile::new("child.kgr", "").id(),
        Span::new(0, text.len()),
    );
    let analyzed = crate::analyze_source(&source, foundation_catalog::shared())
        .expect("installed declaration analysis");
    assert!(analyzed.diagnostics().iter().any(|diagnostic| matches!(
        diagnostic.kind,
        DiagnosticKind::CompileLimitExceeded {
            resource: "module identity path segments",
            limit: 64
        }
    )));
    assert!(analyzed.into_codegen().is_err());
    let mut database = SourceDatabase::default();
    assert!(
        database
            .bind_module("deep.kgr", source.module_identity().clone())
            .is_err()
    );
}

#[test]
fn single_source_analysis_uses_installed_declarations_without_replacing_source_identity() {
    let mut sources = SourceDatabase::default();
    let module = ModuleIdentity {
        package: PackageId("application".into()),
        path: vec!["logic".into()],
    };
    let id = sources.bind_module("source.kgr", module.clone()).unwrap();
    sources
        .set("source.kgr", "fn previous() {}".into(), SourceLayer::Base)
        .unwrap();
    sources.set("source.kgr", "use core::option::Option as Maybe; fn identity(value: Maybe<i32>) -> Maybe<i32> { value }".into(), SourceLayer::Overlay).unwrap();
    let files = sources.snapshot();
    let source = files.file(id).unwrap();
    let result = crate::analyze_source(source, foundation_catalog::shared())
        .expect("installed declaration analysis");
    assert!(
        result.diagnostics().is_empty(),
        "{:?}",
        result.diagnostics()
    );
    let facts = result.facts();
    assert_eq!(facts.lowered.source.id(), id);
    assert_eq!(facts.lowered.source.revision(), source.revision());
    assert_eq!(facts.lowered.source.module_identity(), &module);
    let option = facts
        .declarations
        .imported_types()
        .resolved(facts.declarations.resolve_name("Maybe").unwrap())
        .unwrap();
    assert_eq!(option.declaration.name, "Option");
    assert_ne!(option.declaration.location.file, id);
    assert!(option.native_type.is_none());
    let enumeration = facts
        .aggregates
        .enumerations()
        .find(|item| item.declaration.id == option.declaration.id)
        .unwrap();
    assert_eq!(enumeration.declaration, option.declaration);
    assert!(enumeration.id == kagari_types::language::binding::option_declaration());
}

#[test]
fn named_declarations_point_to_identifier_tokens() {
    let text = "// 中文 😀\r\nconst C: i32 = 1; struct S { val x: i32 } enum E { V } trait T { fn f(self); } fn run() {}";
    let mut sources = SourceDatabase::default();
    let id = sources
        .set("declaration-names.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let analyzed = snapshot(&mut test_analysis(), &sources);
    let file = analyzed.file(id).unwrap();
    for (name, source, name_offset) in [
        ("C", "const C", 6),
        ("S", "struct S", 7),
        ("E", "enum E", 5),
        ("T", "trait T", 6),
        ("f", "fn f(self)", 3),
        ("run", "fn run()", 3),
    ] {
        let declaration = file
            .result()
            .facts()
            .declarations
            .iter()
            .find(|declaration| declaration.name == name)
            .unwrap();
        let range = declaration.location.range;
        assert_eq!(&text[range.start..range.end], name);
        assert_eq!(range.start, text.find(source).unwrap() + name_offset);
        assert_eq!(file.definition_at(range.start), Some(declaration));
    }
    let function = file
        .result()
        .facts()
        .lowered
        .module
        .functions
        .iter()
        .find(|function| function.name == "run")
        .unwrap();
    let item = file
        .result()
        .facts()
        .lowered
        .source_map
        .function_span(function.id);
    assert!(item.start < item.end);
    assert!(item.end > text.find("fn run()").unwrap() + "fn run()".len());
}

#[test]
fn declaration_site_navigation_excludes_synthetic_module_span() {
    let text =
        "// 中文 😀\r\nconst top: i32 = 1; fn run<T>(value: T) -> T { val local = value; local }";
    let mut sources = SourceDatabase::default();
    let id = sources
        .set("declaration-sites.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let snapshot = snapshot(&mut test_analysis(), &sources);
    let file = snapshot.file(id).unwrap();
    for (name, source, name_offset) in [
        ("top", "const top", 6),
        ("run", "fn run", 3),
        ("T", "run<T>", 4),
        ("value", "(value: T)", 1),
        ("local", "val local", 4),
    ] {
        let offset = text.find(source).unwrap() + name_offset;
        assert_eq!(file.definition_at(offset).unwrap().name, name);
    }
    let keyword = text.find("fn run").unwrap();
    assert!(file.definition_at(keyword).is_none());
    assert!(file.definition_at(keyword + 1).is_none());
    assert!(
        file.definition_at(text.find("{ val local").unwrap())
            .is_none()
    );
}

#[test]
fn semantic_diagnostic_budget_invalidates_cached_results_without_changing_old_snapshots() {
    let mut sources = SourceDatabase::default();
    let text = "fn bad() { missing_one; missing_two; } fn good() -> i32 { 7 }";
    let id = sources
        .set("diagnostic-budget.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut db = test_analysis();
    let original = snapshot(&mut db, &sources);
    assert!(original.file(id).unwrap().result().diagnostics().len() >= 2);

    db.set_max_semantic_diagnostics(1);
    let limited = snapshot(&mut db, &sources);
    let diagnostics = limited.file(id).unwrap().result().diagnostics();
    assert_eq!(diagnostics.len(), 2);
    assert!(matches!(
        diagnostics[1].kind,
        DiagnosticKind::CompileLimitExceeded {
            resource: "semantic diagnostics",
            limit: 1
        }
    ));
    assert!(limited.check_program(id, &Default::default()).is_err());
    assert!(
        limited
            .file(id)
            .unwrap()
            .result()
            .facts()
            .declarations
            .iter()
            .any(|declaration| declaration.name == "good")
    );
    assert!(original.file(id).unwrap().result().diagnostics().len() >= 2);

    db.set_max_semantic_diagnostics(0);
    let zero = snapshot(&mut db, &sources);
    assert!(matches!(
        zero.file(id).unwrap().result().diagnostics()[0].kind,
        DiagnosticKind::CompileLimitExceeded {
            resource: "semantic diagnostics",
            limit: 0
        }
    ));
}

#[test]
fn scope_queries_and_navigation_share_resolver_shadowing_and_match_bindings() {
    let text = "fn main(value: i32) -> i32 { var n = value; val n = n + 1; match n { bound => bound + n, _ => n } }";
    let mut sources = SourceDatabase::default();
    let file = sources
        .set("scope.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let snapshot = snapshot(&mut test_analysis(), &sources);
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
    let snapshot = snapshot(&mut test_analysis(), &sources);
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
fn r04_recovery_keeps_semantic_targets_but_rejects_codegen() {
    let text = "// 中文 😀\r\nstruct Point { var x: i32 }\r\nfn broken(p: Point) { p.; missing }\r\nfn good(p: Point) -> i32 { p.x }";
    let mut sources = SourceDatabase::default();
    let id = sources
        .set("r04-acceptance.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let snapshot = snapshot(&mut test_analysis(), &sources);
    let file = snapshot.file(id).unwrap();
    let incomplete = text.find("p.;").unwrap() + 2;
    let good_receiver = text.rfind("p.x").unwrap();
    let good_member = good_receiver + 2;
    assert!(matches!(
        file.member_receiver_type(incomplete),
        Some(TypeId::Struct(_))
    ));
    let broken_binding = file.definition_at(incomplete - 2).unwrap();
    let good_binding = file.definition_at(good_receiver).unwrap();
    assert_ne!(broken_binding.id, good_binding.id);
    assert_eq!(good_binding.name, "p");
    assert_eq!(file.definition_at(good_member).unwrap().name, "x");
    assert!(file.definition_at(text.find("missing").unwrap()).is_none());
    assert_eq!(
        file.visible_bindings(good_receiver)
            .iter()
            .map(|binding| binding.declaration.name.as_str())
            .collect::<Vec<_>>(),
        ["p"]
    );
    assert!(file.result().clone().into_codegen().is_err());
}

#[test]
fn function_scopes_do_not_inherit_module_declarations_as_local_bindings() {
    let text =
        "const outside: i32 = 1; struct P { val field: i32 } fn good(value: i32) -> i32 { value }";
    let mut sources = SourceDatabase::default();
    let file = sources
        .set("init-scope.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let snapshot = snapshot(&mut test_analysis(), &sources);
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
    let mut db = test_analysis();
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
    assert_eq!(
        first.definitions().resolve(binding.body).unwrap().to_path(),
        definitions.resolve(*owner).unwrap().to_path()
    );
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

    // Unchanged source and declarations reuse the same semantic identities.

    let third = db
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    assert_eq!(third.declaration(&new_binding.id), Some(new_binding));
    assert!(third.declaration(&function_a.id).is_some());
    sources.bind_module("a.kgr", module("renamed")).unwrap();
    let rebound = snapshot(&mut db, &sources);
    assert!(rebound.declaration(&function_a.id).is_none());
    assert!(rebound.declaration(&function_b.id).is_some());
}

#[test]
fn trait_call_navigation_consumes_checked_method_target_even_with_bad_arguments() {
    let text = "trait Show { fn show(self, n: i32) -> i32; } fn render(value: Show) -> i32 { value.show(true) }";
    let mut sources = SourceDatabase::default();
    let file = sources
        .set("method.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let snapshot = snapshot(&mut test_analysis(), &sources);
    let analysis = snapshot.file(file).unwrap();
    assert!(!analysis.result().diagnostics().is_empty());
    let method = analysis
        .definition_at(text.find("show(true)").unwrap())
        .unwrap();
    assert_eq!(method.name, "show");
    assert_eq!(method.location.range.start, text.find("show(self").unwrap());
    let receiver = analysis
        .definition_at(text.find("value.show").unwrap())
        .unwrap();
    assert_eq!(receiver.name, "value");
    assert!(
        analysis
            .definition_at(text.find("true)").unwrap())
            .is_none()
    );
}

#[test]
fn field_navigation_distinguishes_owners_and_retains_rejected_write_targets() {
    let text = "struct A { val x: i32 } struct B { var x: i32 } fn bad(a: A) { a.x = 2; } fn good(b: B) -> i32 { b.x += 1; b.x }";
    let mut sources = SourceDatabase::default();
    let file = sources
        .set("fields.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let snapshot = snapshot(&mut test_analysis(), &sources);
    let analysis = snapshot.file(file).unwrap();
    assert!(!analysis.result().diagnostics().is_empty());
    let readonly = analysis
        .definition_at(text.find("a.x =").unwrap() + 2)
        .unwrap();
    let writable = analysis
        .definition_at(text.find("b.x +=").unwrap() + 2)
        .unwrap();
    let read = analysis
        .definition_at(text.rfind("b.x }").unwrap() + 2)
        .unwrap();
    assert_ne!(readonly.id, writable.id);
    assert_eq!(writable, read);
    assert_eq!(
        readonly.location.range.start,
        text.find("val x").unwrap() + 4
    );
    assert_eq!(
        writable.location.range.start,
        text.find("var x").unwrap() + 4
    );
    assert_eq!(
        &text[writable.location.range.start..writable.location.range.end],
        "x"
    );
    let DeclarationId::Definition(id) = &writable.id else {
        panic!("field definition")
    };
    let path = analysis.definitions().resolve(*id).unwrap().to_path();
    assert_eq!(path.path[0].name, "B");
    assert_eq!(path.path[1].kind, DefinitionKind::Field);
    assert_eq!(
        analysis
            .definition_at(text.rfind("b.x }").unwrap())
            .unwrap()
            .name,
        "b"
    );
}

#[test]
fn erroneous_field_type_keeps_its_identity_without_unknown_member_cascades() {
    let text = "struct P { val broken: Missing, val good: i32 } fn bad(p: P) { p.broken } fn good(p: P) -> i32 { p.good }";
    let mut sources = SourceDatabase::default();
    let file = sources
        .set("field-type.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let snapshot = snapshot(&mut test_analysis(), &sources);
    let analysis = snapshot.file(file).unwrap();
    assert_eq!(
        analysis.result().diagnostics().len(),
        1,
        "{:?}",
        analysis.result().diagnostics()
    );
    let offset = text.find("p.broken").unwrap() + 2;
    assert_eq!(analysis.definition_at(offset).unwrap().name, "broken");
    assert_eq!(analysis.type_at(offset), Some(TypeId::Error));
    assert_eq!(
        analysis.type_at(text.find("p.good").unwrap() + 2),
        Some(TypeId::Builtin(kagari_types::scalar::BuiltinType::I32))
    );
}

#[test]
fn named_field_identity_survives_slot_reordering_and_remains_module_owned() {
    let first_text = "struct P { val x: i32, val y: i32 } fn read(p: P) -> i32 { p.x }";
    let second_text = "struct P { val y: i32, val x: i32 } fn read(p: P) -> i32 { p.x }";
    let mut sources = SourceDatabase::default();
    let a = sources
        .set("field-a.kgr", first_text.into(), SourceLayer::Base)
        .unwrap();
    let b = sources
        .set("field-b.kgr", first_text.into(), SourceLayer::Base)
        .unwrap();
    let mut db = test_analysis();
    let first = snapshot(&mut db, &sources);
    let x = first
        .file(a)
        .unwrap()
        .definition_at(first_text.find("p.x").unwrap() + 2)
        .unwrap();
    let other = first
        .file(b)
        .unwrap()
        .definition_at(first_text.find("p.x").unwrap() + 2)
        .unwrap();
    assert_ne!(x.id, other.id);
    sources
        .set("field-a.kgr", second_text.into(), SourceLayer::Overlay)
        .unwrap();
    let second = snapshot(&mut db, &sources);
    let current = second.declaration(&x.id).unwrap();
    assert_ne!(current.location, x.location);
    assert_eq!(
        current.location.range.start,
        second_text.find("val x").unwrap() + 4
    );
    assert_eq!(
        second.file(a).unwrap().result().facts().typed.reused_bodies,
        0
    );
    assert_eq!(first.declaration(&x.id), Some(x));
}

#[test]
fn type_navigation_retains_later_tuple_members_and_local_annotations() {
    let text = "struct P { val n: i32 } fn bad() -> (Missing, P) {} fn good(p: P) -> P { val result: P = p; result }";
    let mut sources = SourceDatabase::default();
    let file = sources
        .set("types.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let snapshot = snapshot(&mut test_analysis(), &sources);
    let analysis = snapshot.file(file).unwrap();
    assert!(!analysis.result().diagnostics().is_empty());
    let missing = text.find("Missing").unwrap();
    assert_eq!(analysis.type_at(missing), Some(TypeId::Error));
    assert!(analysis.definition_at(missing).is_none());
    let later = text.find(", P)").unwrap() + 2;
    let declaration = analysis.definition_at(later).unwrap();
    assert_eq!(declaration.name, "P");
    let DeclarationId::Definition(id) = &declaration.id else {
        panic!("nominal declaration");
    };
    assert_eq!(
        analysis.type_at(later),
        Some(TypeId::Struct(crate::types::NominalType {
            associated_types: Default::default(),
            declaration: analysis.definitions().resolve(*id).unwrap().to_path(),
            arguments: Vec::new()
        }))
    );
    let annotation = text.find("result: P").unwrap() + "result: ".len();
    assert_eq!(analysis.definition_at(annotation), Some(declaration));
    assert_eq!(
        analysis.type_at(annotation),
        Some(TypeId::Struct(crate::types::NominalType {
            associated_types: Default::default(),
            declaration: analysis.definitions().resolve(*id).unwrap().to_path(),
            arguments: Vec::new()
        }))
    );
}

#[test]
fn generic_parameter_identity_is_owner_and_position_based() {
    let text = "fn first<T>(value: T) -> T { val copy: T = value; copy } fn second<T>(value: T) -> T { value }";
    let mut sources = SourceDatabase::default();
    let file = sources
        .set("generic.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut db = test_analysis();
    let first = snapshot(&mut db, &sources);
    let analysis = first.file(file).unwrap();
    assert!(
        analysis.result().diagnostics().is_empty(),
        "{:?}",
        analysis.result().diagnostics()
    );
    let parameter = analysis
        .definition_at(text.find("value: T").unwrap() + 7)
        .unwrap();
    let other = analysis
        .definition_at(text.rfind("value: T").unwrap() + 7)
        .unwrap();
    assert_ne!(parameter.id, other.id);
    assert_eq!(
        parameter.location.range.start,
        text.find("<T>").unwrap() + 1
    );
    assert_eq!(
        analysis.definition_at(text.find("copy: T").unwrap() + 6),
        Some(parameter)
    );
    let DeclarationId::GenericParameter { owner, position } = &parameter.id else {
        panic!("generic parameter")
    };
    assert_eq!(*position, 0);
    assert_eq!(
        analysis
            .definitions()
            .resolve(*owner)
            .unwrap()
            .to_path()
            .path
            .last()
            .unwrap()
            .name,
        "first"
    );
    let renamed = text.replacen(
        "first<T>(value: T) -> T { val copy: T",
        "first<U>(value: U) -> U { val copy: U",
        1,
    );
    sources
        .set("generic.kgr", renamed, SourceLayer::Overlay)
        .unwrap();
    let second = snapshot(&mut db, &sources);
    assert_eq!(second.declaration(&parameter.id).unwrap().name, "U");
    assert_eq!(first.declaration(&parameter.id).unwrap().name, "T");
}

#[test]
fn bound_navigation_retains_valid_references_beside_unknown_constraints() {
    let text = "trait Show { fn show(self) -> i32; } fn broken<T: Missing + Show>(value: T) -> i32 where T: Show { value.show() } fn good() -> i32 { 7 }";
    let mut sources = SourceDatabase::default();
    let file = sources
        .set("bounds.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let snapshot = snapshot(&mut test_analysis(), &sources);
    let analysis = snapshot.file(file).unwrap();
    assert_eq!(
        analysis.result().diagnostics().len(),
        1,
        "{:?}",
        analysis.result().diagnostics()
    );
    let diagnostic = &analysis.result().diagnostics()[0];
    assert_eq!(diagnostic.kind.code(), "KG_TYPE_UNKNOWN_TRAIT");
    assert_eq!(
        diagnostic.span,
        Some(Span::new(
            text.find("Missing").unwrap(),
            text.find("Missing").unwrap() + 7
        ))
    );
    assert_eq!(
        analysis.type_at(text.find("Missing").unwrap()),
        Some(TypeId::Error)
    );
    let inline = analysis
        .definition_at(text.find("+ Show").unwrap() + 2)
        .unwrap();
    let predicate = analysis
        .definition_at(text.find("where T").unwrap() + 6)
        .unwrap();
    let constraint = analysis
        .definition_at(text.rfind("T: Show").unwrap() + 3)
        .unwrap();
    assert_eq!(inline.id, constraint.id);
    assert_eq!(inline.name, "Show");
    assert_eq!(predicate.location.range.start, text.find("<T").unwrap() + 1);
    assert_eq!(
        analysis
            .definition_at(text.find("value.show").unwrap() + 6)
            .unwrap()
            .name,
        "show"
    );
    assert_eq!(
        analysis.type_at(text.find("7 }").unwrap()),
        Some(TypeId::Builtin(kagari_types::scalar::BuiltinType::I32))
    );
}

#[test]
fn same_spelled_nominal_types_in_different_modules_are_distinct() {
    let text = "struct P { val n: i32 } enum E { A } trait Show { fn show(self) -> i32; } fn inspect(p: P, e: E, s: Show) {}";
    let mut sources = SourceDatabase::default();
    let left = sources
        .set("left.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let right = sources
        .set("right.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let snapshot = snapshot(&mut test_analysis(), &sources);
    let left = snapshot.file(left).unwrap();
    let right = snapshot.file(right).unwrap();
    assert!(
        left.result().diagnostics().is_empty(),
        "{:?}",
        left.result().diagnostics()
    );
    assert!(
        right.result().diagnostics().is_empty(),
        "{:?}",
        right.result().diagnostics()
    );
    for annotation in ["p: P", "e: E", "s: Show"] {
        let offset = text.find(annotation).unwrap() + 3;
        let a = left.type_at(offset).unwrap();
        let b = right.type_at(offset).unwrap();
        assert_eq!(a.display_name(), b.display_name());
        assert_ne!(a, b);
        let definition = match &a {
            TypeId::Struct(id) | TypeId::Enum(id) | TypeId::Trait(id) => &id.declaration,
            _ => panic!("nominal type"),
        };
        assert_eq!(
            left.definition_at(offset).unwrap().id,
            DeclarationId::Definition(left.definitions().lookup(definition).unwrap())
        );
    }
}

#[test]
fn generic_type_equality_and_hash_use_owner_and_position() {
    use std::hash::{Hash, Hasher};
    let hash = |ty: &TypeId| {
        let mut state = std::collections::hash_map::DefaultHasher::new();
        ty.hash(&mut state);
        state.finish()
    };
    let text = "fn first<T>(value: T) -> T { value } fn second<T>(value: T) -> T { value }";
    let mut sources = SourceDatabase::default();
    let file = sources
        .set("generic-types.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut database = test_analysis();
    let original = snapshot(&mut database, &sources);
    let analysis = original.file(file).unwrap();
    let first = analysis
        .type_at(text.find("value: T").unwrap() + 7)
        .unwrap();
    let second = analysis
        .type_at(text.rfind("value: T").unwrap() + 7)
        .unwrap();
    assert_ne!(first, second);
    let renamed = text.replacen("first<T>(value: T) -> T", "first<U>(value: U) -> U", 1);
    sources
        .set("generic-types.kgr", renamed.clone(), SourceLayer::Overlay)
        .unwrap();
    let edited = snapshot(&mut database, &sources);
    let updated = edited
        .file(file)
        .unwrap()
        .type_at(renamed.find("value: U").unwrap() + 7)
        .unwrap();
    assert_eq!(first, updated);
    assert_eq!(hash(&first), hash(&updated));
    assert_eq!(first.display_name(), "T");
    assert_eq!(updated.display_name(), "U");
}

#[test]
fn implicit_self_types_belong_to_their_trait() {
    let text = "trait A { fn copy(self) -> Self; } trait B { fn copy(self) -> Self; } fn identity<Self>(value: Self) -> Self { value }";
    let mut sources = SourceDatabase::default();
    let file = sources
        .set("self-types.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let snapshot = snapshot(&mut test_analysis(), &sources);
    let analysis = snapshot.file(file).unwrap();
    assert!(
        analysis.result().diagnostics().is_empty(),
        "{:?}",
        analysis.result().diagnostics()
    );
    let first = analysis.type_at(text.find("-> Self").unwrap() + 3).unwrap();
    let second = analysis
        .type_at(
            text.find("trait B").unwrap()
                + text[text.find("trait B").unwrap()..]
                    .find("-> Self")
                    .unwrap()
                + 3,
        )
        .unwrap();
    let generic = analysis
        .type_at(text.find("value: Self").unwrap() + 7)
        .unwrap();
    assert!(matches!(first, TypeId::SelfType(_)));
    assert!(matches!(second, TypeId::SelfType(_)));
    assert!(matches!(generic, TypeId::Generic(_)));
    assert_ne!(first, second);
    assert_ne!(first, generic);
    let TypeId::SelfType(owner) = &first else {
        unreachable!();
    };
    let concrete = TypeId::Builtin(kagari_types::scalar::BuiltinType::I32);
    let nested = TypeId::Tuple(vec![first.clone(), second.clone(), generic.clone()]);
    assert_eq!(
        nested.with_self(owner, &concrete),
        TypeId::Tuple(vec![concrete, second, generic])
    );
}

#[test]
fn inherited_generic_parameters_keep_the_trait_or_impl_owner() {
    let text = "struct P { val n: i32 } trait Source<T> { fn map<U>(self, value: T, other: U) -> T; } impl<T> P { fn apply<U>(self, value: T, other: U) -> T { value } }";
    let mut sources = SourceDatabase::default();
    let file = sources
        .set("owners.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let snapshot = snapshot(&mut test_analysis(), &sources);
    let analysis = snapshot.file(file).unwrap();
    assert!(
        analysis.result().diagnostics().is_empty(),
        "{:?}",
        analysis.result().diagnostics()
    );
    let inherited = analysis
        .definition_at(text.find("value: T").unwrap() + 7)
        .unwrap();
    let method = analysis
        .definition_at(text.find("other: U").unwrap() + 7)
        .unwrap();
    let implementation = analysis
        .definition_at(text.rfind("value: T").unwrap() + 7)
        .unwrap();
    let own = analysis
        .definition_at(text.rfind("other: U").unwrap() + 7)
        .unwrap();
    for (declaration, kind, position) in [
        (inherited, DefinitionKind::Trait, 0),
        (method, DefinitionKind::Method, 0),
        (implementation, DefinitionKind::Impl, 0),
        (own, DefinitionKind::Method, 0),
    ] {
        let DeclarationId::GenericParameter {
            owner,
            position: actual,
        } = &declaration.id
        else {
            panic!("generic owner")
        };
        assert_eq!(
            analysis
                .definitions()
                .resolve(*owner)
                .unwrap()
                .to_path()
                .path
                .last()
                .unwrap()
                .kind,
            kind
        );
        assert_eq!(*actual, position);
    }
    assert_ne!(method.id, own.id);
}

#[test]
fn implicit_receiver_keeps_impl_type_context_when_method_shadows_a_generic() {
    let text = "impl<T> [T] { fn apply<T>(self, value: T) -> T { value } }";
    let mut sources = SourceDatabase::default();
    let file = sources
        .set("impl-context.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let snapshot = snapshot(&mut test_analysis(), &sources);
    let analysis = snapshot.file(file).unwrap();
    assert!(
        analysis.result().diagnostics().is_empty(),
        "{:?}",
        analysis.result().diagnostics()
    );
    let target = analysis
        .definition_at(text.find("[T]").unwrap() + 1)
        .unwrap();
    let argument = analysis
        .definition_at(text.find("value: T").unwrap() + 7)
        .unwrap();
    assert_ne!(target.id, argument.id);
    assert_eq!(
        target.location.range.start,
        text.find("impl<T").unwrap() + 5
    );
    assert_eq!(
        argument.location.range.start,
        text.find("apply<T").unwrap() + 6
    );
}

#[test]
fn declaration_paths_distinguish_kinds_duplicates_and_method_owners() {
    let text = "struct Same { val n: i32 } fn Same() -> i32 { 1 } fn Same() -> i32 { 2 } trait A { fn get(self) -> i32; } trait B { fn get(self) -> i32; } impl A for Same { fn get(self) -> i32 { self.n } }";
    let source = SourceFile::new("definitions.kgr", text);
    let result = crate::analyze_source(&source, foundation_catalog::shared())
        .expect("installed declaration analysis");
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

#[test]
fn checked_analysis_retains_compact_named_and_generic_metadata() {
    use crate::types::semantic::lower_type;
    use kagari_common::identity::mapping::DefinitionMappingError;
    use kagari_types::ty::Ty;

    let source = SourceFile::new(
        "compact.kgr",
        "struct Player { var hp: i32 } fn identity<T>(value: T) -> T { value }",
    );
    let first = crate::analyze_source(&source, foundation_catalog::shared())
        .expect("installed declaration analysis")
        .into_codegen()
        .unwrap();
    let second = crate::analyze_source(&source, foundation_catalog::shared())
        .expect("installed declaration analysis")
        .into_codegen()
        .unwrap();
    let function = first
        .typed
        .functions
        .iter()
        .find(|function| function.name == "identity")
        .unwrap();
    let other = second
        .typed
        .functions
        .iter()
        .find(|function| function.name == "identity")
        .unwrap();
    let owner = function.generic_params[0].owner;
    assert_ne!(owner, other.generic_params[0].owner);
    assert!(second.definitions().resolve(owner).is_err());
    assert_eq!(
        first.definitions().resolve(owner).unwrap().to_path(),
        second
            .definitions()
            .resolve(other.generic_params[0].owner)
            .unwrap()
            .to_path()
    );
    assert_eq!(
        lower_type(&function.return_type),
        Ty::Parameter { owner, position: 0 }
    );
    let field = first
        .declarations
        .iter()
        .find(|declaration| declaration.name == "hp")
        .unwrap();
    let DeclarationId::Definition(field) = field.id else {
        panic!("field definition");
    };
    let field_path = first.definitions().resolve(field).unwrap().to_path();
    assert_eq!(
        field_path
            .path
            .iter()
            .map(|segment| segment.name.as_str())
            .collect::<Vec<_>>(),
        ["Player", "hp"]
    );
    let cancelled = CancellationToken::default();
    cancelled.cancel();
    assert!(matches!(
        first.to_unverified(&cancelled),
        Err(DefinitionMappingError::Cancelled)
    ));
    let authoring = first.to_unverified(&Default::default()).unwrap();
    assert_eq!(
        authoring
            .typed
            .functions
            .iter()
            .find(|function| function.name == "identity")
            .unwrap()
            .generic_params[0]
            .owner,
        first.definitions().resolve(owner).unwrap().to_path()
    );
    drop(source);
    drop(second);
    assert_eq!(
        first.definitions().resolve(field).unwrap().to_path(),
        field_path
    );
}

#[test]
fn independently_constructed_hir_types_observe_conversion_bounds() {
    use crate::types::TypeId;
    use kagari_common::identity::{
        mapping::{DefinitionMappingError, DefinitionRecord},
        metadata::scope_record,
    };
    let mut ty = TypeId::Builtin(kagari_types::scalar::BuiltinType::I32);
    for _ in 0..65 {
        ty = TypeId::Tuple(vec![ty]);
    }
    assert!(matches!(
        scope_record(&ty, &Default::default()),
        Err(DefinitionMappingError::LimitExceeded)
    ));
    let cancelled = CancellationToken::default();
    cancelled.cancel();
    assert!(matches!(
        ty.visit_definitions(&mut |_| Ok(()), &cancelled),
        Err(DefinitionMappingError::Cancelled)
    ));
}

#[test]
fn published_cache_scopes_retain_prefixes_and_reject_foreign_queries() {
    use kagari_common::identity::{mapping::DefinitionRecord, table::DefinitionId};
    let text = "struct Player { var hp: i32 } fn identity<T>(value: T) -> T { value } fn read(p: Player) -> i32 { p.hp }";
    let mut sources = SourceDatabase::default();
    let file = sources
        .set("cache-scopes.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut database = test_analysis();
    let first = snapshot(&mut database, &sources);
    let analysis = first.file(file).unwrap().clone();
    let signature = first.signature_snapshot().file(file).unwrap().clone();
    let declaration = analysis
        .result()
        .facts()
        .declarations
        .iter()
        .find(|d| d.name == "read")
        .unwrap();
    let DeclarationId::Definition(owner) = declaration.id else {
        panic!("function owner");
    };
    let path = analysis.definitions().resolve(owner).unwrap().to_path();
    let body = database
        .body(sources.snapshot(), &owner, &Default::default())
        .unwrap()
        .unwrap();
    fn validate<T: DefinitionRecord<DefinitionId>>(records: &T, definitions: &DefinitionTable) {
        records
            .visit_definitions(
                &mut |id| {
                    definitions.resolve(*id)?;
                    Ok(())
                },
                &Default::default(),
            )
            .unwrap();
    }
    validate(analysis.result(), analysis.definitions());
    validate(signature.signatures().as_ref(), signature.definitions());
    validate(body.type_table(), body.definitions());
    assert!(Arc::ptr_eq(analysis.signatures(), signature.signatures()));
    let mut independent = test_analysis();
    assert!(
        independent
            .body(sources.snapshot(), &owner, &Default::default())
            .is_err()
    );
    let independent_body = independent
        .body(sources.snapshot(), &path, &Default::default())
        .unwrap()
        .unwrap();
    assert_ne!(*independent_body.owner(), owner);
    sources
        .set(
            "cache-scopes.kgr",
            format!("{text} fn added() {{}}"),
            SourceLayer::Overlay,
        )
        .unwrap();
    let second = snapshot(&mut database, &sources);
    assert!(second.definitions().resolve(owner).is_ok());
    drop(database);
    drop(first);
    drop(second);
    drop(sources);
    assert_eq!(
        body.definitions().resolve(*body.owner()).unwrap().to_path(),
        path
    );
    assert_eq!(
        analysis.definitions().resolve(owner).unwrap().to_path(),
        path
    );
    validate(signature.signatures().as_ref(), signature.definitions());
}
