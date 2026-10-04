use super::*;
use crate::{tests::test_analysis, typeck::table::CallTarget};
use kagari_contract::library::catalog as foundation_catalog;
use kagari_source::diagnostic::DiagnosticKind;

#[test]
fn body_constraints_use_later_arguments_and_local_uses() {
    for body in [
        "val xs = []; xs.push(42);",
        "val xs = Vec::new(); xs.push(42);",
        "val xs = HashSet::new(); xs.insert(42);",
        "val xs = HashMap::new(); xs.insert(1, true);",
        "var xs = []; xs = [42];",
        "consume(Marker { value: 7 }, 1);",
        "val callback = |x| x + 1; callback(41);",
        "apply(|x| x + 1, 41);",
        "val checked: Result<Vec<i32>, String> = Ok([42]);",
        "val checked: Result<Vec<i32>, String> = apply(|x| Ok([x]), 42);",
        "val checked: Option<Result<Vec<i32>, String>> = apply(|x| Some(Ok([x])), 42);",
    ] {
        let source = SourceFile::new(
            "body-inference.kgr",
            format!(
                "use std::collections::{{HashMap, HashSet}}; struct Marker<T> {{ val value: i32 }} fn consume<T>(marker: Marker<T>, seed: T) {{}} fn apply<T, U>(callback: fn(T) -> U, value: T) -> U {{ callback(value) }} fn main() {{ {body} }}"
            ),
        );
        let analysis = crate::analyze_source(&source, foundation_catalog::shared())
            .expect("installed declaration analysis");
        assert!(
            analysis.diagnostics().is_empty(),
            "{body}: {:?}",
            analysis.diagnostics()
        );
        assert!(analysis.into_codegen().is_ok());
    }
}

#[test]
fn unresolved_body_variables_and_conflicting_uses_are_rejected() {
    for body in [
        "val xs = [];",
        "val xs = []; xs.push(1); xs.push(true);",
        "val xs: List<i32> = []; xs.push(1);",
        "val checked: Result<Vec<i32>, bool> = apply(|x| Result<Vec<i32>, String>::Ok([x]), 42);",
        "val checked: Result<i32, String> = apply(|x| Ok([x]), 42);",
    ] {
        let source = SourceFile::new(
            "body-inference-errors.kgr",
            format!(
                "use std::collections::List; fn apply<T,U>(callback: fn(T) -> U, value: T) -> U {{ callback(value) }} fn main() {{ {body} }}"
            ),
        );
        let analysis = crate::analyze_source(&source, foundation_catalog::shared())
            .expect("installed declaration analysis");
        assert!(!analysis.diagnostics().is_empty(), "{body}");
        assert!(analysis.into_codegen().is_err());
    }
}

#[test]
fn declared_bound_inference_follows_unique_nested_implementations() {
    let source = SourceFile::new(
        "nested-bound-inference.kgr",
        r#"
struct Sink<T> { val seed: i32 }
struct Wrap<C> { val inner: C }
trait Accept<T> { fn accept(self, value: T); }
impl<T> Accept<T> for Sink<T> { fn accept(self, value: T) {} }
impl<T, C: Accept<T>> Accept<T> for Wrap<C> {
    fn accept(self, value: T) { self.inner.accept(value); }
}
fn apply<T, C: Accept<T>>(value: T, target: C) { target.accept(value); }
fn main() {
    val sink: Wrap<Sink<Result<i32, String>>> = Wrap { inner: Sink { seed: 0 } };
    apply(Ok(42), sink);
}
"#,
    );
    let analysis = crate::analyze_source(&source, foundation_catalog::shared())
        .expect("installed declaration analysis");
    assert!(
        analysis.diagnostics().is_empty(),
        "{:?}",
        analysis.diagnostics()
    );
    assert!(analysis.into_codegen().is_ok());

    let ambiguous = SourceFile::new(
        "ambiguous-bound-inference.kgr",
        r#"
struct Sink { val seed: i32 }
trait Accept<T> { fn accept(self, value: T); }
impl Accept<i32> for Sink { fn accept(self, value: i32) {} }
impl Accept<bool> for Sink { fn accept(self, value: bool) {} }
fn choose<T, C: Accept<T>>(target: C) -> T { loop {} }
fn main() { val value = choose(Sink { seed: 0 }); }
"#,
    );
    let analysis = crate::analyze_source(&ambiguous, foundation_catalog::shared())
        .expect("installed declaration analysis");
    assert!(analysis.diagnostics().iter().any(|diagnostic| matches!(
        diagnostic.kind,
        DiagnosticKind::CannotInferGenericArgument { .. }
    )));
    assert!(analysis.into_codegen().is_err());
}

#[test]
fn explicit_enum_arguments_check_units_payloads_and_constraints() {
    for (body, valid) in [
        ("val value = Token<i32>::Empty;", true),
        ("val value = Token<bool>::Empty();", true),
        ("val value = Token<i32>::Data(7);", true),
        ("val value = Token<i32>::Data(false);", false),
        ("val value = Token<>::Empty;", false),
        ("val value = Token<i32, bool>::Empty;", false),
        ("val value = Token<Missing>::Empty;", false),
        ("val value = Key<f32>::Empty;", false),
        ("val value = Token<HashMap<f32, bool>>::Empty;", false),
        ("val value: Token<bool> = Token<i32>::Empty;", false),
        ("val value = Token<i32>::Missing;", false),
        ("val value = HashMap<i32>::new();", false),
    ] {
        let source = SourceFile::new(
            "explicit-enum.kgr",
            format!(
                "use std::collections::HashMap; use std::hash::{{Hash}};\nenum Token<T> {{ Empty, Data(T) }} enum Key<T: Eq + Hash> {{ Empty }} fn main() {{ {body} }}"
            ),
        );
        let analysis = crate::analyze_source(&source, foundation_catalog::shared())
            .expect("installed declaration analysis");
        assert_eq!(
            analysis.diagnostics().is_empty(),
            valid,
            "{body}: {:?}",
            analysis.diagnostics()
        );
        assert_eq!(analysis.into_codegen().is_ok(), valid, "{body}");
    }
}

#[test]
fn nominal_and_call_constraints_share_recursive_comparable_binders() {
    let source = SourceFile::new(
        "shared-bounds.kgr",
        "struct Key<T: PartialEq> { val value: i32 } fn consume<T: PartialEq>(value: T) {} fn make<T: PartialEq>(value: T) -> Key<(T, i32)> { consume((value, 7)); Key { value: 42 } } fn main() -> i32 { make(true).value }",
    );
    let analysis = crate::analyze_source(&source, foundation_catalog::shared())
        .expect("installed declaration analysis");
    assert!(
        analysis.diagnostics().is_empty(),
        "{:?}",
        analysis.diagnostics()
    );
    assert!(analysis.into_codegen().is_ok());
    let unconstrained = SourceFile::new(
        "missing-bound.kgr",
        "struct Key<T: PartialEq> { val value: i32 } fn consume<T: PartialEq>(value: T) {} fn make<T>(value: T) -> Key<(T, i32)> { consume((value, 7)); Key { value: 42 } }",
    );
    let analysis = crate::analyze_source(&unconstrained, foundation_catalog::shared())
        .expect("installed declaration analysis");
    assert!(analysis.diagnostics().iter().any(|diagnostic| matches!(
        diagnostic.kind,
        DiagnosticKind::GenericBoundNotSatisfied { .. }
    )));
    assert!(analysis.into_codegen().is_err());
}

#[test]
fn partial_nominal_arguments_check_known_outer_standard_constraints() {
    for (bound, argument, expected_constraints) in [
        ("Eq + Hash", "[Missing]", 0),
        ("Eq + Hash", "Missing", 0),
        ("OrderedNumber", "[Missing]", 1),
        ("SignedNumber", "HashMap<i32, Missing>", 1),
        ("Iterable", "[Missing]", 0),
        ("Iterable", "(Missing, i32)", 1),
        ("PartialEq", "(Missing, i32)", 0),
    ] {
        let source = SourceFile::new(
            "partial-bound.kgr",
            format!(
                "use std::collections::HashMap; use std::hash::Hash; struct Restricted<T: {bound}> {{ val value: i32 }} fn take(value: Restricted<{argument}>) {{}}"
            ),
        );
        let analysis = crate::analyze_source(&source, foundation_catalog::shared())
            .expect("installed declaration analysis");
        let constraints = analysis
            .diagnostics()
            .iter()
            .filter(|diagnostic| {
                matches!(
                    diagnostic.kind,
                    DiagnosticKind::StandardConstraintNotSatisfied { .. }
                        | DiagnosticKind::GenericBoundNotSatisfied { .. }
                )
            })
            .count();
        assert_eq!(
            constraints,
            expected_constraints,
            "{bound}: {argument}: {:?}",
            analysis.diagnostics()
        );
        assert!(analysis.into_codegen().is_err());
    }
}

#[test]
fn partial_annotations_preserve_independent_container_constraint_errors() {
    for template in [
        "struct Item { val field: TYPE }",
        "enum Item { Data(TYPE) }",
        "fn take(value: TYPE) {}",
        "fn make() -> TYPE { HashMap::new() }",
        "const value: TYPE = 0;",
        "fn main() { val value: TYPE = HashMap::new(); }",
        "struct Marker<T> { val value: i32 } fn main() { Marker<TYPE> { value: 7 }; }",
    ] {
        for (annotation, expected_constraints) in [
            ("HashMap<f32, Missing>", 1),
            ("HashMap<Missing, i32>", 0),
            ("HashMap<i32, (Missing, HashSet<f32>)>", 1),
            ("HashMap<HashMap<f32, Missing>, i32>", 1),
        ] {
            let text = format!(
                "use std::collections::{{HashMap, HashSet}}; {}",
                template.replace("TYPE", annotation)
            );
            let source = SourceFile::new("partial-constraints.kgr", text.clone());
            let analysis = crate::analyze_source(&source, foundation_catalog::shared())
                .expect("installed declaration analysis");
            let constraints = analysis
                .diagnostics()
                .iter()
                .filter(|diagnostic| {
                    matches!(
                        diagnostic.kind,
                        DiagnosticKind::StandardConstraintNotSatisfied { .. }
                    )
                })
                .count();
            assert_eq!(
                constraints,
                expected_constraints,
                "{text}: {:?}",
                analysis.diagnostics()
            );
            assert!(analysis.into_codegen().is_err(), "{text}");
        }
    }
}

#[test]
fn explicit_constructor_type_queries_survive_body_reuse() {
    let text = "fn neighbor() -> i32 { 1 } struct Marker<T> { val value: i32 } enum Token<T> { Empty } fn make() { Marker<bool> { value: 7 }; Token<bool>::Empty; }";
    let mut sources = SourceDatabase::default();
    let root = sources
        .set("explicit-reuse.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut db = test_analysis();
    let old = db
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    old.check_program(root, &Default::default()).unwrap();
    let changed = text.replace("{ 1 }", "{ 2 + 3 }");
    sources
        .set("explicit-reuse.kgr", changed.clone(), SourceLayer::Base)
        .unwrap();
    let new = db
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    new.check_program(root, &Default::default()).unwrap();
    assert_eq!(
        new.file(root).unwrap().result().facts().typed.reused_bodies,
        1
    );
    for (snapshot, source) in [(&old, text), (&new, changed.as_str())] {
        for (offset, _) in source.match_indices("bool") {
            assert_eq!(
                snapshot.file(root).unwrap().type_at(offset),
                Some(TypeId::Builtin(BuiltinType::Bool))
            );
        }
    }
}

#[test]
fn explicit_struct_arguments_check_identity_arity_bounds_and_fields() {
    for (body, valid) in [
        ("val value = Marker<i32> { value: 7 };", true),
        ("val value = Marker<HashMap<i32, bool>> { value: 7 };", true),
        (
            "val value = Marker<HashMap<f32, bool>> { value: 7 };",
            false,
        ),
        ("val value = Marker<> { value: 7 };", false),
        ("val value = Marker<i32, bool> { value: 7 };", false),
        ("val value = Marker<Missing> { value: 7 };", false),
        ("val value = Marker<i32> { value: false };", false),
        ("val value: Marker<bool> = Marker<i32> { value: 7 };", false),
        ("val value = Key<f32> { value: 7 };", false),
    ] {
        let source = SourceFile::new(
            "explicit-constructor.kgr",
            format!(
                "use std::collections::HashMap; use std::hash::{{Hash}};\nstruct Marker<T> {{ val value: i32 }} struct Key<T: Eq + Hash> {{ val value: i32 }} fn main() {{ {body} }}"
            ),
        );
        let analysis = crate::analyze_source(&source, foundation_catalog::shared())
            .expect("installed declaration analysis");
        assert_eq!(
            analysis.diagnostics().is_empty(),
            valid,
            "{body}: {:?}",
            analysis.diagnostics()
        );
        assert_eq!(analysis.into_codegen().is_ok(), valid);
    }
}

#[test]
fn earlier_constructor_members_supply_context_to_later_members() {
    for (body, valid) in [
        (
            "fn main() { Fixed { marker: Marker { value: 7 }, seed: 1 }; }",
            true,
        ),
        (
            "fn main() { Bundle { seed: 1, marker: Marker { value: 7 } }; }",
            true,
        ),
        (
            "fn main() { Packet::Data(true, Marker { value: 7 }); }",
            true,
        ),
        (
            "fn forward<T>(seed: T) { Bundle { seed: seed, marker: Marker { value: 7 } }; Packet::Data(seed, Marker { value: 7 }); }",
            true,
        ),
        (
            "fn main() { Bundle { seed: 1, marker: Marker { value: false } }; }",
            false,
        ),
        (
            "fn main() { Packet::Data(true, Marker { value: false }); }",
            false,
        ),
    ] {
        let source = SourceFile::new(
            "member-context.kgr",
            format!(
                "struct Marker<T> {{ val value: i32 }} struct Bundle<T> {{ val seed: T, val marker: Marker<T> }} struct Fixed<T> {{ val marker: Marker<bool>, val seed: T }} enum Packet<T> {{ Data(T, Marker<T>) }} {body}"
            ),
        );
        let analysis = crate::analyze_source(&source, foundation_catalog::shared())
            .expect("installed declaration analysis");
        assert_eq!(
            analysis.diagnostics().is_empty(),
            valid,
            "{body}: {:?}",
            analysis.diagnostics()
        );
        assert_eq!(analysis.into_codegen().is_ok(), valid);
    }
}

#[test]
fn local_container_annotations_enforce_the_same_key_bounds_as_signatures() {
    for (source, valid) in [
        (
            "use std::collections::HashMap; fn main() { val value: HashMap<f32, i32> = HashMap::new(); }",
            false,
        ),
        (
            "use std::collections::{HashSet};\nfn main() { val value: HashSet<f32> = HashSet::new(); }",
            false,
        ),
        (
            "use std::collections::HashMap; fn main() { val value: Vec<HashMap<f32, i32>> = []; }",
            false,
        ),
        (
            "use std::collections::{HashSet};\nfn make<T>() { val value: HashSet<T> = HashSet::new(); }",
            false,
        ),
        (
            "use std::collections::{HashSet};\nuse std::hash::{Hash};\nfn make<T: Eq + Hash>() { val value: HashSet<T> = HashSet::new(); }",
            true,
        ),
        (
            "use std::collections::HashMap; fn main() { val value: HashMap<i32, bool> = HashMap::new(); }",
            true,
        ),
    ] {
        let analysis = crate::analyze_source(
            &SourceFile::new("key-context.kgr", source),
            foundation_catalog::shared(),
        )
        .expect("installed declaration analysis");
        assert_eq!(
            analysis.diagnostics().is_empty(),
            valid,
            "{source}: {:?}",
            analysis.diagnostics()
        );
        if !valid {
            assert!(analysis.diagnostics().iter().any(|d| matches!(
                d.kind,
                DiagnosticKind::StandardConstraintNotSatisfied { .. }
            )));
        }
        assert_eq!(analysis.into_codegen().is_ok(), valid);
    }
}

#[test]
fn empty_container_context_is_shared_by_all_expression_positions() {
    for body in [
        "fn make() -> [i32] { [] }",
        "use std::collections::HashMap; fn make() -> HashMap<i32, bool> { HashMap::new() }",
        "use std::collections::{HashSet};\nfn make() -> HashSet<i32> { HashSet::new() }",
        "use std::collections::{HashMap, HashSet};\nfn take(values: Vec<i32>, map: HashMap<i32, bool>, set: HashSet<i32>) {} fn main() { take([], HashMap::new(), HashSet::new()); }",
        "use std::collections::{HashMap, HashSet};\nstruct Values { val array: Vec<i32>, val map: HashMap<i32, bool>, val set: HashSet<i32> } fn main() { Values { array: [], map: HashMap::new(), set: HashSet::new() }; }",
        "use std::collections::HashMap; fn main() { var map: HashMap<i32, bool> = HashMap::new(); map = HashMap::new(); }",
    ] {
        let source = SourceFile::new("empty-context.kgr", body);
        let analysis = crate::analyze_source(&source, foundation_catalog::shared())
            .expect("installed declaration analysis");
        assert!(
            analysis.diagnostics().is_empty(),
            "{body}: {:?}",
            analysis.diagnostics()
        );
        assert!(analysis.into_codegen().is_ok());
    }
    for source in [
        "use std::collections::HashMap; fn make() -> HashMap<i32, bool> { HashMap::new(1) }",
        "use std::collections::{HashMap, HashSet};\nfn make() -> HashMap<i32, bool> { HashSet::new() }",
        "fn make() -> [i32] { [true] }",
    ] {
        let analysis = crate::analyze_source(
            &SourceFile::new("invalid-empty-context.kgr", source),
            foundation_catalog::shared(),
        )
        .expect("installed declaration analysis");
        assert!(analysis.into_codegen().is_err(), "{source}");
    }
}

#[test]
fn assignment_context_uses_checked_target_types_without_bypassing_writeability() {
    for (body, valid) in [
        (
            "var value: Marker<i32> = Marker { value: 1 }; value = Marker { value: 2 };",
            true,
        ),
        (
            "val value: Box = Box { marker: Marker { value: 1 } }; value.marker = Marker { value: 2 };",
            true,
        ),
        (
            "val values: Vec<Marker<i32>> = [Marker { value: 1 }]; values[0] = Marker { value: 2 };",
            true,
        ),
        (
            "var value: Marker<i32> = Marker { value: 1 }; value = Marker { value: true };",
            false,
        ),
        (
            "val value: Marker<i32> = Marker { value: 1 }; value = Marker { value: 2 };",
            false,
        ),
        (
            "var value: Token<i32> = Token::Empty; value = Token::Empty();",
            true,
        ),
    ] {
        let source = SourceFile::new(
            "assignment-context.kgr",
            format!(
                "struct Marker<T> {{ val value: i32 }} struct Box {{ var marker: Marker<i32> }} enum Token<T> {{ Empty }} fn main() {{ {body} }}"
            ),
        );
        let analysis = crate::analyze_source(&source, foundation_catalog::shared())
            .expect("installed declaration analysis");
        assert_eq!(
            analysis.diagnostics().is_empty(),
            valid,
            "{body}: {:?}",
            analysis.diagnostics()
        );
        assert_eq!(analysis.into_codegen().is_ok(), valid);
    }
}

#[test]
fn caller_owned_binders_are_context_but_uninferred_callee_binders_are_not() {
    for (body, valid) in [
        (
            "fn relay<T>(seed: T) { consume(seed, Marker { value: 7 }); }",
            true,
        ),
        (
            "fn relay<T>() -> Marker<T> { identity(Marker { value: 7 }) }",
            true,
        ),
        (
            "fn recursive<T>(seed: T, marker: Marker<T>) { recursive(seed, Marker { value: 7 }); }",
            true,
        ),
        (
            "fn relay<T>(seed: T) { unseeded(Marker { value: 7 }); }",
            false,
        ),
    ] {
        let source = SourceFile::new(
            "binder-context.kgr",
            format!(
                "struct Marker<T> {{ val value: i32 }} fn consume<T>(seed: T, value: Marker<T>) {{}} fn unseeded<T>(value: Marker<T>) {{}} fn identity<T>(value: T) -> T {{ value }} {body}"
            ),
        );
        let analysis = crate::analyze_source(&source, foundation_catalog::shared())
            .expect("installed declaration analysis");
        assert_eq!(
            analysis.diagnostics().is_empty(),
            valid,
            "{body}: {:?}",
            analysis.diagnostics()
        );
        assert_eq!(analysis.into_codegen().is_ok(), valid);
    }
}

#[test]
fn generic_calls_use_result_context_and_preceding_arguments() {
    for (body, valid) in [
        (
            "val marker: Marker<i32> = identity(Marker { value: 7 });",
            true,
        ),
        ("val token: Token<bool> = empty();", true),
        ("consume(1, Marker { value: 7 });", true),
        (
            "val marker: Marker<i32> = identity(Marker { value: true });",
            false,
        ),
        ("val marker: Marker<i32> = identity(7);", false),
        ("val token = empty();", false),
    ] {
        let source = SourceFile::new(
            "generic-context.kgr",
            format!(
                "struct Marker<T> {{ val value: i32 }} enum Token<T> {{ Empty }} fn identity<T>(value: T) -> T {{ value }} fn empty<T>() -> Token<T> {{ Token::Empty }} fn consume<T>(seed: T, marker: Marker<T>) {{}} fn main() {{ {body} }}"
            ),
        );
        let analysis = crate::analyze_source(&source, foundation_catalog::shared())
            .expect("installed declaration analysis");
        assert_eq!(
            analysis.diagnostics().is_empty(),
            valid,
            "{body}: {:?}",
            analysis.diagnostics()
        );
        assert_eq!(analysis.into_codegen().is_ok(), valid);
    }
}

#[test]
fn trait_parameter_context_keeps_targets_through_invalid_payloads() {
    for (arguments, valid) in [
        ("Marker { value: 7 }, Token::Empty", true),
        ("Marker { value: true }, Token::Empty", false),
        ("Marker { value: 7 }", false),
        ("Marker { value: 7 }, Token::Empty, missing", false),
    ] {
        let source = SourceFile::new(
            "trait-context.kgr",
            format!(
                "struct Marker<T> {{ val value: i32 }} enum Token<T> {{ Empty }} trait Take {{ fn take(self, marker: Marker<i32>, token: Token<bool>) -> i32; }} fn invoke<T: Take>(value: T) -> i32 {{ value.take({arguments}) }}"
            ),
        );
        let analysis = crate::analyze_source(&source, foundation_catalog::shared())
            .expect("installed declaration analysis");
        assert_eq!(
            analysis.diagnostics().is_empty(),
            valid,
            "{arguments}: {:?}",
            analysis.diagnostics()
        );
        let facts = analysis.facts();
        assert!(facts.lowered.module.body.expressions().any(|(id, _)| {
            facts
                .typed
                .type_table
                .call_resolution(id)
                .is_some_and(|call| matches!(call.target, CallTarget::TraitMethod { .. }))
        }));
        assert_eq!(analysis.into_codegen().is_ok(), valid);
    }
}

#[test]
fn concrete_call_parameters_supply_constructor_context_and_keep_errors() {
    for (body, valid) in [
        ("take(Marker { value: 7 }, Token::Empty);", true),
        ("take(Marker { value: true }, Token::Empty);", false),
        ("take(Marker { value: 7 });", false),
        ("take(Marker { value: 7 }, Token::Empty, missing);", false),
        (
            "val take = 1; take(Marker { value: 7 }, Token::Empty);",
            false,
        ),
    ] {
        let source = SourceFile::new(
            "call-context.kgr",
            format!(
                "struct Marker<T> {{ val value: i32 }} enum Token<T> {{ Empty }} fn take(value: Marker<i32>, token: Token<bool>) {{}} fn main() {{ {body} }}"
            ),
        );
        let analysis = crate::analyze_source(&source, foundation_catalog::shared())
            .expect("installed declaration analysis");
        assert_eq!(
            analysis.diagnostics().is_empty(),
            valid,
            "{body}: {:?}",
            analysis.diagnostics()
        );
        assert_eq!(analysis.into_codegen().is_ok(), valid);
    }
}

#[test]
fn enum_context_resolves_unit_variants_and_nested_payload_constructors() {
    for (body, valid) in [
        ("fn make() -> Packet<i32> { Packet::Empty }", true),
        ("fn make() -> Packet<i32> { Packet::Empty() }", true),
        (
            "fn make<T>() -> Packet<T> { Packet::Data(Marker { value: 7 }) }",
            true,
        ),
        (
            "fn make() { val value: Packet<bool> = Packet::Data(Marker { value: 7 }); }",
            true,
        ),
        ("fn make() { val value = Packet::Empty; }", false),
        ("fn make() -> Packet<i32> { Packet::Empty(7) }", false),
        (
            "fn make() -> Packet<i32> { Packet::Data(Marker { value: true }) }",
            false,
        ),
        ("fn make() -> Packet<i32> { Other::Empty }", false),
    ] {
        let source = SourceFile::new(
            "enum-context.kgr",
            format!(
                "struct Marker<T> {{ val value: i32 }} enum Packet<T> {{ Empty, Data(Marker<T>) }} enum Other<T> {{ Empty }} {body}"
            ),
        );
        let analysis = crate::analyze_source(&source, foundation_catalog::shared())
            .expect("installed declaration analysis");
        assert_eq!(
            analysis.diagnostics().is_empty(),
            valid,
            "{body}: {:?}",
            analysis.diagnostics()
        );
        assert_eq!(analysis.into_codegen().is_ok(), valid);
    }
}

#[test]
fn return_context_reaches_control_flow_and_composite_constructors() {
    for body in [
        "fn build() -> Marker<i32> { Marker { value: 7 } }",
        "fn build<T>() -> Marker<T> { Marker { value: 7 } }",
        "fn build() -> Marker<i32> { if true { return Marker { value: 7 }; }; Marker { value: 8 } }",
        "fn build() -> Marker<i32> { if true { Marker { value: 7 } } else { Marker { value: 8 } } }",
        "fn build() -> Marker<i32> { match 0 { 0 => Marker { value: 7 }, _ => Marker { value: 8 } } }",
        "fn build() -> (Marker<i32>, [Marker<bool>]) { (Marker { value: 7 }, [Marker { value: 8 }]) }",
        "fn build() { val result: Marker<i32> = if true { Marker { value: 7 } } else { Marker { value: 8 } }; }",
    ] {
        let source = SourceFile::new(
            "return-context.kgr",
            format!("struct Marker<T> {{ val value: i32 }} {body}"),
        );
        let analysis = crate::analyze_source(&source, foundation_catalog::shared())
            .expect("installed declaration analysis");
        assert!(
            analysis.diagnostics().is_empty(),
            "{body}: {:?}",
            analysis.diagnostics()
        );
        assert!(analysis.into_codegen().is_ok());
    }
}

#[test]
fn annotated_struct_constructors_infer_phantom_parameters_and_check_fields() {
    for (body, valid) in [
        ("val marker: Marker<i32> = Marker { value: 7 };", true),
        (
            "val outer: Outer<i32> = Outer { marker: Marker { value: 7 } };",
            true,
        ),
        ("val marker: Marker<i32> = Marker { value: true };", false),
        ("val marker = Marker { value: 7 };", false),
        ("val marker: Other<i32> = Marker { value: 7 };", false),
    ] {
        let source = SourceFile::new(
            "context.kgr",
            format!(
                "struct Marker<T> {{ val value: i32 }} struct Outer<T> {{ val marker: Marker<T> }} struct Other<T> {{ val value: i32 }} fn main() {{ {body} }}"
            ),
        );
        let analysis = crate::analyze_source(&source, foundation_catalog::shared())
            .expect("installed declaration analysis");
        assert_eq!(
            analysis.diagnostics().is_empty(),
            valid,
            "{body}: {:?}",
            analysis.diagnostics()
        );
        assert_eq!(analysis.into_codegen().is_ok(), valid);
    }
}

#[test]
fn associated_output_constrains_unannotated_callback_before_numeric_defaults() {
    for (item, argument, valid) in [
        ("i32", "|item| item + 1", true),
        ("i64", "|item| item + 1", true),
        ("i32", "|item: i64| item + 1", false),
    ] {
        let source = SourceFile::new(
            "associated-callback-inference.kgr",
            format!(
                r#"
trait Source {{ type Item; fn value(self) -> Self::Item; }}
struct Feed {{ val value: {item} }}
impl Source for Feed {{ type Item = {item}; fn value(self) -> {item} {{ self.value }} }}
fn transform<S: Source<Item = T>, T, U>(source: S, callback: fn(T) -> U) -> U {{ callback(source.value()) }}
fn main() -> {item} {{ transform(Feed {{ value: 1 }}, {argument}) }}
"#
            ),
        );
        let analysis = crate::analyze_source(&source, foundation_catalog::shared())
            .expect("installed declaration analysis");
        assert_eq!(
            analysis.diagnostics().is_empty(),
            valid,
            "{:?}",
            analysis.diagnostics()
        );
        assert_eq!(analysis.into_codegen().is_ok(), valid);
    }
}
