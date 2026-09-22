use super::*;
use crate::types::{BuiltinType, TypeId};
use kagari_common::source_database::{SourceDatabase, SourceLayer};

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
        ("val value = Token<Map<f32, bool>>::Empty;", false),
        ("val value: Token<bool> = Token<i32>::Empty;", false),
        ("val value = Token<i32>::Missing;", false),
        ("val value = std::map<i32>::new();", false),
    ] {
        let source = SourceFile::new(
            "explicit-enum.kgr",
            format!(
                "enum Token<T> {{ Empty, Data(T) }} enum Key<T: HashKey> {{ Empty }} fn main() {{ {body} }}"
            ),
        );
        let analysis = crate::analyze_source(&source, Default::default());
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
        "struct Key<T: Comparable> { val value: i32 } fn consume<T: Comparable>(value: T) {} fn make<T: Comparable>(value: T) -> Key<(T, i32)> { consume((value, 7)); Key { value: 42 } } fn main() -> i32 { make(true).value }",
    );
    let analysis = crate::analyze_source(&source, Default::default());
    assert!(
        analysis.diagnostics().is_empty(),
        "{:?}",
        analysis.diagnostics()
    );
    assert!(analysis.into_codegen().is_ok());
    let unconstrained = SourceFile::new(
        "missing-bound.kgr",
        "struct Key<T: Comparable> { val value: i32 } fn consume<T: Comparable>(value: T) {} fn make<T>(value: T) -> Key<(T, i32)> { consume((value, 7)); Key { value: 42 } }",
    );
    let analysis = crate::analyze_source(&unconstrained, Default::default());
    assert!(analysis.diagnostics().iter().any(|diagnostic| matches!(
        diagnostic.kind,
        kagari_common::DiagnosticKind::StandardConstraintNotSatisfied { .. }
    )));
    assert!(analysis.into_codegen().is_err());
}

#[test]
fn partial_nominal_arguments_check_known_outer_standard_constraints() {
    for (bound, argument, expected_constraints) in [
        ("HashKey", "[Missing]", 1),
        ("HashKey", "Missing", 0),
        ("OrderedNumber", "[Missing]", 1),
        ("SignedNumber", "Map<i32, Missing>", 1),
        ("Iterable", "[Missing]", 0),
        ("Iterable", "(Missing, i32)", 1),
        ("Comparable", "(Missing, i32)", 0),
    ] {
        let source = SourceFile::new(
            "partial-bound.kgr",
            format!(
                "struct Restricted<T: {bound}> {{ val value: i32 }} fn take(value: Restricted<{argument}>) {{}}"
            ),
        );
        let analysis = crate::analyze_source(&source, Default::default());
        let constraints = analysis
            .diagnostics()
            .iter()
            .filter(|diagnostic| {
                matches!(
                    diagnostic.kind,
                    kagari_common::DiagnosticKind::StandardConstraintNotSatisfied { .. }
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
        "fn make() -> TYPE { std::map::new() }",
        "const value: TYPE = 0;",
        "fn main() { val value: TYPE = std::map::new(); }",
        "struct Marker<T> { val value: i32 } fn main() { Marker<TYPE> { value: 7 }; }",
    ] {
        for (annotation, expected_constraints) in [
            ("Map<f32, Missing>", 1),
            ("Map<Missing, i32>", 0),
            ("Map<i32, (Missing, Set<f32>)>", 1),
            ("Map<Map<f32, Missing>, i32>", 2),
        ] {
            let text = template.replace("TYPE", annotation);
            let source = SourceFile::new("partial-constraints.kgr", text.clone());
            let analysis = crate::analyze_source(&source, Default::default());
            let constraints = analysis
                .diagnostics()
                .iter()
                .filter(|diagnostic| {
                    matches!(
                        diagnostic.kind,
                        kagari_common::DiagnosticKind::StandardConstraintNotSatisfied { .. }
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
    let mut db = AnalysisDatabase::default();
    let old = db
        .snapshot(sources.snapshot(), Default::default(), &Default::default())
        .unwrap();
    old.check_program(root, &Default::default()).unwrap();
    let changed = text.replace("{ 1 }", "{ 2 + 3 }");
    sources
        .set("explicit-reuse.kgr", changed.clone(), SourceLayer::Base)
        .unwrap();
    let new = db
        .snapshot(sources.snapshot(), Default::default(), &Default::default())
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
        ("val value = Marker<Map<i32, bool>> { value: 7 };", true),
        ("val value = Marker<Map<f32, bool>> { value: 7 };", false),
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
                "struct Marker<T> {{ val value: i32 }} struct Key<T: HashKey> {{ val value: i32 }} fn main() {{ {body} }}"
            ),
        );
        let analysis = crate::analyze_source(&source, Default::default());
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
        let analysis = crate::analyze_source(&source, Default::default());
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
            "fn main() { val value: Map<f32, i32> = std::map::new(); }",
            false,
        ),
        (
            "fn main() { val value: Set<f32> = std::set::new(); }",
            false,
        ),
        ("fn main() { val value: [Map<f32, i32>] = []; }", false),
        (
            "fn make<T>() { val value: Set<T> = std::set::new(); }",
            false,
        ),
        (
            "fn make<T: HashKey>() { val value: Set<T> = std::set::new(); }",
            true,
        ),
        (
            "fn main() { val value: Map<i32, bool> = std::map::new(); }",
            true,
        ),
    ] {
        let analysis = crate::analyze_source(
            &SourceFile::new("key-context.kgr", source),
            Default::default(),
        );
        assert_eq!(
            analysis.diagnostics().is_empty(),
            valid,
            "{source}: {:?}",
            analysis.diagnostics()
        );
        if !valid {
            assert!(analysis.diagnostics().iter().any(|d| matches!(
                d.kind,
                kagari_common::DiagnosticKind::StandardConstraintNotSatisfied { .. }
            )));
        }
        assert_eq!(analysis.into_codegen().is_ok(), valid);
    }
}

#[test]
fn empty_container_context_is_shared_by_all_expression_positions() {
    for body in [
        "fn make() -> [i32] { [] }",
        "fn make() -> Map<i32, bool> { std::map::new() }",
        "fn make() -> Set<i32> { std::set::new() }",
        "fn take(values: [i32], map: Map<i32, bool>, set: Set<i32>) {} fn main() { take([], std::map::new(), std::set::new()); }",
        "struct Values { val array: [i32], val map: Map<i32, bool>, val set: Set<i32> } fn main() { Values { array: [], map: std::map::new(), set: std::set::new() }; }",
        "fn main() { var map: Map<i32, bool> = std::map::new(); map = std::map::new(); }",
    ] {
        let source = SourceFile::new("empty-context.kgr", body);
        let analysis = crate::analyze_source(&source, Default::default());
        assert!(
            analysis.diagnostics().is_empty(),
            "{body}: {:?}",
            analysis.diagnostics()
        );
        assert!(analysis.into_codegen().is_ok());
    }
    for source in [
        "fn make() -> Map<i32, bool> { std::map::new(1) }",
        "fn make() -> Map<i32, bool> { std::set::new() }",
        "fn make() -> [i32] { [true] }",
    ] {
        let analysis = crate::analyze_source(
            &SourceFile::new("invalid-empty-context.kgr", source),
            Default::default(),
        );
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
            "val values: [Marker<i32>] = [Marker { value: 1 }]; values[0] = Marker { value: 2 };",
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
        let analysis = crate::analyze_source(&source, Default::default());
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
        let analysis = crate::analyze_source(&source, Default::default());
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
        let analysis = crate::analyze_source(&source, Default::default());
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
        let analysis = crate::analyze_source(&source, Default::default());
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
                .is_some_and(|call| {
                    matches!(call.target, crate::typeck::CallTarget::TraitMethod(_))
                })
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
        let analysis = crate::analyze_source(&source, Default::default());
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
        let analysis = crate::analyze_source(&source, Default::default());
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
        let analysis = crate::analyze_source(&source, Default::default());
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
        let analysis = crate::analyze_source(&source, Default::default());
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
fn branch_and_array_merges_recover_complementary_member_facts() {
    for (expression, array, conflict) in [
        (
            "if true { (missing, true) } else { (1, missing) }",
            false,
            false,
        ),
        (
            "match 0 { 0 => (missing, true), _ => (1, missing) }",
            false,
            false,
        ),
        ("[(missing, true), (1, missing)]", true, false),
        ("[(missing, true), (1, missing), (false, true)]", true, true),
    ] {
        let text = format!("fn bad() {{ val combined = {expression}; combined; }}");
        let mut sources = SourceDatabase::default();
        let root = sources
            .set("merge.kgr", text.clone(), SourceLayer::Base)
            .unwrap();
        let mut db = AnalysisDatabase::default();
        let snapshot = db
            .snapshot(sources.snapshot(), Default::default(), &Default::default())
            .unwrap();
        let file = snapshot.file(root).unwrap();
        let pair = TypeId::Tuple(vec![
            TypeId::Builtin(BuiltinType::I32),
            TypeId::Builtin(BuiltinType::Bool),
        ]);
        let expected = if array {
            TypeId::Array(Box::new(pair))
        } else {
            pair
        };
        assert_eq!(
            file.type_at(text.rfind("combined").unwrap()),
            Some(expected),
            "{expression}"
        );
        assert_eq!(
            file.result().diagnostics().iter().any(|d| matches!(
                d.kind,
                kagari_common::DiagnosticKind::ArrayElementTypeMismatch { .. }
                    | kagari_common::DiagnosticKind::IfBranchTypeMismatch { .. }
                    | kagari_common::DiagnosticKind::MatchArmTypeMismatch { .. }
            )),
            conflict,
            "{expression}"
        );
        assert!(snapshot.check_program(root, &Default::default()).is_err());
    }
}

#[test]
fn recovery_members_do_not_hide_independent_argument_mismatches() {
    for (actual, mismatch) in [
        ("(missing, true)", true),
        ("(missing, 2)", false),
        ("(true, missing)", true),
        ("(1, missing)", false),
    ] {
        let source = SourceFile::new(
            "partial-conflict.kgr",
            format!("fn take(value: (i32, i32)) {{}} fn bad() {{ take({actual}); }}"),
        );
        let analysis = crate::analyze_source(&source, Default::default());
        assert_eq!(
            analysis.diagnostics().iter().any(|d| matches!(
                d.kind,
                kagari_common::DiagnosticKind::ArgumentTypeMismatch { .. }
            )),
            mismatch,
            "{actual}: {:?}",
            analysis.diagnostics()
        );
        assert!(analysis.into_codegen().is_err());
    }
}

#[test]
fn indexing_partial_composites_preserves_the_selected_member() {
    for (text, expected, invalid_index) in [
        (
            "fn bad() { (7, missing)[0]; }",
            TypeId::Builtin(BuiltinType::I32),
            false,
        ),
        (
            "fn bad(value: (i32, Missing)) { value[0]; }",
            TypeId::Builtin(BuiltinType::I32),
            false,
        ),
        (
            "fn bad(value: [(i32, Missing)]) { value[0][0]; }",
            TypeId::Builtin(BuiltinType::I32),
            false,
        ),
        (
            "fn bad(value: (i32, Missing)) { value[1]; }",
            TypeId::Error,
            false,
        ),
        (
            "fn bad(value: (i32, Missing)) { value[2]; }",
            TypeId::Error,
            true,
        ),
        (
            "fn bad(value: (i32, Missing)) { value[true]; }",
            TypeId::Error,
            true,
        ),
    ] {
        let source = SourceFile::new("index-recovery.kgr", text);
        let analysis = crate::analyze_source(&source, Default::default());
        let facts = analysis.facts();
        let ty = facts
            .lowered
            .module
            .body
            .expressions()
            .filter(|(_, expr)| matches!(expr.kind, crate::hir::ExprKind::Index { .. }))
            .max_by_key(|(id, _)| {
                let span = facts.lowered.source_map.expr_span(*id);
                span.end - span.start
            })
            .and_then(|(id, _)| facts.typed.type_table.expr_type(id));
        assert_eq!(ty, Some(expected), "{text}");
        assert_eq!(
            analysis.diagnostics().iter().any(|d| matches!(
                d.kind,
                kagari_common::DiagnosticKind::InvalidIndexTarget { .. }
            )),
            invalid_index,
            "{text}"
        );
        assert!(analysis.into_codegen().is_err(), "{text}");
    }
}

#[test]
fn composite_annotations_retain_structure_without_authorizing_codegen() {
    for annotation in [
        "(i32, Missing)",
        "[Missing]",
        "Map<i32, Missing>",
        "Cell<Missing>",
    ] {
        for declaration in [
            format!("fn bad(value: {annotation}) {{}}"),
            format!("fn bad() -> {annotation} {{}}"),
            format!("struct Bad {{ val value: {annotation} }}"),
            format!("enum Bad {{ Value({annotation}) }}"),
            format!("fn bad() {{ val value: {annotation} = (); }}"),
            format!("const bad: {annotation} = ();"),
        ] {
            let text =
                format!("struct Cell<T> {{ val value: T }} {declaration} fn good() -> i32 {{ 7 }}");
            let mut sources = SourceDatabase::default();
            let root = sources
                .set("annotations.kgr", text.clone(), SourceLayer::Base)
                .unwrap();
            let mut db = AnalysisDatabase::default();
            let snapshot = db
                .snapshot(sources.snapshot(), Default::default(), &Default::default())
                .unwrap();
            let file = snapshot.file(root).unwrap();
            assert!(!file.result().diagnostics().is_empty(), "{declaration}");
            let ty = file.type_at(text.find(annotation).unwrap()).unwrap();
            assert!(ty.is_unresolved(), "{declaration}: {ty:?}");
            assert!(
                !matches!(ty, TypeId::Error | TypeId::Unknown),
                "{declaration}: {ty:?}"
            );
            assert_eq!(
                file.type_at(text.rfind('7').unwrap()),
                Some(TypeId::Builtin(BuiltinType::I32))
            );
            assert!(
                snapshot.check_program(root, &Default::default()).is_err(),
                "{declaration}"
            );
        }
    }
}

#[test]
fn failed_call_inference_substitutes_error_without_leaking_callee_binders() {
    for argument in ["", "missing"] {
        let text = format!(
            "struct Cell<T> {{ val value: T }} fn wrap<T>(value: T) -> Cell<T> {{ Cell {{ value: value }} }} fn bad() {{ val cell = wrap({argument}); cell.value; }} fn good() -> i32 {{ 7 }}"
        );
        let source = SourceFile::new("recovery.kgr", text);
        let analysis = crate::analyze_source(&source, Default::default());
        assert!(!analysis.diagnostics().is_empty());
        let facts = analysis.facts();
        let call = facts
            .lowered
            .module
            .body
            .expressions()
            .find_map(|(id, expr)| {
                matches!(expr.kind, crate::hir::ExprKind::Call { .. }).then_some(id)
            })
            .unwrap();
        let TypeId::Struct(result) = facts.typed.type_table.expr_type(call).unwrap() else {
            panic!("known nominal return type must survive inference failure");
        };
        assert_eq!(result.arguments, [TypeId::Error]);
        let field = facts
            .lowered
            .module
            .body
            .expressions()
            .find_map(|(id, expr)| {
                matches!(expr.kind, crate::hir::ExprKind::Field { .. }).then_some(id)
            })
            .unwrap();
        assert_eq!(facts.typed.type_table.expr_type(field), Some(TypeId::Error));
        assert!(facts.typed.type_table.expr_field(field).is_some());
        assert!(analysis.into_codegen().is_err());
    }
}

#[test]
fn partial_call_inference_preserves_known_and_caller_owned_arguments() {
    let source = SourceFile::new(
        "partial.kgr",
        "fn pair<A, B>(a: A, b: B) -> (A, B) { (a, b) } fn bad<T>(value: T) { pair(value); } fn concrete() { pair(1); }",
    );
    let analysis = crate::analyze_source(&source, Default::default());
    let facts = analysis.facts();
    let calls: Vec<_> = facts
        .lowered
        .module
        .body
        .expressions()
        .filter(|(_, expr)| matches!(expr.kind, crate::hir::ExprKind::Call { .. }))
        .map(|(id, _)| facts.typed.type_table.expr_type(id).unwrap())
        .collect();
    assert_eq!(calls.len(), 2);
    let TypeId::Tuple(arguments) = &calls[0] else {
        panic!("tuple result");
    };
    let TypeId::Generic(parameter) = &arguments[0] else {
        panic!("caller binder");
    };
    assert_eq!(parameter.name, "T");
    assert_eq!(arguments[1], TypeId::Error);
    assert_eq!(
        calls[1],
        TypeId::Tuple(vec![TypeId::Builtin(BuiltinType::I32), TypeId::Error])
    );
    assert!(analysis.into_codegen().is_err());
}

#[test]
fn inference_uses_valid_members_of_partially_erroneous_arguments() {
    for source in [
        "fn identity<A, B>(value: (A, B)) -> (A, B) { value } fn bad() { identity((1, missing)); }",
        "struct Pair<A, B> { val first: A, val second: B } fn identity<A, B>(value: Pair<A, B>) -> Pair<A, B> { value } fn bad() { identity(Pair { first: 1, second: missing }); }",
        "struct Pair<A, B> { val first: A, val second: B } fn identity<A, B>(value: Pair<A, B>) -> Pair<A, B> { value } fn bad(value: Pair<i32, Missing>) { identity(value); }",
    ] {
        let source = SourceFile::new("partial-members.kgr", source);
        let analysis = crate::analyze_source(&source, Default::default());
        let facts = analysis.facts();
        let result = facts
            .lowered
            .module
            .body
            .expressions()
            .find_map(|(id, expr)| {
                matches!(expr.kind, crate::hir::ExprKind::Call { .. })
                    .then(|| facts.typed.type_table.expr_type(id))
                    .flatten()
            })
            .unwrap();
        let arguments = match result {
            TypeId::Tuple(arguments) => arguments,
            TypeId::Struct(ty) => ty.arguments,
            _ => panic!("known composite result"),
        };
        assert_eq!(
            arguments,
            [TypeId::Builtin(BuiltinType::I32), TypeId::Error]
        );
        assert!(analysis.into_codegen().is_err());
    }
}

#[test]
fn incomplete_whole_type_does_not_poison_later_inference() {
    let source = SourceFile::new(
        "later-argument.kgr",
        "fn choose<T>(first: T, second: T) -> T { second } fn bad() { choose((1, missing), (2, true)); }",
    );
    let analysis = crate::analyze_source(&source, Default::default());
    let facts = analysis.facts();
    let result = facts
        .lowered
        .module
        .body
        .expressions()
        .find_map(|(id, expr)| {
            matches!(expr.kind, crate::hir::ExprKind::Call { .. })
                .then(|| facts.typed.type_table.expr_type(id))
                .flatten()
        });
    assert_eq!(
        result,
        Some(TypeId::Tuple(vec![
            TypeId::Builtin(BuiltinType::I32),
            TypeId::Builtin(BuiltinType::Bool)
        ]))
    );
    assert!(analysis.into_codegen().is_err());
}

#[test]
fn repeated_generic_arguments_merge_partial_types_without_hiding_conflicts() {
    for (arguments, conflict) in [
        ("(1, missing), (missing, true)", false),
        ("(missing, true), (1, missing)", false),
        ("(1, missing), (false, true)", true),
    ] {
        let source = SourceFile::new(
            "repeated.kgr",
            format!(
                "fn choose<T>(first: T, second: T) -> T {{ first }} fn bad() {{ choose({arguments}); }}"
            ),
        );
        let analysis = crate::analyze_source(&source, Default::default());
        let facts = analysis.facts();
        let result = facts
            .lowered
            .module
            .body
            .expressions()
            .find_map(|(id, expr)| {
                matches!(expr.kind, crate::hir::ExprKind::Call { .. })
                    .then(|| facts.typed.type_table.expr_type(id))
                    .flatten()
            });
        assert_eq!(
            result,
            Some(TypeId::Tuple(vec![
                TypeId::Builtin(BuiltinType::I32),
                TypeId::Builtin(BuiltinType::Bool)
            ]))
        );
        assert_eq!(
            analysis.diagnostics().iter().any(|d| matches!(
                d.kind,
                kagari_common::DiagnosticKind::ArgumentTypeMismatch { .. }
            )),
            conflict
        );
        assert!(analysis.into_codegen().is_err());
    }
}

#[test]
fn aggregate_bounds_are_checked_for_annotations_constructors_and_forwarded_parameters() {
    for text in [
        "struct Key<T: HashKey> { val value: T } fn bad(x: Key<f32>) {}",
        "struct Key<T: HashKey> { val value: T } fn bad() { Key { value: 1.5 }; }",
        "enum Key<T: HashKey> { Value(T) } fn bad() { Key::Value(1.5); }",
        "struct Key<T: HashKey> { val value: T } fn bad<T>(value: T) { Key { value: value }; }",
    ] {
        let source = SourceFile::new("bounds.kgr", text);
        let analysis = crate::analyze_source(&source, Default::default());
        assert!(
            analysis
                .diagnostics()
                .iter()
                .any(|d| d.kind.code() == "KG_TYPE_STANDARD_CONSTRAINT_NOT_SATISFIED"),
            "{text}: {:?}",
            analysis.diagnostics()
        );
        assert!(analysis.into_codegen().is_err());
    }
    let source = SourceFile::new(
        "bounds.kgr",
        "struct Key<T: HashKey> { val value: T } enum Items<T: HashKey> { Values(Set<T>) } fn pass<T: HashKey>(value: T) -> Key<T> { Key { value: value } }",
    );
    let analysis = crate::analyze_source(&source, Default::default());
    assert!(
        analysis.diagnostics().is_empty(),
        "{:?}",
        analysis.diagnostics()
    );
}

#[test]
fn generic_templates_construct_and_project_distinct_instances() {
    let text = "struct Cell<T> { var value: T } enum Packet<T> { Data(T) } fn get<T>(cell: Cell<T>) -> T { cell.value } fn main() -> (i32, String, Packet<i32>) { val a = Cell { value: 7 }; val b = Cell { value: \"hello\" }; a.value = 8; (get(a), get(b), Packet::Data(a.value)) }";
    let source = SourceFile::new("generic.kgr", text);
    let result = crate::analyze_source(&source, Default::default());
    assert!(
        result.diagnostics().is_empty(),
        "{:?}",
        result.diagnostics()
    );
    let facts = result.facts();
    let structure = facts.aggregates.structures().next().unwrap();
    let enumeration = facts.aggregates.enumerations().next().unwrap();
    assert_eq!(structure.generic_params.len(), 1);
    assert_eq!(enumeration.generic_params.len(), 1);
    assert_ne!(structure.generic_params[0], enumeration.generic_params[0]);
    assert_eq!(
        structure.fields[0].ty,
        TypeId::Generic(structure.generic_params[0].clone())
    );
    let instances = facts
        .lowered
        .module
        .body
        .expressions()
        .filter(|(_, expr)| matches!(expr.kind, crate::hir::ExprKind::StructInit { .. }))
        .map(|(id, _)| facts.typed.type_table.expr_type(id).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(instances.len(), 2);
    assert_ne!(instances[0], instances[1]);
    for (instance, expected) in instances
        .iter()
        .zip([BuiltinType::I32, BuiltinType::String])
    {
        let TypeId::Struct(instance) = instance else {
            panic!("struct instance");
        };
        assert_eq!(instance.arguments, [TypeId::Builtin(expected)]);
        assert_eq!(instance.declaration, structure.id);
    }
}

#[test]
fn generic_type_parameters_navigate_and_rebase_without_losing_their_owner() {
    let text = "fn before() {} struct Cell<T> { val value: T } enum Packet<T> { Data(T) } fn read(x: Cell<i32>) -> i32 { x.value }";
    let mut sources = SourceDatabase::default();
    let id = sources
        .set("generic.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut db = AnalysisDatabase::default();
    let old = db
        .snapshot(sources.snapshot(), Default::default(), &Default::default())
        .unwrap();
    let old_file = old.file(id).unwrap();
    assert!(
        old_file.result().diagnostics().is_empty(),
        "{:?}",
        old_file.result().diagnostics()
    );
    let parameter = old_file
        .definition_at(text.find("value: T").unwrap() + 7)
        .unwrap();
    assert_eq!(
        parameter.location.range.start,
        text.find("Cell<T").unwrap() + 5
    );
    let payload = old_file
        .definition_at(text.find("Data(T").unwrap() + 5)
        .unwrap();
    assert_ne!(payload.id, parameter.id);
    let edit = text.replace("fn before() {}", "fn before() { val n = 1; }");
    sources
        .set("generic.kgr", edit.clone(), SourceLayer::Overlay)
        .unwrap();
    let changed = db
        .snapshot(sources.snapshot(), Default::default(), &Default::default())
        .unwrap();
    let file = changed.file(id).unwrap();
    assert!(file.signatures_reused());
    let new_parameter = file
        .definition_at(edit.find("value: T").unwrap() + 7)
        .unwrap();
    assert_eq!(new_parameter.id, parameter.id);
    assert_eq!(
        new_parameter.location.range.start,
        edit.find("Cell<T").unwrap() + 5
    );
    assert!(matches!(
        file.type_at(edit.rfind("value").unwrap()),
        Some(TypeId::Builtin(BuiltinType::I32))
    ));
}

#[test]
fn generic_parameter_context_preserves_known_members_beside_uninferred_binders() {
    for (body, valid) in [
        ("take((Token::Empty, true));", true),
        ("take((Token<i32>::Empty, true));", true),
        ("take((Token<bool>::Empty, true));", false),
        ("unseeded(Token::Empty);", false),
        ("identity(std::map::new());", false),
        ("identity(std::set::new());", false),
    ] {
        let source = SourceFile::new(
            "partial-parameter-context.kgr",
            format!(
                "enum Token<T> {{ Empty }} fn take<T>(pair: (Token<i32>, T)) {{}} fn unseeded<T>(value: Token<T>) {{}} fn identity<T>(value: T) -> T {{ value }} fn main() {{ {body} }}"
            ),
        );
        let analysis = crate::analyze_source(&source, Default::default());
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
fn constructor_fields_share_partial_argument_context_and_reject_unseeded_members() {
    for (body, valid) in [
        ("val item = Pair { pair: (Token::Empty, true) };", true),
        ("val item = Payload::Pair((Token::Empty, true));", true),
        (
            "val item = Pair { pair: (Token<bool>::Empty, true) };",
            false,
        ),
        (
            "val item = Payload::Pair((Token<bool>::Empty, true));",
            false,
        ),
        (
            "val item = Pair { pair: (Token::Empty, Token::Empty) };",
            false,
        ),
        (
            "val item = Pair { pair: (Token::Empty, std::map::new()) };",
            false,
        ),
    ] {
        let source = SourceFile::new(
            "partial-field-context.kgr",
            format!(
                "enum Token<T> {{ Empty }} struct Pair<T> {{ val pair: (Token<i32>, T) }} enum Payload<T> {{ Pair((Token<i32>, T)) }} fn main() {{ {body} }}"
            ),
        );
        let analysis = crate::analyze_source(&source, Default::default());
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
fn failed_generic_inference_retains_known_members_inside_each_type_argument() {
    for (declaration, initializer, nominal) in [
        (
            "fn identity<T>(value: T) -> T { value }",
            "identity((7, std::map::new()))",
            false,
        ),
        (
            "struct Wrap<T> { val value: T }",
            "Wrap { value: (7, std::map::new()) }",
            true,
        ),
        (
            "enum Wrap<T> { Value(T) }",
            "Wrap::Value((7, std::map::new()))",
            true,
        ),
    ] {
        let source = SourceFile::new(
            "inference-recovery.kgr",
            format!(
                "{declaration} fn broken() {{ val item = {initializer}; item; }} fn good() -> i32 {{ 42 }}"
            ),
        );
        let analysis = crate::analyze_source(&source, Default::default());
        assert!(analysis.diagnostics().iter().any(|diagnostic| matches!(
            diagnostic.kind,
            kagari_common::DiagnosticKind::CannotInferGenericArgument { .. }
        )));
        let facts = analysis.facts();
        let ty = facts.lowered.module.body.expressions().find_map(|(id, expression)| {
            matches!(&expression.kind, crate::hir::ExprKind::Name { name, .. } if name == "item")
                .then(|| facts.typed.type_table.expr_type(id)).flatten()
        }).expect("local reference retains inferred type");
        let argument = if nominal {
            match ty {
                TypeId::Struct(ty) | TypeId::Enum(ty) => ty.arguments[0].clone(),
                _ => panic!("nominal type must survive inference failure"),
            }
        } else {
            ty
        };
        assert_eq!(
            argument,
            TypeId::Tuple(vec![
                TypeId::Builtin(BuiltinType::I32),
                TypeId::Map {
                    key: Box::new(TypeId::Error),
                    value: Box::new(TypeId::Error)
                },
            ]),
            "{initializer}"
        );
        assert!(analysis.into_codegen().is_err());
    }
}

#[test]
fn constructor_mismatch_diagnostics_use_finalized_recovery_substitutions() {
    for (declaration, initializer) in [
        (
            "struct Pair<T> { val first: T, val second: T }",
            "Pair { first: (1, std::map::new()), second: (true, std::map::new()) }",
        ),
        (
            "enum Pair<T> { Values(T, T) }",
            "Pair::Values((1, std::map::new()), (true, std::map::new()))",
        ),
    ] {
        let source = SourceFile::new(
            "finalized-constructor.kgr",
            format!("{declaration} fn bad() {{ val pair = {initializer}; }}"),
        );
        let analysis = crate::analyze_source(&source, Default::default());
        let mismatch = analysis
            .diagnostics()
            .iter()
            .find_map(|diagnostic| match &diagnostic.kind {
                kagari_common::DiagnosticKind::AssignmentTypeMismatch { expected, .. }
                | kagari_common::DiagnosticKind::ArgumentTypeMismatch { expected, .. } => {
                    Some(expected)
                }
                _ => None,
            })
            .expect("known i32/bool conflict survives recovery");
        assert_eq!(mismatch, "(i32, Map<<error>, <error>>)", "{initializer}");
        assert!(analysis.into_codegen().is_err());
    }
}

#[test]
fn reflective_writes_share_target_context_and_recovery_member_comparison() {
    for (body, valid, mismatch) in [
        (
            "set_field(box, \"value\", Marker { value: 42 });",
            true,
            false,
        ),
        ("set_index(array, 0, Marker { value: 42 });", true, false),
        (
            "set_field(box, \"value\", Marker<bool> { value: 42 });",
            false,
            true,
        ),
        (
            "set_index(array, 0, Marker<bool> { value: 42 });",
            false,
            true,
        ),
        ("set_field(box, \"pair\", (1, missing));", false, false),
        ("set_field(box, \"pair\", (true, missing));", false, true),
        ("set_index(pairs, 0, (1, missing));", false, false),
        ("set_index(pairs, 0, (true, missing));", false, true),
    ] {
        let source = SourceFile::new(
            "reflective-context.kgr",
            format!(
                "struct Marker<T> {{ val value: i32 }} struct Box {{ var value: Marker<i32>, var pair: (i32, bool) }} fn main() {{ val box = Box {{ value: Marker {{ value: 0 }}, pair: (1, true) }}; val array: [Marker<i32>] = [Marker {{ value: 0 }}]; val pairs = [(1, true)]; {body} }}"
            ),
        );
        let analysis = crate::analyze_source(
            &source,
            crate::LanguageFeatureProfile {
                allow_reflection: true,
                allow_reflection_write: true,
                ..Default::default()
            },
        );
        assert_eq!(
            analysis.diagnostics().is_empty(),
            valid,
            "{body}: {:?}",
            analysis.diagnostics()
        );
        assert_eq!(
            analysis.diagnostics().iter().any(|diagnostic| matches!(
                diagnostic.kind,
                kagari_common::DiagnosticKind::AssignmentTypeMismatch { .. }
            )),
            mismatch,
            "{body}"
        );
        assert_eq!(analysis.into_codegen().is_ok(), valid, "{body}");
    }
}

#[test]
fn standard_arguments_suppress_dependent_errors_but_keep_known_member_conflicts() {
    for (body, mismatch) in [
        ("values.push((1, missing));", false),
        ("values.push((true, missing));", true),
        ("std::array::push(values, (1, missing));", false),
        ("std::array::push(values, (true, missing));", true),
        ("std::array::len(missing);", false),
        ("std::array::len((1, missing));", true),
        ("std::string::contains(missing, \"x\");", false),
        ("std::string::contains(\"x\", missing);", false),
        ("std::string::contains(\"x\", (1, missing));", true),
    ] {
        let source = SourceFile::new(
            "standard-recovery.kgr",
            format!("fn bad() {{ val values = [(1, true)]; {body} }} fn good() -> i32 {{ 42 }}"),
        );
        let analysis = crate::analyze_source(&source, Default::default());
        assert_eq!(
            analysis.diagnostics().len(),
            1 + usize::from(mismatch),
            "{body}: {:?}",
            analysis.diagnostics()
        );
        assert_eq!(
            analysis.diagnostics().iter().any(|d| matches!(
                d.kind,
                kagari_common::DiagnosticKind::ArgumentTypeMismatch { .. }
            )),
            mismatch,
            "{body}"
        );
        assert!(analysis.into_codegen().is_err());
    }
}

#[test]
fn standard_container_operands_supply_constructor_context_in_both_call_forms() {
    for (body, valid) in [
        ("values.push(Marker { value: 7 });", true),
        ("map.get(1).unwrap_or(Marker { value: 7 });", true),
        (
            "std::option::unwrap_or(map.get(1), Marker { value: 7 });",
            true,
        ),
        ("std::array::push(values, Marker { value: 7 });", true),
        ("map.insert(1, Marker { value: 7 });", true),
        ("std::map::insert(map, 1, Marker { value: 7 });", true),
        ("values.push(Marker<bool> { value: 7 });", false),
        (
            "std::map::insert(map, 1, Marker<bool> { value: 7 });",
            false,
        ),
        ("values.push(Marker { value: true });", false),
    ] {
        let source = SourceFile::new(
            "standard-context.kgr",
            format!(
                "struct Marker<T> {{ val value: i32 }} fn main() {{ val values: [Marker<i32>] = []; val map: Map<i32, Marker<i32>> = std::map::new(); {body} }}"
            ),
        );
        let analysis = crate::analyze_source(&source, Default::default());
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
fn standard_set_and_result_context_preserves_concrete_receiver_arguments() {
    for body in [
        "keys.union(std::set::new());",
        "std::set::intersection(keys, std::set::new());",
        "result.unwrap_or(Marker { value: 7 });",
        "std::result::unwrap_or(result, Marker { value: 7 });",
    ] {
        let analysis = crate::analyze_source(
            &SourceFile::new(
                "standard-fallback-context.kgr",
                format!(
                    "struct Marker<T> {{ val value: i32 }} fn check(keys: Set<i32>, result: Result<Marker<i32>, String>) {{ {body} }}"
                ),
            ),
            Default::default(),
        );
        assert!(
            analysis.diagnostics().is_empty(),
            "{body}: {:?}",
            analysis.diagnostics()
        );
        assert!(analysis.into_codegen().is_ok());
    }
}

#[test]
fn boolean_operator_recovery_keeps_result_types_and_known_operand_conflicts() {
    for (expression, mismatch) in [
        ("missing == 1", false),
        ("missing < 1", false),
        ("missing < true", true),
        ("(view, missing) == (view, true)", true),
        ("missing && true", false),
        ("missing || 1", true),
        ("(1, missing) == (1, true)", false),
        ("(1, missing) == (false, true)", true),
        ("(1, missing) < (1, true)", true),
    ] {
        let source = SourceFile::new(
            "operator-recovery.kgr",
            format!(
                "trait View {{}} fn bad(view: View) {{ val result = {expression}; result; }} fn good() -> i32 {{ 42 }}"
            ),
        );
        let analysis = crate::analyze_source(&source, Default::default());
        assert_eq!(
            analysis.diagnostics().len(),
            1 + usize::from(mismatch),
            "{expression}: {:?}",
            analysis.diagnostics()
        );
        let facts = analysis.facts();
        let result = facts
            .lowered
            .module
            .body
            .expressions()
            .find_map(|(id, expr)| {
                matches!(&expr.kind, crate::hir::ExprKind::Name { name, .. } if name == "result")
                    .then(|| facts.typed.type_table.expr_type(id))
                    .flatten()
            });
        assert_eq!(
            result,
            Some(TypeId::Builtin(BuiltinType::Bool)),
            "{expression}"
        );
        assert!(analysis.into_codegen().is_err());
    }
}

#[test]
fn unary_negation_uses_declared_signed_bounds_and_known_recovery_shapes() {
    for (source, valid, unary_error) in [
        (
            "fn negate<T: SignedNumber>(value: T) -> T { -value }",
            true,
            false,
        ),
        (
            "fn negate<T>(value: T) -> T where T: SignedNumber { -value }",
            true,
            false,
        ),
        ("fn negate<T>(value: T) -> T { -value }", false, true),
        (
            "fn negate<T: OrderedNumber>(value: T) -> T { -value }",
            false,
            true,
        ),
        (
            "fn negate<T: SignedNumber>(value: T) -> T { -value } fn call(value: u32) -> u32 { negate(value) }",
            false,
            false,
        ),
        ("fn bad() { -missing; }", false, false),
        ("fn bad() { -(1, missing); }", false, true),
    ] {
        let analysis = crate::analyze_source(
            &SourceFile::new("unary-bounds.kgr", source),
            Default::default(),
        );
        assert_eq!(
            analysis.diagnostics().is_empty(),
            valid,
            "{source}: {:?}",
            analysis.diagnostics()
        );
        assert_eq!(
            analysis.diagnostics().iter().any(|d| matches!(
                d.kind,
                kagari_common::DiagnosticKind::UnaryOperandTypeMismatch { .. }
            )),
            unary_error,
            "{source}"
        );
        assert_eq!(analysis.into_codegen().is_ok(), valid);
    }
}

#[test]
fn standard_math_and_equality_check_each_known_operand_after_recovery() {
    for (body, extra) in [
        ("std::math::min(missing, 7);", 0),
        ("std::math::min(missing, true);", 1),
        ("std::math::clamp(missing, 7, true);", 2),
        (
            "std::debug::assert_eq((1, missing), (1, true), \"test\");",
            0,
        ),
        (
            "std::debug::assert_eq((1, missing), (false, true), \"test\");",
            1,
        ),
        ("std::debug::assert_eq(missing, view, \"test\");", 1),
    ] {
        let analysis = crate::analyze_source(
            &SourceFile::new(
                "standard-operand-recovery.kgr",
                format!("trait View {{}} fn bad(view: View) {{ {body} }}"),
            ),
            Default::default(),
        );
        assert_eq!(
            analysis.diagnostics().len(),
            1 + extra,
            "{body}: {:?}",
            analysis.diagnostics()
        );
        assert!(analysis.into_codegen().is_err());
    }
}

#[test]
fn binary_rhs_uses_left_type_without_overriding_explicit_constructor_arguments() {
    for (body, valid) in [
        ("Token<i32>::Empty == Token::Empty", true),
        ("Token<i32>::Empty != Token::Empty", true),
        ("(Token<i32>::Empty, true) == (Token::Empty, true)", true),
        ("Token<i32>::Empty == Token<bool>::Empty", false),
        ("(Token<i32>::Empty, true) == (Token::Empty, 7)", false),
    ] {
        let source = SourceFile::new(
            "binary-context.kgr",
            format!("enum Token<T> {{ Empty }} fn main() -> bool {{ {body} }}"),
        );
        let analysis = crate::analyze_source(&source, Default::default());
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
fn preceding_array_elements_and_completing_branches_supply_constructor_context() {
    for (body, valid) in [
        ("val values = [Token<i32>::Empty, Token::Empty];", true),
        (
            "val value = if true { Token<i32>::Empty } else { Token::Empty };",
            true,
        ),
        (
            "val value = match true { true => Token<i32>::Empty, false => Token::Empty };",
            true,
        ),
        (
            "val values = [Token<i32>::Empty, Token<bool>::Empty];",
            false,
        ),
        (
            "val value = if true { Token<i32>::Empty } else { Token<bool>::Empty };",
            false,
        ),
        (
            "val value = match true { true => Token<i32>::Empty, false => Token<bool>::Empty };",
            false,
        ),
        (
            "val value = if true { return; } else { Token::Empty };",
            false,
        ),
        (
            "val value = match true { true => { return; }, false => Token::Empty };",
            false,
        ),
    ] {
        let source = SourceFile::new(
            "sequence-context.kgr",
            format!("enum Token<T> {{ Empty }} fn main() {{ {body} }}"),
        );
        let analysis = crate::analyze_source(&source, Default::default());
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
fn terminating_array_members_do_not_contribute_or_enable_later_type_joins() {
    for (body, valid) in [
        (
            "val items = [if true { return 42; } else { return 42; }, true];",
            true,
        ),
        (
            "val items = [7, if true { return 42; } else { return 42; }, true];",
            true,
        ),
        (
            "val items: [i32] = [7, if true { return 42; } else { return 42; }, true];",
            true,
        ),
        (
            "val items = [7, true, if true { return 42; } else { return 42; }];",
            false,
        ),
        (
            "val items = [7, if true { return 42; } else { return 42; }, missing];",
            false,
        ),
    ] {
        let analysis = crate::analyze_source(
            &SourceFile::new(
                "array-completion.kgr",
                format!("fn main() -> i32 {{ {body} 0 }}"),
            ),
            Default::default(),
        );
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
fn terminating_conditions_do_not_require_a_boolean_value_but_keep_operand_errors() {
    for (body, valid) in [
        (
            "if (if true { return 42; } else { return 42; }) { 1; };",
            true,
        ),
        (
            "while (if true { return 42; } else { return 42; }) { 1; }",
            true,
        ),
        ("if (if true { return 42; } else { 7 }) { 1; };", false),
        ("while (if true { return 42; } else { 7 }) { 1; }", false),
        (
            "if (if missing { return 42; } else { return 42; }) { 1; };",
            false,
        ),
    ] {
        let analysis = crate::analyze_source(
            &SourceFile::new(
                "condition-completion.kgr",
                format!("fn main() -> i32 {{ {body} 0 }}"),
            ),
            Default::default(),
        );
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
fn return_values_are_checked_only_when_their_expression_completes() {
    for (body, mismatches) in [
        ("return if true { return 42; } else { return 7; };", 0),
        ("return if true { return 42; } else { 7 };", 0),
        ("return if true { return 42; } else { false };", 1),
        ("return if true { return false; } else { return 7; };", 1),
        ("return;", 1),
    ] {
        let analysis = crate::analyze_source(
            &SourceFile::new(
                "return-completion.kgr",
                format!("fn main() -> i32 {{ {body} }}"),
            ),
            Default::default(),
        );
        assert_eq!(
            analysis.diagnostics().len(),
            mismatches,
            "{body}: {:?}",
            analysis.diagnostics()
        );
        assert!(analysis.diagnostics().iter().all(|d| matches!(
            d.kind,
            kagari_common::DiagnosticKind::ReturnTypeMismatch { .. }
        )));
        assert_eq!(analysis.into_codegen().is_ok(), mismatches == 0);
    }
}
