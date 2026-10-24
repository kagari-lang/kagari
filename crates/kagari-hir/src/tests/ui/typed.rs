use super::support::{Environment, Fixture};
use kagari_source::diagnostic::{DiagnosticKind, TypePosition};
use rstest::rstest;

#[rstest]
#[case::number(include_str!("../../../tests/fixtures/ui/bare/unknown_parameter_number.kgr"), "number")]
#[case::isize(include_str!("../../../tests/fixtures/ui/bare/unknown_parameter_isize.kgr"), "isize")]
#[case::usize(include_str!("../../../tests/fixtures/ui/bare/unknown_parameter_usize.kgr"), "usize")]
fn reports_unknown_parameter_type(#[case] source: &str, #[case] name: &str) {
    let fixture = Fixture::new(source, "test.kg", Environment::Bare);
    let diagnostics = fixture.result().diagnostics();
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(
        diagnostics[0].kind,
        DiagnosticKind::UnknownType {
            type_name: name.to_string(),
            function_name: "foo".to_string(),
            position: TypePosition::Parameter,
        }
    );
}

#[test]
fn reports_unknown_return_type() {
    let fixture = Fixture::new(
        include_str!("../../../tests/fixtures/ui/bare/reports_unknown_return_type.kgr"),
        "test.kg",
        Environment::Bare,
    );
    let diagnostics = fixture.result().diagnostics();
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(
        diagnostics[0].kind,
        DiagnosticKind::UnknownType {
            type_name: "number".to_string(),
            function_name: "foo".to_string(),
            position: TypePosition::Return,
        }
    );
}

#[test]
fn reports_invalid_const_initializer_expression() {
    let fixture = Fixture::new(
        include_str!(
            "../../../tests/fixtures/ui/bare/reports_invalid_const_initializer_expression.kgr"
        ),
        "test.kg",
        Environment::Bare,
    );
    let diagnostics = fixture.result().diagnostics();
    assert_eq!(
        diagnostics[0].kind,
        DiagnosticKind::InvalidConstInitializer {
            const_name: "VALUE".to_string(),
            reason: "unsupported const initializer expression".to_string(),
        }
    );
}

#[test]
fn reports_reflection_write_on_const_value() {
    let fixture = Fixture::new(
        include_str!("../../../tests/fixtures/ui/bare/reports_reflection_write_on_const_value.kgr"),
        "test.kg",
        Environment::Bare,
    );
    let diagnostics = fixture.result().diagnostics();
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.kind
            == DiagnosticKind::ConstWriteNotAllowed {
                const_name: "ROOT".to_string(),
            }
    }));
}

#[test]
fn reports_const_dependency_cycle() {
    let fixture = Fixture::new(
        include_str!("../../../tests/fixtures/ui/bare/reports_const_dependency_cycle.kgr"),
        "test.kg",
        Environment::Bare,
    );
    let diagnostics = fixture.result().diagnostics();
    assert!(diagnostics.iter().any(|diagnostic| diagnostic.kind
        == DiagnosticKind::ConstCycle {
            const_name: "A".to_string(),
        }));
}

#[test]
fn rejects_heap_backed_const_types() {
    let fixture = Fixture::new(
        include_str!("../../../tests/fixtures/ui/standard/rejects_heap_backed_const_types.kgr"),
        "test.kg",
        Environment::Standard,
    );
    let diagnostics = fixture.result().diagnostics();
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.kind
            == DiagnosticKind::InvalidConstInitializer {
                const_name: "PAIR".to_string(),
                reason: "const type `(i32, i32)` is heap-backed; const supports value types only"
                    .to_string(),
            }
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.kind
            == DiagnosticKind::InvalidConstInitializer {
                const_name: "VALUES".to_string(),
                reason: "const type `Vec<i32>` is heap-backed; const supports value types only"
                    .to_string(),
            }
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.kind
            == DiagnosticKind::InvalidConstInitializer {
                const_name: "POINT".to_string(),
                reason: "const type `Point` is heap-backed; const supports value types only"
                    .to_string(),
            }
    }));
}

#[test]
fn reports_function_call_argument_type_mismatch() {
    let fixture = Fixture::new(
        include_str!(
            "../../../tests/fixtures/ui/bare/reports_function_call_argument_type_mismatch.kgr"
        ),
        "test.kg",
        Environment::Bare,
    );
    let diagnostics = fixture.result().diagnostics();
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.kind
            == DiagnosticKind::ArgumentTypeMismatch {
                function_name: "add_one".to_string(),
                parameter_name: "value".to_string(),
                expected: "i32".to_string(),
                found: "bool".to_string(),
            }
    }));
}

#[test]
fn reports_function_call_arity_mismatch() {
    let fixture = Fixture::new(
        include_str!("../../../tests/fixtures/ui/bare/reports_function_call_arity_mismatch.kgr"),
        "test.kg",
        Environment::Bare,
    );
    let diagnostics = fixture.result().diagnostics();
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.kind
            == DiagnosticKind::CallArityMismatch {
                function_name: "answer".to_string(),
                expected: 0,
                found: 1,
            }
    }));
}

#[test]
fn reports_return_type_mismatch() {
    let fixture = Fixture::new(
        include_str!("../../../tests/fixtures/ui/bare/reports_return_type_mismatch.kgr"),
        "test.kg",
        Environment::Bare,
    );
    let diagnostics = fixture.result().diagnostics();
    assert_eq!(
        diagnostics[0].kind,
        DiagnosticKind::ReturnTypeMismatch {
            function_name: "foo".to_string(),
            expected: "i32".to_string(),
            found: "bool".to_string(),
        }
    );
}

#[test]
fn reports_break_and_continue_outside_loop() {
    let fixture = Fixture::new(
        include_str!("../../../tests/fixtures/ui/bare/reports_break_and_continue_outside_loop.kgr"),
        "test.kg",
        Environment::Bare,
    );
    let diagnostics = fixture.result().diagnostics();
    assert_eq!(diagnostics.len(), 2);
    assert_eq!(diagnostics[0].kind, DiagnosticKind::BreakOutsideLoop);
    assert_eq!(diagnostics[1].kind, DiagnosticKind::ContinueOutsideLoop);
}

#[test]
fn reports_invalid_assignment_target() {
    let fixture = Fixture::new(
        include_str!("../../../tests/fixtures/ui/bare/reports_invalid_assignment_target.kgr"),
        "test.kg",
        Environment::Bare,
    );
    let diagnostics = fixture.result().diagnostics();
    assert_eq!(
        diagnostics[0].kind,
        DiagnosticKind::InvalidAssignmentTarget {
            reason: "function item is not assignable".to_string(),
        }
    );
}

#[test]
fn reports_assignment_type_mismatch() {
    let fixture = Fixture::new(
        include_str!("../../../tests/fixtures/ui/bare/reports_assignment_type_mismatch.kgr"),
        "test.kg",
        Environment::Bare,
    );
    let diagnostics = fixture.result().diagnostics();
    assert_eq!(
        diagnostics[0].kind,
        DiagnosticKind::AssignmentTypeMismatch {
            expected: "i32".to_string(),
            found: "bool".to_string(),
        }
    );
}

#[test]
fn reports_condition_type_mismatch() {
    let fixture = Fixture::new(
        include_str!("../../../tests/fixtures/ui/bare/reports_condition_type_mismatch.kgr"),
        "test.kg",
        Environment::Bare,
    );
    let diagnostics = fixture.result().diagnostics();
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.kind
            == DiagnosticKind::ConditionTypeMismatch {
                context: "if",
                found: "i32".to_string(),
            }
    }));
}

#[test]
fn reports_binary_operand_type_mismatch() {
    let fixture = Fixture::new(
        include_str!("../../../tests/fixtures/ui/bare/reports_binary_operand_type_mismatch.kgr"),
        "test.kg",
        Environment::Bare,
    );
    let diagnostics = fixture.result().diagnostics();
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.kind
            == DiagnosticKind::BinaryOperandTypeMismatch {
                operator: "+",
                expected: "matching numeric".to_string(),
                lhs: "i32".to_string(),
                rhs: "bool".to_string(),
            }
    }));
}

#[test]
fn reports_array_element_type_mismatch() {
    let fixture = Fixture::new(
        include_str!("../../../tests/fixtures/ui/bare/reports_array_element_type_mismatch.kgr"),
        "test.kg",
        Environment::Bare,
    );
    let diagnostics = fixture.result().diagnostics();
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.kind
            == DiagnosticKind::ArrayElementTypeMismatch {
                expected: "i32".to_string(),
                found: "bool".to_string(),
            }
    }));
}

#[test]
fn reports_invalid_struct_initializers() {
    let fixture = Fixture::new(
        include_str!("../../../tests/fixtures/ui/bare/reports_invalid_struct_initializers.kgr"),
        "test.kg",
        Environment::Bare,
    );
    let diagnostics = fixture.result().diagnostics();
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.kind
            == DiagnosticKind::AssignmentTypeMismatch {
                expected: "i32".to_string(),
                found: "bool".to_string(),
            }
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.kind
            == DiagnosticKind::InvalidStructInitializer {
                struct_name: "Point".to_string(),
                reason: "unknown field `z`".to_string(),
            }
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.kind
            == DiagnosticKind::InvalidStructInitializer {
                struct_name: "Point".to_string(),
                reason: "duplicate field `x`".to_string(),
            }
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.kind
            == DiagnosticKind::InvalidStructInitializer {
                struct_name: "Point".to_string(),
                reason: "missing field `y`".to_string(),
            }
    }));
}

#[test]
fn val_local_rebinding() {
    let fixture = Fixture::new(
        include_str!("../../../tests/fixtures/ui/bare/val_local_rebinding.kgr"),
        "test.kg",
        Environment::Bare,
    );
    let diagnostics = fixture.result().diagnostics();
    assert_eq!(
        diagnostics[0].kind,
        DiagnosticKind::InvalidAssignmentTarget {
            reason: "`val` binding cannot be reassigned".to_string(),
        }
    );
}

#[test]
fn parameter_rebinding() {
    let fixture = Fixture::new(
        include_str!("../../../tests/fixtures/ui/bare/parameter_rebinding.kgr"),
        "test.kg",
        Environment::Bare,
    );
    let diagnostics = fixture.result().diagnostics();
    assert_eq!(
        diagnostics[0].kind,
        DiagnosticKind::InvalidAssignmentTarget {
            reason: "function parameters are `val` bindings and cannot be reassigned".to_string(),
        }
    );
}

#[test]
fn rejects_assignment_to_val_field() {
    let fixture = Fixture::new(
        include_str!("../../../tests/fixtures/ui/bare/rejects_assignment_to_val_field.kgr"),
        "test.kg",
        Environment::Bare,
    );
    let diagnostics = fixture.result().diagnostics();
    assert_eq!(
        diagnostics[0].kind,
        DiagnosticKind::InvalidAssignmentTarget {
            reason: "`val` field `x` cannot be assigned".to_string(),
        }
    );
}

#[test]
fn reports_if_branch_type_mismatch() {
    let fixture = Fixture::new(
        include_str!("../../../tests/fixtures/ui/bare/reports_if_branch_type_mismatch.kgr"),
        "test.kg",
        Environment::Bare,
    );
    let diagnostics = fixture.result().diagnostics();
    assert_eq!(
        diagnostics[0].kind,
        DiagnosticKind::IfBranchTypeMismatch {
            expected: "i32".to_string(),
            found: "bool".to_string(),
        }
    );
}

#[test]
fn reports_match_arm_type_mismatch() {
    let fixture = Fixture::new(
        include_str!("../../../tests/fixtures/ui/bare/reports_match_arm_type_mismatch.kgr"),
        "test.kg",
        Environment::Bare,
    );
    let diagnostics = fixture.result().diagnostics();
    assert_eq!(
        diagnostics[0].kind,
        DiagnosticKind::MatchArmTypeMismatch {
            expected: "i32".to_string(),
            found: "bool".to_string(),
        }
    );
}

#[test]
fn rejects_assignment_to_const() {
    let fixture = Fixture::new(
        include_str!("../../../tests/fixtures/ui/bare/rejects_assignment_to_const.kgr"),
        "test.kg",
        Environment::Bare,
    );
    let diagnostics = fixture.result().diagnostics();
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(
        diagnostics[0].kind,
        DiagnosticKind::InvalidAssignmentTarget {
            reason: "`const` item cannot be reassigned".to_string(),
        }
    );
}

#[test]
fn reports_duplicate_function_names() {
    let fixture = Fixture::new(
        include_str!("../../../tests/fixtures/ui/bare/duplicate_function_names.kgr"),
        "test.kg",
        Environment::Bare,
    );
    let diagnostics = fixture.result().diagnostics();
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(
        diagnostics[0].kind,
        DiagnosticKind::DuplicateDeclaration {
            name: "foo".to_string(),
        }
    );
}

#[test]
fn vec_push_arity() {
    let fixture = Fixture::new(
        include_str!("../../../tests/fixtures/ui/standard/vec_push_arity.kgr"),
        "test.kg",
        Environment::Standard,
    );
    let diagnostics = fixture.result().diagnostics();
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.kind
            == DiagnosticKind::CallArityMismatch {
                function_name: "push".to_owned(),
                expected: 2,
                found: 1,
            }
    }));
}

#[test]
fn map_contains_key_argument() {
    let fixture = Fixture::new(
        include_str!("../../../tests/fixtures/ui/standard/map_contains_key_argument.kgr"),
        "test.kg",
        Environment::Standard,
    );
    let diagnostics = fixture.result().diagnostics();
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.kind
            == DiagnosticKind::ArgumentTypeMismatch {
                function_name: "contains_key".to_owned(),
                parameter_name: "arg1".to_owned(),
                expected: "String".to_owned(),
                found: "i32".to_owned(),
            }
    }));
}

#[test]
fn reports_unknown_trait_bounds() {
    let fixture = Fixture::new(
        include_str!("../../../tests/fixtures/ui/bare/reports_unknown_trait_bounds.kgr"),
        "test.kg",
        Environment::Bare,
    );
    let diagnostics = fixture.result().diagnostics();
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.kind
            == DiagnosticKind::UnknownTrait {
                trait_name: "Missing".to_string(),
            }
    }));
}

#[test]
fn rejects_interface_method_with_another_self_parameter() {
    let fixture = Fixture::new(
        include_str!(
            "../../../tests/fixtures/ui/bare/rejects_interface_method_with_another_self_parameter.kgr"
        ),
        "test.kg",
        Environment::Bare,
    );
    let diagnostics = fixture.result().diagnostics();
    assert!(diagnostics.iter().any(|diagnostic| matches!(
        &diagnostic.kind,
        DiagnosticKind::InvalidInterfaceType { reason, .. }
            if reason == "method `same` is not interface-compatible"
    )));
}

#[test]
fn trait_impl_missing_method() {
    let fixture = Fixture::new(
        include_str!("../../../tests/fixtures/ui/standard/trait_impl_missing_method.kgr"),
        "test.kg",
        Environment::Standard,
    );
    let diagnostics = fixture.result().diagnostics();
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.kind
            == DiagnosticKind::TraitMethodMismatch {
                trait_name: "Display".to_string(),
                method_name: "show".to_string(),
                reason: "missing impl method".to_string(),
            }
    }));
}

#[test]
fn trait_impl_wrong_result() {
    let fixture = Fixture::new(
        include_str!("../../../tests/fixtures/ui/standard/trait_impl_wrong_result.kgr"),
        "test.kg",
        Environment::Standard,
    );
    let diagnostics = fixture.result().diagnostics();
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.kind
            == DiagnosticKind::TraitMethodMismatch {
                trait_name: "Display".to_string(),
                method_name: "show".to_string(),
                reason: "return type expected `String`, found `i32`".to_string(),
            }
    }));
}

#[rstest]
#[case::local_public_local(include_str!("../../../tests/fixtures/ui/local_functions/local_public_local.kgr"))]
#[case::local_constant_local(include_str!("../../../tests/fixtures/ui/local_functions/local_constant_local.kgr"))]
fn local_unsupported_syntax_retains_its_typed_kind(#[case] source: &str) {
    let fixture = Fixture::new(source, "locals.kgr", Environment::Standard);
    assert!(
        fixture
            .result()
            .diagnostics()
            .iter()
            .any(|diagnostic| matches!(diagnostic.kind, DiagnosticKind::UnsupportedSyntax { .. }))
    );
}
