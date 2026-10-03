use super::contracts::analyze_contracts;
use super::*;
use crate::hir::expr::ExprKind;
use {crate::typeck::table::CallTarget, kagari_source::diagnostic::DiagnosticKind};

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
        let analysis = crate::analyze_source(&SourceFile::new(
            "array-completion.kgr",
            format!("fn main() -> i32 {{ {body} 0 }}"),
        ));
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
        let analysis = crate::analyze_source(&SourceFile::new(
            "condition-completion.kgr",
            format!("fn main() -> i32 {{ {body} 0 }}"),
        ));
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
        let analysis = crate::analyze_source(&SourceFile::new(
            "return-completion.kgr",
            format!("fn main() -> i32 {{ {body} }}"),
        ));
        assert_eq!(
            analysis.diagnostics().len(),
            mismatches,
            "{body}: {:?}",
            analysis.diagnostics()
        );
        assert!(
            analysis
                .diagnostics()
                .iter()
                .all(|d| matches!(d.kind, DiagnosticKind::ReturnTypeMismatch { .. }))
        );
        assert_eq!(analysis.into_codegen().is_ok(), mismatches == 0);
    }
}

#[test]
fn binding_and_assignment_values_require_types_only_when_they_complete() {
    for statement in [
        "val value: i32 = VALUE;",
        "var value = 1; value = VALUE;",
        "var value = 1; value += VALUE;",
    ] {
        for (value, valid) in [
            ("if true { return 42; } else { return 7; }", true),
            ("if true { return 42; } else { 7 }", true),
            ("if true { return 42; } else { false }", false),
            ("if true { return false; } else { return 7; }", false),
        ] {
            let body = statement.replace("VALUE", value);
            let analysis = crate::analyze_source(&SourceFile::new(
                "assignment-completion.kgr",
                format!("fn main() -> i32 {{ {body} 0 }}"),
            ));
            assert_eq!(
                analysis.diagnostics().is_empty(),
                valid,
                "{body}: {:?}",
                analysis.diagnostics()
            );
            assert_eq!(analysis.into_codegen().is_ok(), valid, "{body}");
        }
    }
    let analysis = crate::analyze_source(&SourceFile::new(
        "readonly-terminating-assignment.kgr",
        "fn main() -> i32 { val value = 1; value = if true { return 42; } else { return 7; }; 0 }",
    ));
    assert!(
        analysis
            .diagnostics()
            .iter()
            .any(|d| matches!(d.kind, DiagnosticKind::InvalidAssignmentTarget { .. }))
    );
    assert!(analysis.into_codegen().is_err());
}

#[test]
fn terminating_function_arguments_supply_no_value_or_generic_constraint() {
    for signature in [
        "fn take(value: i32) -> i32 { value }",
        "fn take<T: SignedNumber>(value: T) -> i32 { 0 }",
    ] {
        for (argument, valid) in [
            ("if true { return 42; } else { return 7; }", true),
            ("if true { return 42; } else { 7 }", true),
            ("if true { return 42; } else { false }", false),
            ("if true { return false; } else { return 7; }", false),
        ] {
            let source = format!("{signature} fn main() -> i32 {{ take({argument}) }}");
            let analysis =
                crate::analyze_source(&SourceFile::new("argument-completion.kgr", source.clone()));
            assert_eq!(
                analysis.diagnostics().is_empty(),
                valid,
                "{source}: {:?}",
                analysis.diagnostics()
            );
            assert_eq!(analysis.into_codegen().is_ok(), valid, "{source}");
        }
    }
    for arguments in [
        "if true { return 42; } else { return 7; }, false",
        "if true { return 42; } else { return 7; }, missing",
    ] {
        let analysis = crate::analyze_source(&SourceFile::new(
            "argument-completion-errors.kgr",
            format!(
                "fn take(value: i32) -> i32 {{ value }} fn main() -> i32 {{ take({arguments}) }}"
            ),
        ));
        assert!(
            analysis
                .diagnostics()
                .iter()
                .any(|d| matches!(d.kind, DiagnosticKind::CallArityMismatch { .. }))
        );
        assert!(analysis.into_codegen().is_err());
    }
}

#[test]
fn trait_and_declared_parameters_share_completion_aware_value_checks() {
    for call in ["value.take(ARG)", r#"take_bool(ARG, "unreachable"); 0"#] {
        for (argument, valid) in [
            ("if true { return 42; } else { return 7; }", true),
            ("if true { return 42; } else { true }", true),
            ("if true { return 42; } else { 7 }", false),
            ("if true { return false; } else { return 7; }", false),
        ] {
            let body = call.replace("ARG", argument);
            let source = format!(
                "trait Take {{ fn take(self, input: bool) -> i32; }} fn run<T: Take>(value: T) -> i32 {{ {body} }}"
            );
            let analysis = analyze_contracts(&SourceFile::new(
                "shared-parameter-completion.kgr",
                source.clone(),
            ));
            assert_eq!(
                analysis.diagnostics().is_empty(),
                valid,
                "{source}: {:?}",
                analysis.diagnostics()
            );
            assert_eq!(analysis.into_codegen().is_ok(), valid, "{source}");
        }
    }
}

#[test]
fn declared_operand_constraints_require_normally_produced_values() {
    for call in [
        "ordered_pair(ARG, 7)",
        "ordered_pair(ARG, ARG)",
        "ordered_pair(7, ARG)",
        "ordered_three(7, ARG, 9)",
        "signed_value(ARG)",
        r#"check_equal(ARG, 7, "test"); 0"#,
        r#"check_equal(7, ARG, "test"); 0"#,
    ] {
        for (argument, valid) in [
            ("if true { return 42; } else { return 7; }", true),
            ("if true { return 42; } else { 7 }", true),
            ("if true { return false; } else { return 7; }", false),
        ] {
            let body = call.replace("ARG", argument);
            let analysis = analyze_contracts(&SourceFile::new(
                "standard-completion.kgr",
                format!("fn main() -> i32 {{ {body} }}"),
            ));
            assert_eq!(
                analysis.diagnostics().is_empty(),
                valid,
                "{body}: {:?}",
                analysis.diagnostics()
            );
            assert_eq!(analysis.into_codegen().is_ok(), valid, "{body}");
        }
    }
    let analysis = analyze_contracts(&SourceFile::new(
        "standard-completion-invalid.kgr",
        "fn main() -> i32 { ordered_pair(if true { return 42; } else { return 7; }, false); 0 }",
    ));
    assert!(!analysis.diagnostics().is_empty());
    assert!(analysis.into_codegen().is_err());
}

#[test]
fn enum_payloads_follow_function_argument_completion_rules() {
    for constructor in ["Item::Value(ARG)", "Item<i32>::Value(ARG)"] {
        for (argument, valid) in [
            ("if true { return 42; } else { return 7; }", true),
            ("if true { return 42; } else { 7 }", true),
            ("if true { return false; } else { return 7; }", false),
            ("if true { return 42; } else { return 7; }, missing", false),
        ] {
            let expression = constructor.replace("ARG", argument);
            let analysis = crate::analyze_source(&SourceFile::new(
                "enum-completion.kgr",
                format!("enum Item<T> {{ Value(T) }} fn main() -> i32 {{ {expression}; 0 }}"),
            ));
            assert_eq!(
                analysis.diagnostics().is_empty(),
                valid,
                "{expression}: {:?}",
                analysis.diagnostics()
            );
            assert_eq!(analysis.into_codegen().is_ok(), valid, "{expression}");
        }
    }
    let analysis = crate::analyze_source(&SourceFile::new(
        "enum-completion-invalid.kgr",
        "enum Item<T> { Value(T, i32) } fn main() -> i32 { Item::Value(if true { return 42; } else { return 7; }, false); 0 }",
    ));
    assert!(
        analysis.diagnostics().iter().any(|diagnostic| matches!(
            diagnostic.kind,
            DiagnosticKind::ArgumentTypeMismatch { .. }
        ))
    );
    assert!(analysis.into_codegen().is_err());
}

#[test]
fn struct_fields_infer_and_check_only_normally_produced_values() {
    for constructor in ["Item", "Item<i32>"] {
        for (fields, valid) in [
            (
                "value: if true { return 42; } else { return 7; }, flag: true",
                true,
            ),
            ("value: if true { return 42; } else { 7 }, flag: true", true),
            (
                "value: if true { return false; } else { return 7; }, flag: true",
                false,
            ),
            (
                "value: if true { return 42; } else { return 7; }, flag: 7",
                false,
            ),
            ("value: if true { return 42; } else { return 7; }", false),
            (
                "value: if true { return 42; } else { return 7; }, flag: true, extra: 0",
                false,
            ),
            (
                "value: if true { return 42; } else { return 7; }, flag: true, flag: true",
                false,
            ),
        ] {
            let source = format!(
                "struct Item<T> {{ val value: T, val flag: bool }} fn main() -> i32 {{ {constructor} {{ {fields} }}; 0 }}"
            );
            let analysis =
                crate::analyze_source(&SourceFile::new("struct-completion.kgr", source.clone()));
            assert_eq!(
                analysis.diagnostics().is_empty(),
                valid,
                "{source}: {:?}",
                analysis.diagnostics()
            );
            assert_eq!(analysis.into_codegen().is_ok(), valid, "{source}");
        }
    }
}

#[test]
fn unary_constraints_apply_only_to_normally_produced_operands() {
    for (operator, good, bad) in [("-", "7", "false"), ("!", "true", "7.0")] {
        for (operand, valid) in [
            ("if true { return 42; } else { return 7; }".to_owned(), true),
            (format!("if true {{ return 42; }} else {{ {good} }}"), true),
            (format!("if true {{ return 42; }} else {{ {bad} }}"), false),
            (
                "if true { return false; } else { return 7; }".to_owned(),
                false,
            ),
        ] {
            let expression = format!("{operator}({operand})");
            let analysis = crate::analyze_source(&SourceFile::new(
                "unary-completion.kgr",
                format!("fn main() -> i32 {{ {expression}; 0 }}"),
            ));
            assert_eq!(
                analysis.diagnostics().is_empty(),
                valid,
                "{expression}: {:?}",
                analysis.diagnostics()
            );
            assert_eq!(analysis.into_codegen().is_ok(), valid, "{expression}");
        }
    }
}

#[test]
fn binary_constraints_ignore_absent_operands_but_check_known_counterparts() {
    let returning = "(if true { return 42; } else { return 7; })";
    for (expression, valid) in [
        (format!("{returning} + 1"), true),
        (format!("1 + {returning}"), true),
        (format!("{returning} < 1"), true),
        (format!("{returning} == false"), true),
        (format!("true && {returning}"), true),
        (format!("{returning} || false"), true),
        (format!("{returning} + false"), false),
        (format!("false + {returning}"), false),
        (format!("7 && {returning}"), false),
        (format!("{returning} || 7"), false),
        (format!("{returning} + missing"), false),
    ] {
        let analysis = crate::analyze_source(&SourceFile::new(
            "binary-completion.kgr",
            format!("fn main() -> i32 {{ {expression}; 0 }}"),
        ));
        assert_eq!(
            analysis.diagnostics().is_empty(),
            valid,
            "{expression}: {:?}",
            analysis.diagnostics()
        );
        assert_eq!(analysis.into_codegen().is_ok(), valid, "{expression}");
    }
}

#[test]
fn terminating_indexes_preserve_receiver_rules_without_requiring_an_index_value() {
    for statement in [
        "array[INDEX];",
        "tuple[INDEX];",
        "array[INDEX] = 7;",
        "array[INDEX] += 7;",
        "tuple[INDEX] = 7;",
        "set_index(array, INDEX, 7);",
    ] {
        for (index, valid) in [
            ("if true { return 42; } else { return 7; }", true),
            ("if true { return 42; } else { false }", false),
            ("if true { return false; } else { return 7; }", false),
        ] {
            let statement = statement.replace("INDEX", index);
            let analysis = crate::analyze_source(&SourceFile::new(
                "index-completion.kgr",
                format!(
                    "fn main() -> i32 {{ val array = [1]; var tuple = (1, true); {statement} 0 }}"
                ),
            ));
            assert_eq!(
                analysis.diagnostics().is_empty(),
                valid,
                "{statement}: {:?}",
                analysis.diagnostics()
            );
            assert_eq!(analysis.into_codegen().is_ok(), valid, "{statement}");
        }
    }
    for statement in ["true[INDEX];", "val tuple = (1, true); tuple[INDEX] = 7;"] {
        let statement = statement.replace("INDEX", "if true { return 42; } else { return 7; }");
        let analysis = crate::analyze_source(&SourceFile::new(
            "index-target-invalid.kgr",
            format!("fn main() -> i32 {{ {statement} 0 }}"),
        ));
        assert!(!analysis.diagnostics().is_empty(), "{statement}");
        assert!(analysis.into_codegen().is_err());
    }
}

#[test]
fn match_patterns_and_joins_require_a_normally_produced_scrutinee() {
    for (scrutinee, arms, valid) in [
        (
            "if true { return 42; } else { return 7; }",
            "1 => true, _ => 7",
            true,
        ),
        (
            "if true { return 42; } else { 7 }",
            "1 => true, _ => 7",
            false,
        ),
        (
            "if true { return 42; } else { false }",
            "1 => 7, _ => 7",
            false,
        ),
        (
            "if true { return false; } else { return 7; }",
            "1 => true, _ => 7",
            false,
        ),
        (
            "if true { return 42; } else { return 7; }",
            "1 => missing, _ => 7",
            false,
        ),
    ] {
        let source = format!("fn main() -> i32 {{ match ({scrutinee}) {{ {arms} }}; 0 }}");
        let analysis =
            crate::analyze_source(&SourceFile::new("match-completion.kgr", source.clone()));
        assert_eq!(
            analysis.diagnostics().is_empty(),
            valid,
            "{source}: {:?}",
            analysis.diagnostics()
        );
        assert_eq!(analysis.into_codegen().is_ok(), valid, "{source}");
    }
}

#[test]
fn if_result_joins_require_a_normally_produced_condition() {
    for (condition, branches, valid) in [
        (
            "if true { return 42; } else { return 7; }",
            "{ true } else { 7 }",
            true,
        ),
        (
            "if true { return 42; } else { true }",
            "{ true } else { 7 }",
            false,
        ),
        (
            "if true { return false; } else { return 7; }",
            "{ true } else { 7 }",
            false,
        ),
        (
            "if true { return 42; } else { return 7; }",
            "{ missing } else { 7 }",
            false,
        ),
        ("if true { return 42; } else { return 7; }", "{ 7 }", true),
    ] {
        let source = format!("fn main() -> i32 {{ if ({condition}) {branches}; 0 }}");
        let analysis =
            crate::analyze_source(&SourceFile::new("if-result-completion.kgr", source.clone()));
        assert_eq!(
            analysis.diagnostics().is_empty(),
            valid,
            "{source}: {:?}",
            analysis.diagnostics()
        );
        assert_eq!(analysis.into_codegen().is_ok(), valid, "{source}");
    }
}

#[test]
fn field_reads_require_member_targets_only_when_the_receiver_completes() {
    for (expression, valid) in [
        ("(if true { return 42; } else { return 7; }).value", true),
        (
            "(if true { return 42; } else { return 7; }).value.other",
            true,
        ),
        (
            "(if true { return 42; } else { Box { value: 7 } }).value",
            true,
        ),
        (
            "(if true { return 42; } else { Box { value: 7 } }).missing",
            false,
        ),
        (
            "(if true { return false; } else { return 7; }).value",
            false,
        ),
        ("(if true { return 42; } else { return 7; }).", false),
    ] {
        let source =
            format!("struct Box {{ val value: i32 }} fn main() -> i32 {{ {expression}; 0 }}");
        let analysis =
            crate::analyze_source(&SourceFile::new("field-completion.kgr", source.clone()));
        assert_eq!(
            analysis.diagnostics().is_empty(),
            valid,
            "{source}: {:?}",
            analysis.diagnostics()
        );
        assert_eq!(analysis.into_codegen().is_ok(), valid, "{source}");
    }
}

#[test]
fn index_reads_require_a_receiver_value_but_keep_inner_index_errors() {
    for (expression, valid) in [
        ("(if true { return 42; } else { return 7; })[0]", true),
        ("(if true { return 42; } else { return 7; })[0][1]", true),
        ("(if true { return 42; } else { [7] })[0]", true),
        ("(if true { return 42; } else { false })[0]", false),
        ("(if true { return 42; } else { [7] })[false]", false),
        ("(if true { return false; } else { return 7; })[0]", false),
        (
            "(if true { return 42; } else { return 7; })[missing]",
            false,
        ),
    ] {
        let source = format!("fn main() -> i32 {{ {expression}; 0 }}");
        let analysis = crate::analyze_source(&SourceFile::new(
            "index-receiver-completion.kgr",
            source.clone(),
        ));
        assert_eq!(
            analysis.diagnostics().is_empty(),
            valid,
            "{source}: {:?}",
            analysis.diagnostics()
        );
        assert_eq!(analysis.into_codegen().is_ok(), valid, "{source}");
    }
}

#[test]
fn declared_receiver_shapes_require_normally_produced_values() {
    for call in [
        "list_count(ARG)",
        "map_count(ARG)",
        "set_count(ARG)",
        "take_text(ARG)",
        "option_present(ARG)",
        "result_present(ARG)",
    ] {
        for (argument, valid) in [
            ("if true { return 42; } else { return 7; }", true),
            ("if true { return 42; } else { false }", false),
            ("if true { return false; } else { return 7; }", false),
        ] {
            let expression = call.replace("ARG", argument);
            let source = format!("fn main() -> i32 {{ {expression}; 0 }}");
            let analysis = analyze_contracts(&SourceFile::new(
                "standard-receiver-completion.kgr",
                source.clone(),
            ));
            assert_eq!(
                analysis.diagnostics().is_empty(),
                valid,
                "{source}: {:?}",
                analysis.diagnostics()
            );
            assert_eq!(analysis.into_codegen().is_ok(), valid, "{source}");
        }
    }
}

#[test]
fn terminating_callees_retain_explicit_call_facts_and_independent_errors() {
    for (callee, args, valid) in [
        ("(if true { return 42; } else { return 7; })", "1", true),
        (
            "(if true { return 42; } else { return 7; }).missing",
            "1",
            true,
        ),
        (
            "(if true { return 42; } else { return 7; })",
            "missing",
            false,
        ),
        ("(if true { return false; } else { return 7; })", "1", false),
        ("(if true { return 42; } else { 7 })", "1", false),
    ] {
        let source = format!("fn main() -> i32 {{ {callee}({args}); 0 }}");
        let analysis =
            crate::analyze_source(&SourceFile::new("callee-completion.kgr", source.clone()));
        assert_eq!(
            analysis.diagnostics().is_empty(),
            valid,
            "{source}: {:?}",
            analysis.diagnostics()
        );
        if valid {
            let facts = analysis.facts();
            assert!(facts.lowered.module.body.expressions().any(|(id, _)| {
                facts
                    .typed
                    .type_table
                    .call_resolution(id)
                    .is_some_and(|call| {
                        matches!(call.target, CallTarget::TerminatingCallee)
                            && call.receiver.is_some()
                    })
            }));
        }
        assert_eq!(analysis.into_codegen().is_ok(), valid, "{source}");
    }
}

#[test]
fn terminating_callee_facts_rebase_with_unchanged_body_reuse() {
    let text = "fn neighbor() -> i32 { 1 } fn main() -> i32 { (if true { return 42; } else { return 7; })(1); 0 }";
    let mut sources = SourceDatabase::default();
    let root = sources
        .set("callee-reuse.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut db = AnalysisDatabase::default();
    let old = db
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    old.check_program(root, &Default::default()).unwrap();
    sources
        .set(
            "callee-reuse.kgr",
            text.replace("{ 1 }", "{ 2 + 3 }"),
            SourceLayer::Base,
        )
        .unwrap();
    let new = db
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    new.check_program(root, &Default::default()).unwrap();
    assert_eq!(
        new.file(root).unwrap().result().facts().typed.reused_bodies,
        1
    );
    for snapshot in [&old, &new] {
        let facts = snapshot.file(root).unwrap().result().facts();
        let mut found = false;
        for (id, expr) in facts.lowered.module.body.expressions() {
            if let ExprKind::Call { callee, .. } = expr.kind {
                let call = facts.typed.type_table.call_resolution(id).unwrap();
                assert_eq!(call.target, CallTarget::TerminatingCallee);
                assert_eq!(call.receiver, Some(callee));
                found = true;
            }
        }
        assert!(found);
    }
}
