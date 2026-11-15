use crate::{AnalyzerQueries, query_tests::test_analysis};
use kagari_common::{identity::DefinitionKind, span::Span};
use kagari_hir::{analysis::*, types::*};
use kagari_hir::{declarations::DeclarationId, resolver::resolved::ResolvedName};
use kagari_source::{
    diagnostic::DiagnosticKind,
    source_database::{SourceDatabase, SourceLayer},
};

fn snapshot(db: &mut AnalysisDatabase, sources: &SourceDatabase) -> AnalysisSnapshot {
    db.snapshot(sources.snapshot(), &Default::default())
        .unwrap()
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
        let range = declaration.source_location().unwrap().range;
        assert_eq!(&text[range.start..range.end], name);
        assert_eq!(range.start, text.find(source).unwrap() + name_offset);
        assert_eq!(file.queries().definition_at(range.start), Some(declaration));
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
        assert_eq!(file.queries().definition_at(offset).unwrap().name, name);
    }
    let keyword = text.find("fn run").unwrap();
    assert!(file.queries().definition_at(keyword).is_none());
    assert!(file.queries().definition_at(keyword + 1).is_none());
    assert!(
        file.queries()
            .definition_at(text.find("{ val local").unwrap())
            .is_none()
    );
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
    let outer = analysis.queries().definition_at(initializer).unwrap();
    assert_eq!(
        &text[outer.source_location().unwrap().range.start
            ..outer.source_location().unwrap().range.end],
        "n"
    );
    assert_eq!(
        outer.source_location().unwrap().range.start,
        text.find("var n").unwrap() + 4
    );
    let visible = analysis.queries().visible_bindings(initializer);
    assert_eq!(
        visible
            .iter()
            .find(|b| b.declaration.name == "n")
            .unwrap()
            .declaration,
        *outer
    );
    let bound = text.find("bound + n").unwrap();
    let pattern = analysis.queries().definition_at(bound).unwrap();
    assert_eq!(
        &text[pattern.source_location().unwrap().range.start
            ..pattern.source_location().unwrap().range.end],
        "bound"
    );
    let visible = analysis.queries().visible_bindings(bound);
    assert_eq!(
        visible
            .iter()
            .map(|b| b.declaration.name.as_str())
            .collect::<Vec<_>>(),
        ["bound", "n", "value"]
    );
    assert_eq!(visible[0].declaration, *pattern);
    let inner = analysis
        .queries()
        .definition_at(bound + "bound + ".len())
        .unwrap();
    assert_ne!(outer.id, inner.id);
    assert_eq!(visible[1].declaration, *inner);
    let sibling = text.find("_ => n").unwrap() + "_ => ".len();
    assert_eq!(
        analysis
            .queries()
            .visible_bindings(sibling)
            .iter()
            .map(|b| b.declaration.name.as_str())
            .collect::<Vec<_>>(),
        ["n", "value"]
    );
    assert_eq!(analysis.queries().definition_at(sibling).unwrap(), inner);
    assert_eq!(pattern.source_location().unwrap().file, file);
    assert_eq!(
        pattern.source_location().unwrap().revision,
        analysis.source().revision()
    );
}

#[test]
fn structural_scopes_preserve_binder_regions_and_noncapturing_shadow_blockers() {
    let text = include_str!("../../tests/fixtures/scope-bindings.kgr");
    let mut sources = SourceDatabase::default();
    let id = sources
        .set("scopes.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let snapshot = snapshot(&mut test_analysis(), &sources);
    let file = snapshot.file(id).unwrap();
    let diagnostics = file.result().diagnostics();
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert!(
        matches!(&diagnostics[0].kind, DiagnosticKind::UnknownName { name } if name == "helper")
    );
    let facts = file.result().facts();
    let index = crate::scope::ScopeIndex::new(&facts.lowered, &facts.names);
    // Every selected lexical expression must agree with the structural projection,
    // including the for iterable, match guard, closure and hoisted local call.
    for (expr, _) in facts.lowered.module.body.expressions() {
        let Some(resolved) = facts.names.expr_resolution(expr) else {
            continue;
        };
        let Some(canonical) = facts.names.binding(&resolved) else {
            continue;
        };
        let offset = facts.lowered.source_map.expr_span(expr).start;
        let visible = index.visible_bindings(offset);
        let selected = visible
            .iter()
            .find(|binding| binding.name == canonical.name)
            .unwrap();
        assert!(
            std::ptr::eq(*selected, canonical),
            "{} at {offset}",
            canonical.name
        );
    }
    let before = index.visible_bindings(text.find("helper()\n    }").unwrap());
    assert!(before.iter().any(
        |binding| matches!(binding.resolved, ResolvedName::Function(_)) && binding.name == "helper"
    ));
    let after = index.visible_bindings(text.rfind("helper()\n    }").unwrap());
    assert!(after.iter().all(|binding| binding.name != "helper"
        && binding.name != "blocked"
        && binding.name != "input"));
    let tail = text.find("match input").unwrap();
    let visible = index.visible_bindings(tail);
    assert!(visible.iter().any(|binding| binding.name == "unused"));
    for hidden in [
        "argument",
        "unused_inner",
        "unused_iteration",
        "branch",
        "unused_branch",
        "repeated",
        "unused_loop",
        "bound",
    ] {
        assert!(
            visible.iter().all(|binding| binding.name != hidden),
            "{hidden}"
        );
    }
    // Unused binders have declaration sites despite having no selected references.
    for name in [
        "unused",
        "unused_inner",
        "unused_iteration",
        "unused_branch",
        "unused_loop",
    ] {
        let site = text.find(&format!("val {name} =")).unwrap() + 4;
        let declaration = file.queries().definition_at(site).unwrap();
        assert_eq!(declaration.name, name);
        assert_eq!(declaration.source_location().unwrap().range.start, site);
    }
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
            .queries()
            .definition_at(text.find("missing()").unwrap())
            .is_none()
    );
    let assignment = analysis
        .queries()
        .definition_at(text.find("n +=").unwrap())
        .unwrap();
    let read = analysis
        .queries()
        .definition_at(text.rfind("n }").unwrap())
        .unwrap();
    assert_eq!(assignment, read);
    let value = analysis
        .queries()
        .definition_at(text.rfind("value;").unwrap())
        .unwrap();
    assert_eq!(
        &text[value.source_location().unwrap().range.start
            ..value.source_location().unwrap().range.end],
        "value"
    );
    assert_eq!(
        value.source_location().unwrap().range.start,
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
        file.queries().member_receiver_type(incomplete),
        Some(TypeId::Struct(_))
    ));
    let broken_binding = file.queries().definition_at(incomplete - 2).unwrap();
    let good_binding = file.queries().definition_at(good_receiver).unwrap();
    assert_ne!(broken_binding.id, good_binding.id);
    assert_eq!(good_binding.name, "p");
    assert_eq!(file.queries().definition_at(good_member).unwrap().name, "x");
    assert!(
        file.queries()
            .definition_at(text.find("missing").unwrap())
            .is_none()
    );
    assert_eq!(
        file.queries()
            .visible_bindings(good_receiver)
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
            .queries()
            .visible_bindings(text.find("field:").unwrap())
            .is_empty()
    );
    assert!(
        analysis
            .queries()
            .visible_bindings(text.find("good(").unwrap())
            .is_empty()
    );
    let offset = text.rfind("value }").unwrap();
    let bindings = analysis.queries().visible_bindings(offset);
    assert_eq!(
        bindings
            .iter()
            .map(|binding| binding.declaration.name.as_str())
            .collect::<Vec<_>>(),
        ["value"]
    );
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
        .queries()
        .definition_at(text.find("show(true)").unwrap())
        .unwrap();
    assert_eq!(method.name, "show");
    assert_eq!(
        method.source_location().unwrap().range.start,
        text.find("show(self").unwrap()
    );
    let receiver = analysis
        .queries()
        .definition_at(text.find("value.show").unwrap())
        .unwrap();
    assert_eq!(receiver.name, "value");
    assert!(
        analysis
            .queries()
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
        .queries()
        .definition_at(text.find("a.x =").unwrap() + 2)
        .unwrap();
    let writable = analysis
        .queries()
        .definition_at(text.find("b.x +=").unwrap() + 2)
        .unwrap();
    let read = analysis
        .queries()
        .definition_at(text.rfind("b.x }").unwrap() + 2)
        .unwrap();
    assert_ne!(readonly.id, writable.id);
    assert_eq!(writable, read);
    assert_eq!(
        readonly.source_location().unwrap().range.start,
        text.find("val x").unwrap() + 4
    );
    assert_eq!(
        writable.source_location().unwrap().range.start,
        text.find("var x").unwrap() + 4
    );
    assert_eq!(
        &text[writable.source_location().unwrap().range.start
            ..writable.source_location().unwrap().range.end],
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
            .queries()
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
    assert_eq!(
        analysis.queries().definition_at(offset).unwrap().name,
        "broken"
    );
    assert_eq!(analysis.queries().type_at(offset), Some(TypeId::Error));
    assert_eq!(
        analysis.queries().type_at(text.find("p.good").unwrap() + 2),
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
        .queries()
        .definition_at(first_text.find("p.x").unwrap() + 2)
        .unwrap();
    let other = first
        .file(b)
        .unwrap()
        .queries()
        .definition_at(first_text.find("p.x").unwrap() + 2)
        .unwrap();
    assert_ne!(x.id, other.id);
    sources
        .set("field-a.kgr", second_text.into(), SourceLayer::Overlay)
        .unwrap();
    let second = snapshot(&mut db, &sources);
    let current = second.declaration(&x.id).unwrap();
    assert_ne!(
        current.source_location().unwrap(),
        x.source_location().unwrap()
    );
    assert_eq!(
        current.source_location().unwrap().range.start,
        second_text.find("val x").unwrap() + 4
    );
    assert_eq!(
        second
            .file(a)
            .unwrap()
            .result()
            .facts()
            .typed
            .reused_bodies(),
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
    assert_eq!(analysis.queries().type_at(missing), Some(TypeId::Error));
    assert!(analysis.queries().definition_at(missing).is_none());
    let later = text.find(", P)").unwrap() + 2;
    let declaration = analysis.queries().definition_at(later).unwrap();
    assert_eq!(declaration.name, "P");
    let DeclarationId::Definition(id) = &declaration.id else {
        panic!("nominal declaration");
    };
    assert_eq!(
        analysis.queries().type_at(later),
        Some(TypeId::Struct(kagari_hir::types::NominalType {
            associated_types: Default::default(),
            declaration: *id,
            arguments: Vec::new()
        }))
    );
    let annotation = text.find("result: P").unwrap() + "result: ".len();
    assert_eq!(
        analysis.queries().definition_at(annotation),
        Some(declaration)
    );
    assert_eq!(
        analysis.queries().type_at(annotation),
        Some(TypeId::Struct(kagari_hir::types::NominalType {
            associated_types: Default::default(),
            declaration: *id,
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
        .queries()
        .definition_at(text.find("value: T").unwrap() + 7)
        .unwrap();
    let other = analysis
        .queries()
        .definition_at(text.rfind("value: T").unwrap() + 7)
        .unwrap();
    assert_ne!(parameter.id, other.id);
    assert_eq!(
        parameter.source_location().unwrap().range.start,
        text.find("<T>").unwrap() + 1
    );
    assert_eq!(
        analysis
            .queries()
            .definition_at(text.find("copy: T").unwrap() + 6),
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
        analysis.queries().type_at(text.find("Missing").unwrap()),
        Some(TypeId::Error)
    );
    let inline = analysis
        .queries()
        .definition_at(text.find("+ Show").unwrap() + 2)
        .unwrap();
    let predicate = analysis
        .queries()
        .definition_at(text.find("where T").unwrap() + 6)
        .unwrap();
    let constraint = analysis
        .queries()
        .definition_at(text.rfind("T: Show").unwrap() + 3)
        .unwrap();
    assert_eq!(inline.id, constraint.id);
    assert_eq!(inline.name, "Show");
    assert_eq!(
        predicate.source_location().unwrap().range.start,
        text.find("<T").unwrap() + 1
    );
    assert_eq!(
        analysis
            .queries()
            .definition_at(text.find("value.show").unwrap() + 6)
            .unwrap()
            .name,
        "show"
    );
    assert_eq!(
        analysis.queries().type_at(text.find("7 }").unwrap()),
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
        let a = left.queries().type_at(offset).unwrap();
        let b = right.queries().type_at(offset).unwrap();
        assert_eq!(
            a.display_name(left.definitions()),
            b.display_name(right.definitions())
        );
        assert_ne!(a, b);
        let definition = match &a {
            TypeId::Struct(id) | TypeId::Enum(id) | TypeId::Trait(id) => &id.declaration,
            _ => panic!("nominal type"),
        };
        assert_eq!(
            left.queries().definition_at(offset).unwrap().id,
            DeclarationId::Definition(*definition)
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
        .queries()
        .type_at(text.find("value: T").unwrap() + 7)
        .unwrap();
    let second = analysis
        .queries()
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
        .queries()
        .type_at(renamed.find("value: U").unwrap() + 7)
        .unwrap();
    assert_eq!(first, updated);
    assert_eq!(hash(&first), hash(&updated));
    assert_eq!(first.display_name(analysis.definitions()), "T");
    assert_eq!(
        updated.display_name(edited.file(file).unwrap().definitions()),
        "U"
    );
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
    let first = analysis
        .queries()
        .type_at(text.find("-> Self").unwrap() + 3)
        .unwrap();
    let second = analysis
        .queries()
        .type_at(
            text.find("trait B").unwrap()
                + text[text.find("trait B").unwrap()..]
                    .find("-> Self")
                    .unwrap()
                + 3,
        )
        .unwrap();
    let generic = analysis
        .queries()
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
        .queries()
        .definition_at(text.find("value: T").unwrap() + 7)
        .unwrap();
    let method = analysis
        .queries()
        .definition_at(text.find("other: U").unwrap() + 7)
        .unwrap();
    let implementation = analysis
        .queries()
        .definition_at(text.rfind("value: T").unwrap() + 7)
        .unwrap();
    let own = analysis
        .queries()
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
        .queries()
        .definition_at(text.find("[T]").unwrap() + 1)
        .unwrap();
    let argument = analysis
        .queries()
        .definition_at(text.find("value: T").unwrap() + 7)
        .unwrap();
    assert_ne!(target.id, argument.id);
    assert_eq!(
        target.source_location().unwrap().range.start,
        text.find("impl<T").unwrap() + 5
    );
    assert_eq!(
        argument.source_location().unwrap().range.start,
        text.find("apply<T").unwrap() + 6
    );
}
