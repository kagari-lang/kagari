use super::{WIDTHS, compile, context, engine, execute, execute_checks};
use kagari_runtime::{module::entry::ProgramEntry, native::objects::Object, value::Value};
use kagari_source::source::SourceFile;
use rstest::rstest;

#[rstest]
#[case::i8("i8", "-128i8", "127i8", "-2i8", "-128i8")]
#[case::i64(
    "i64",
    "-9223372036854775808i64",
    "9223372036854775807i64",
    "-2i64",
    "-9223372036854775808i64"
)]
#[case::u8("u8", "0u8", "255u8", "254u8", "0u8")]
#[case::u64(
    "u64",
    "0u64",
    "18446744073709551615u64",
    "18446744073709551614u64",
    "0u64"
)]
fn explicit_policies_preserve_boundary_values_and_result_carriers(
    #[case] ty: &str,
    #[case] min: &str,
    #[case] max: &str,
    #[case] twice_max: &str,
    #[case] wrapped_add: &str,
) {
    let mut checks = Vec::new();
    for (operation, receiver, rhs, wrapped, saturated) in [
        ("add", max, format!("1{ty}"), wrapped_add, max),
        ("sub", min, format!("1{ty}"), max, min),
        ("mul", max, format!("2{ty}"), twice_max, max),
    ] {
        checks.push(format!(
            "({receiver}).wrapping_{operation}({rhs}) == {wrapped}"
        ));
        checks.push(format!(
            "({receiver}).saturating_{operation}({rhs}) == {saturated}"
        ));
        checks.push(format!(
            "match ({receiver}).checked_{operation}({rhs}) \
             {{ None => true, Some(_) => false }}"
        ));
        checks.push(format!(
            "{{ val result: ({ty}, bool) = ({receiver}).overflowing_{operation}({rhs}); \
             val (value, overflow) = result; value == {wrapped} && overflow }}"
        ));
    }
    for method in ["checked_div", "checked_rem"] {
        checks.push(format!(
            "match (1{ty}).{method}(0{ty}) {{ None => true, Some(_) => false }}"
        ));
        if ty.starts_with('i') {
            checks.push(format!(
                "match ({min}).{method}(-1{ty}) {{ None => true, Some(_) => false }}"
            ));
        }
    }
    // Signed overflow is not a carry out of the sign bit.
    if ty == "i8" {
        checks.push(
            "{ val (value, overflow) = (-1i8).overflowing_add(1i8); value == 0i8 && !overflow }"
                .into(),
        );
    } else if ty == "u8" {
        checks.push(
            "{ val (value, overflow) = (127u8).overflowing_add(1u8); value == 128u8 && !overflow }"
                .into(),
        );
    }
    execute_checks(checks);
}

#[rstest]
#[case::u8("u8", "i8", "-128i8", "128u8")]
#[case::u16("u16", "i16", "-32768i16", "32768u16")]
#[case::u32("u32", "i32", "-2147483648i32", "2147483648u32")]
#[case::u64("u64", "i64", "-9223372036854775808i64", "9223372036854775808u64")]
fn unsigned_signed_offsets_keep_the_unsigned_receiver_and_matching_rhs_width(
    #[case] unsigned: &str,
    #[case] signed: &str,
    #[case] min: &str,
    #[case] high_bit: &str,
) {
    let mut checks = Vec::new();
    checks.push(format!(
        "(1{unsigned}).wrapping_add_signed(-1{signed}) == 0{unsigned}"
    ));
    checks.push(format!(
        "(!0{unsigned}).wrapping_add_signed(1{signed}) == 0{unsigned}"
    ));
    checks.push(format!(
        "(0{unsigned}).wrapping_add_signed({min}) == {high_bit}"
    ));
    execute(&format!("fn main() -> bool {{ {} }}", checks.join(" && ")));
}

#[test]
fn receivers_and_arguments_are_evaluated_once_in_left_to_right_order() {
    execute(include_str!(
        "../../../../../tests/fixtures/language/numeric_methods/receiver_argument_order.kgr"
    ));
}

#[test]
fn method_contracts_reject_wrong_arity_operand_types_and_receivers() {
    let compiler = engine();
    let mut invalid = Vec::new();
    for method in [
        "wrapping_add",
        "checked_div",
        "overflowing_mul",
        "saturating_sub",
        "rotate_left",
    ] {
        invalid.push(format!("(1u8).{method}()"));
        let rhs = if method == "rotate_left" {
            "1u32"
        } else {
            "1u8"
        };
        invalid.push(format!("(1u8).{method}({rhs}, {rhs})"));
    }
    for (ty, _) in WIDTHS {
        let wrong_width = if ty.ends_with('8') { "16" } else { "8" };
        let sign = &ty[..1];
        invalid.push(format!("(1{ty}).wrapping_add(1{sign}{wrong_width})"));
        invalid.push(format!("(1{ty}).wrapping_add_signed(1{ty})"));
        if ty.starts_with('u') {
            invalid.push(format!("(1{ty}).wrapping_add_signed(1i{wrong_width})"));
        }
    }
    invalid.extend(
        [
            "(1u8).wrapping_add_signed()",
            "(1u8).wrapping_add_signed(1i8, 1i8)",
            "(1i8).checked_add(true)",
            "(1u64).overflowing_add(1.0f64)",
            "(1i16).saturating_add(1u16)",
            "(1u8).rotate_left(1i32)",
            "(1i64).rotate_right(1u64)",
            "(1u32).rotate_left(true)",
            "(1.0f32).wrapping_add(1.0f32)",
            "(1.0f64).checked_add(1.0f64)",
            "(1.0f32).overflowing_mul(1.0f32)",
            "(1.0f64).rotate_right(1u32)",
        ]
        .map(str::to_owned),
    );
    for expression in invalid {
        let source = format!("fn main() {{ val result = {expression}; }}");
        assert!(
            compiler
                .compile_to_artifact(
                    SourceFile::new("invalid-numeric-method.kgr", source),
                    Default::default(),
                )
                .is_err(),
            "unexpectedly accepted {expression}"
        );
    }
    for (ty, expression) in [
        ("Option<u64>", "(1i64).checked_add(1i64)"),
        ("(u8, bool)", "(127i8).overflowing_add(1i8)"),
        ("i8", "(0u8).wrapping_add_signed(-1i8)"),
    ] {
        assert!(
            compiler
                .compile_to_artifact(
                    SourceFile::new(
                        "invalid-numeric-result.kgr",
                        format!("fn main() {{ val result: {ty} = {expression}; }}"),
                    ),
                    Default::default(),
                )
                .is_err(),
            "unexpectedly accepted {expression} as {ty}"
        );
    }
}

#[test]
fn allocating_numeric_results_survive_collection_and_traps_release_execution_roots() {
    let program = compile(include_str!(
        "../../../../../tests/fixtures/language/numeric_methods/allocating_result_carriers.kgr"
    ));
    let consumer = engine();
    let context = context();
    let mut runtime = consumer.runtime(context.clone());
    let loaded = runtime.load_program(&program, Default::default()).unwrap();
    let retained: Object = runtime
        .execute_typed(&loaded, ProgramEntry::Main, (), &context)
        .unwrap();
    assert!(runtime.runtime().gc().stats().collections > 0);
    runtime.runtime().collect_garbage().unwrap();
    let inspected: bool = runtime
        .execute_typed(&loaded, "inspect", (retained.clone(),), &context)
        .unwrap();
    assert!(inspected);
    drop(retained);
    assert_eq!(runtime.runtime().gc().active_roots(), 0);

    let error = runtime.execute(&loaded, "fail", &[], &context).unwrap_err();
    assert_eq!(error.code(), "KG_RUNTIME_SCRIPT_TRAP");
    assert!(
        format!("{error:?}").contains("division by zero"),
        "{error:?}"
    );
    assert_eq!(runtime.runtime().gc().active_roots(), 0);
    assert_eq!(
        runtime.runtime().resources().counters().current_call_depth,
        0
    );
    runtime.runtime().collect_garbage().unwrap();
    let after = runtime.execute(&loaded, "after", &[], &context).unwrap();
    assert_eq!(
        after
            .return_value
            .inspect(runtime.runtime(), |view| view.as_bool().map(Value::Bool))
            .unwrap(),
        Value::Bool(true)
    );
    drop(after);
    assert_eq!(runtime.runtime().gc().active_roots(), 0);
    assert!(!runtime.runtime().is_quarantined());
    runtime
        .reload_program(&loaded, &program, Default::default())
        .unwrap();
    assert_eq!(runtime.runtime().collect_garbage().unwrap().live_objects, 0);
}
