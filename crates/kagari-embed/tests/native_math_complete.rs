//! All numeric math contracts execute without source or default package selection.
// The generator shares the actual application registration.
#[path = "fixtures/native_math_api.rs"]
mod fixture_api;
use kagari_abi::{
    scalar::BuiltinType,
    standard::surface::StandardTypeConstraint,
    types::{AbiType, ConstraintAbi},
};
use kagari_bytecode::{
    artifact::KbcArtifact,
    instruction::{BytecodeInstruction, ConstantOperand},
    program::BytecodeProgram,
};
use kagari_embed::{
    context::ExecutionContext,
    engine::{EngineConfig, KagariEngine},
    error::{EmbeddingError, RuntimeFailureKind},
    program::PreparedProgram,
};
use kagari_native_macros::native_module;
use kagari_runtime::{native::packages::standard_library, value::Value};

#[native_module("game::invalid_number")]
mod invalid_number {
    use kagari_runtime::native_value::number::NativeNumber;
    #[native]
    pub fn wrong(value: NativeNumber<bool>) -> NativeNumber<bool> {
        value
    }
}

#[native_module("game::invalid_signed")]
mod invalid_signed {
    use kagari_runtime::native_value::number::NativeSignedNumber;
    #[native]
    pub fn wrong(value: NativeSignedNumber<u32>) -> NativeSignedNumber<u32> {
        value
    }
}

const ARTIFACT: &[u8] = include_bytes!("fixtures/native_math.kbc");

#[test]
fn incompatible_concrete_numeric_adapters_fail_during_registration() {
    for api in [invalid_number::native_api(), invalid_signed::native_api()] {
        assert!(
            api.unwrap_err()
                .message()
                .contains("compatible builtin number")
        );
    }
}

fn engine() -> KagariEngine {
    let mut config = EngineConfig::default();
    config.default_runtime.gc.collection_threshold = Some(1);
    KagariEngine::builder()
        .config(config)
        .install_standard_library(false)
        .install(Ok(standard_library()))
        .install(fixture_api::numbers::native_api())
        .build()
        .unwrap()
}

fn patched_program(patches: &[(&str, Vec<ConstantOperand>)]) -> BytecodeProgram {
    let mut program = KbcArtifact::from_bytes(ARTIFACT).unwrap().program;
    let root = &mut program.modules[program.root.index()];
    for (entry, operands) in patches {
        let function = root
            .functions
            .iter_mut()
            .find(|function| function.name == *entry)
            .unwrap();
        let mut replacements = operands.iter();
        let mut count = 0;
        for instruction in &mut function.instructions {
            if let BytecodeInstruction::LoadConst { constant, .. } = instruction {
                *constant = replacements.next().expect("enough inputs").clone();
                count += 1;
            }
        }
        assert_eq!(count, operands.len(), "{entry}");
        for operand in operands {
            if !root.constants.contains(operand) {
                root.constants.push(operand.clone());
            }
        }
    }
    program
}

fn prepared(patches: &[(&str, Vec<ConstantOperand>)]) -> PreparedProgram {
    let artifact = KbcArtifact::from_program(patched_program(patches), Default::default()).unwrap();
    PreparedProgram::from_artifact(
        KbcArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap(),
        &Default::default(),
        &Default::default(),
    )
    .unwrap()
}

fn assert_trap(error: EmbeddingError) {
    assert!(
        matches!(
            error,
            EmbeddingError::Runtime {
                kind: RuntimeFailureKind::ScriptTrap,
                ..
            }
        ),
        "{error:?}"
    );
}

macro_rules! integer_width {
    ($runtime:expr, $context:expr, $name:literal, $rust:ty, $wire:ident) => {{
        for (left, right) in [
            (<$rust>::MIN, <$rust>::MAX),
            (<$rust>::MAX, <$rust>::MIN),
            (0, 0),
            (1, 2),
        ] {
            for (method, expected) in [
                ("min", left.min(right)),
                ("max", left.max(right)),
                ("application", left.min(right)),
            ] {
                let entry = format!("{}_{}", $name, method);
                let loaded = $runtime
                    .load_program(
                        &prepared(&[(
                            &entry,
                            vec![
                                ConstantOperand::$wire(left.into()),
                                ConstantOperand::$wire(right.into()),
                            ],
                        )]),
                        Default::default(),
                    )
                    .unwrap();
                assert_eq!(
                    $runtime
                        .execute(&loaded, &entry, &[], &$context)
                        .unwrap()
                        .return_value,
                    Value::$wire(expected.into()),
                    "{entry}"
                );
            }
        }
        for (value, low, high) in [
            (<$rust>::MIN, 0, 1),
            (<$rust>::MAX, 0, 1),
            (1, 0, 2),
            (1, 1, 1),
        ] {
            let entry = concat!($name, "_clamp");
            let loaded = $runtime
                .load_program(
                    &prepared(&[(
                        entry,
                        vec![
                            ConstantOperand::$wire(value.into()),
                            ConstantOperand::$wire(low.into()),
                            ConstantOperand::$wire(high.into()),
                        ],
                    )]),
                    Default::default(),
                )
                .unwrap();
            assert_eq!(
                $runtime
                    .execute(&loaded, entry, &[], &$context)
                    .unwrap()
                    .return_value,
                Value::$wire(value.clamp(low, high).into())
            );
        }
        let entry = concat!($name, "_clamp");
        let loaded = $runtime
            .load_program(
                &prepared(&[(
                    entry,
                    vec![
                        ConstantOperand::$wire(0),
                        ConstantOperand::$wire(2),
                        ConstantOperand::$wire(1),
                    ],
                )]),
                Default::default(),
            )
            .unwrap();
        assert_trap(
            $runtime
                .execute(&loaded, entry, &[], &$context)
                .unwrap_err(),
        );
    }};
}

#[test]
fn every_integer_width_orders_clamps_and_preserves_extrema() {
    let context = ExecutionContext::default();
    let mut runtime = engine().runtime(context.clone());
    integer_width!(runtime, context, "i8", i8, I32);
    integer_width!(runtime, context, "i16", i16, I32);
    integer_width!(runtime, context, "i32", i32, I32);
    integer_width!(runtime, context, "i64", i64, I64);
    integer_width!(runtime, context, "isize", i64, I64);
    integer_width!(runtime, context, "u8", u8, I64);
    integer_width!(runtime, context, "u16", u16, I64);
    integer_width!(runtime, context, "u32", u32, I64);
    integer_width!(runtime, context, "u64", u64, U64);
    integer_width!(runtime, context, "usize", u64, U64);
    assert_eq!(runtime.runtime().gc().active_roots(), 0);
}

macro_rules! signed_width {
    ($runtime:expr, $context:expr, $name:literal, $rust:ty, $wire:ident) => {{
        for method in ["abs", "magnitude"] {
            let entry = format!("{}_{}", $name, method);
            for value in [-1, 0, 1, <$rust>::MAX, <$rust>::MIN + 1] {
                let loaded = $runtime
                    .load_program(
                        &prepared(&[(&entry, vec![ConstantOperand::$wire(value.into())])]),
                        Default::default(),
                    )
                    .unwrap();
                assert_eq!(
                    $runtime
                        .execute(&loaded, &entry, &[], &$context)
                        .unwrap()
                        .return_value,
                    Value::$wire(value.abs().into())
                );
            }
            let loaded = $runtime
                .load_program(
                    &prepared(&[(&entry, vec![ConstantOperand::$wire(<$rust>::MIN.into())])]),
                    Default::default(),
                )
                .unwrap();
            assert_trap(
                $runtime
                    .execute(&loaded, &entry, &[], &$context)
                    .unwrap_err(),
            );
            assert_eq!($runtime.runtime().gc().active_roots(), 0);
        }
    }};
}

#[test]
fn portable_numeric_applications_reject_out_of_range_shared_wire_representations() {
    for (entry, operands) in [
        (
            "i8_min",
            vec![ConstantOperand::I32(128), ConstantOperand::I32(0)],
        ),
        ("i16_abs", vec![ConstantOperand::I32(-32769)]),
        (
            "u8_max",
            vec![ConstantOperand::I64(-1), ConstantOperand::I64(0)],
        ),
        (
            "u16_clamp",
            vec![
                ConstantOperand::I64(65536),
                ConstantOperand::I64(0),
                ConstantOperand::I64(1),
            ],
        ),
        (
            "u32_application",
            vec![ConstantOperand::I64(4294967296), ConstantOperand::I64(0)],
        ),
    ] {
        assert!(
            KbcArtifact::from_program(patched_program(&[(entry, operands)]), Default::default())
                .is_err(),
            "{entry}"
        );
    }
}

#[test]
fn every_signed_width_checks_absolute_overflow_including_narrow_integers() {
    let context = ExecutionContext::default();
    let mut runtime = engine().runtime(context.clone());
    signed_width!(runtime, context, "i8", i8, I32);
    signed_width!(runtime, context, "i16", i16, I32);
    signed_width!(runtime, context, "i32", i32, I32);
    signed_width!(runtime, context, "i64", i64, I64);
    signed_width!(runtime, context, "isize", i64, I64);
}

macro_rules! floats {
    ($runtime:expr, $context:expr, $name:literal, $rust:ty, $wire:ident) => {{
        for method in ["min", "max", "application", "clamp", "abs", "magnitude"] {
            let entry = format!("{}_{}", $name, method);
            let count = match method {
                "clamp" => 3,
                "abs" | "magnitude" => 1,
                _ => 2,
            };
            let mut cases: Vec<Vec<$rust>> = match count {
                1 => vec![
                    vec![-0.0],
                    vec![<$rust>::MIN],
                    vec![<$rust>::MAX],
                    vec![-2.5],
                ],
                2 => vec![
                    vec![-0.0, 0.0],
                    vec![0.0, -0.0],
                    vec![<$rust>::MIN, <$rust>::MAX],
                    vec![2.5, -2.5],
                ],
                _ => vec![
                    vec![-0.0, -1.0, 1.0],
                    vec![2.5, -1.0, 1.0],
                    vec![-2.5, -1.0, 1.0],
                ],
            };
            for bad in [<$rust>::NAN, <$rust>::INFINITY, <$rust>::NEG_INFINITY] {
                for slot in 0..count {
                    let mut values = vec![0.0; count];
                    values[slot] = bad;
                    cases.push(values);
                }
            }
            for values in cases {
                let loaded = $runtime
                    .load_program(
                        &prepared(&[(
                            &entry,
                            values.iter().copied().map(ConstantOperand::$wire).collect(),
                        )]),
                        Default::default(),
                    )
                    .unwrap();
                let report = $runtime.execute(&loaded, &entry, &[], &$context);
                if values.iter().any(|value| !value.is_finite()) {
                    assert_trap(report.unwrap_err());
                } else {
                    let expected = match method {
                        "min" | "application" => {
                            if values[0] <= values[1] {
                                values[0]
                            } else {
                                values[1]
                            }
                        }
                        "max" => {
                            if values[0] >= values[1] {
                                values[0]
                            } else {
                                values[1]
                            }
                        }
                        "clamp" => values[0].clamp(values[1], values[2]),
                        _ => values[0].abs(),
                    };
                    let Value::$wire(actual) = report.unwrap().return_value else {
                        panic!("wrong width");
                    };
                    assert_eq!(actual.to_bits(), expected.to_bits(), "{entry}");
                }
                assert_eq!($runtime.runtime().gc().active_roots(), 0);
            }
        }
    }};
}

#[test]
fn floats_preserve_tie_bits_and_reject_nonfinite_values_in_every_operand() {
    let context = ExecutionContext::default();
    let mut runtime = engine().runtime(context.clone());
    floats!(runtime, context, "f32", f32, F32);
    floats!(runtime, context, "f64", f64, F64);
    // Bound validation precedes inspecting the value, preserving trap order.
    let loaded = runtime
        .load_program(
            &prepared(&[(
                "f64_clamp",
                vec![
                    ConstantOperand::F64(f64::NAN),
                    ConstantOperand::F64(2.0),
                    ConstantOperand::F64(1.0),
                ],
            )]),
            Default::default(),
        )
        .unwrap();
    let error = runtime
        .execute(&loaded, "f64_clamp", &[], &context)
        .unwrap_err();
    assert!(
        matches!(&error, EmbeddingError::Runtime { message, .. } if message.contains("bounds are reversed")),
        "{error:?}"
    );
}

#[test]
fn all_seven_f64_helpers_define_finite_rounding_and_domain_behavior() {
    let context = ExecutionContext::default();
    let mut runtime = engine().runtime(context.clone());
    for (name, operation) in [
        ("floor", f64::floor as fn(f64) -> f64),
        ("ceil", f64::ceil),
        ("round", f64::round),
        ("sqrt", f64::sqrt),
        ("sin", f64::sin),
        ("cos", f64::cos),
        ("tan", f64::tan),
    ] {
        let entry = format!("{name}_value");
        for value in [
            -0.0,
            0.0,
            -0.5,
            0.5,
            -1.5,
            1.5,
            9.0,
            f64::MAX,
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
        ] {
            let loaded = runtime
                .load_program(
                    &prepared(&[(&entry, vec![ConstantOperand::F64(value)])]),
                    Default::default(),
                )
                .unwrap();
            let report = runtime.execute(&loaded, &entry, &[], &context);
            if !value.is_finite() || name == "sqrt" && value < 0.0 {
                assert_trap(report.unwrap_err());
            } else {
                let Value::F64(actual) = report.unwrap().return_value else {
                    panic!("wrong type");
                };
                assert_eq!(
                    actual.to_bits(),
                    operation(value).to_bits(),
                    "{name}: {value}"
                );
            }
            assert_eq!(runtime.runtime().gc().active_roots(), 0);
            assert_eq!(
                runtime.runtime().resources().counters().current_call_depth,
                0
            );
        }
    }
}

#[test]
fn applied_closed_bounds_survive_encoding_and_cannot_be_dropped_or_forged() {
    for constraint in [
        StandardTypeConstraint::OrderedNumber,
        StandardTypeConstraint::SignedNumber,
    ] {
        for variant in 0..3 {
            let mut program = KbcArtifact::from_bytes(ARTIFACT).unwrap().program;
            let import = program
                .modules
                .iter_mut()
                .flat_map(|module| &mut module.native_imports)
                .find(|import| {
                    import.binding.module.package.0 == "game"
                        && import.requirements.iter().any(|bound| {
                            bound
                                .constraints
                                .contains(&ConstraintAbi::Standard(constraint))
                        })
                })
                .unwrap();
            assert_eq!(import.requirements.len(), 1);
            assert_eq!(import.requirements[0].ty, import.signature.params[0]);
            match variant {
                0 => import.requirements.clear(),
                1 => import.requirements[0].ty = AbiType::Builtin(BuiltinType::Bool),
                _ => {
                    import.requirements[0].constraints =
                        vec![ConstraintAbi::Standard(StandardTypeConstraint::Comparable)]
                }
            }
            assert!(KbcArtifact::from_program(program, Default::default()).is_err());
        }
    }
    let context = ExecutionContext::default();
    let mut runtime = engine().runtime(context.clone());
    let loaded = runtime
        .load_program(&prepared(&[]), Default::default())
        .unwrap();
    assert_eq!(
        runtime
            .execute(&loaded, "forwarded", &[], &context)
            .unwrap()
            .return_value,
        Value::Tuple(vec![Value::I32(3), Value::I32(4)])
    );
}

#[test]
fn eager_arguments_are_evaluated_once_in_order_and_survive_a_native_trap() {
    let context = ExecutionContext::default();
    let mut runtime = engine().runtime(context.clone());
    let loaded = runtime
        .load_program(&prepared(&[]), Default::default())
        .unwrap();
    for (entry, values) in [
        ("ordered_arguments", vec![4, 1, 3]),
        ("reversed_arguments", vec![1, 3, 2]),
    ] {
        assert!(fixture_api::numbers::take_events().is_empty());
        let report = runtime.execute(&loaded, entry, &[], &context);
        if entry == "ordered_arguments" {
            assert_eq!(report.unwrap().return_value, Value::I32(3));
        } else {
            assert_trap(report.unwrap_err());
        }
        assert_eq!(fixture_api::numbers::take_events(), values);
        assert_eq!(runtime.runtime().gc().active_roots(), 0);
        assert_eq!(
            runtime.runtime().resources().counters().current_call_depth,
            0
        );
    }
}

#[test]
fn every_logical_budget_cut_and_cancellation_releases_native_state() {
    let context = ExecutionContext::default();
    let mut runtime = engine().runtime(context.clone());
    let loaded = runtime
        .load_program(&prepared(&[]), Default::default())
        .unwrap();
    for entry in ["forwarded", "round_value", "i8_abs", "u64_clamp"] {
        let before = runtime.runtime().resources().counters().instruction_steps;
        let expected = runtime
            .execute(&loaded, entry, &[], &context)
            .unwrap()
            .return_value;
        let cost = runtime.runtime().resources().counters().instruction_steps - before;
        for limit in 0..=cost {
            let mut limited = context.clone();
            limited.resources.max_instruction_steps = Some(limit);
            match runtime.execute(&loaded, entry, &[], &limited) {
                Ok(report) => {
                    assert_eq!(limit, cost);
                    assert_eq!(report.return_value, expected);
                }
                Err(error) => assert_eq!(error.code(), "KG_RUNTIME_RESOURCE_LIMIT_EXCEEDED"),
            }
            assert_eq!(runtime.runtime().gc().active_roots(), 0);
            assert_eq!(
                runtime.runtime().resources().counters().current_call_depth,
                0
            );
            assert!(!runtime.runtime().is_quarantined());
        }
        let cancelled = ExecutionContext {
            cancellation: Default::default(),
            ..context.clone()
        };
        cancelled.cancellation.cancel();
        assert!(runtime.execute(&loaded, entry, &[], &cancelled).is_err());
        assert_eq!(runtime.runtime().gc().active_roots(), 0);
        assert_eq!(
            runtime
                .execute(&loaded, entry, &[], &context)
                .unwrap()
                .return_value,
            expected
        );
    }
}

#[cfg(feature = "source")]
mod source {
    use super::*;
    use kagari_common::{source::SourceFile, source_database::SourceLayer};
    use kagari_runtime::native::math_api::math;

    #[test]
    fn math_and_application_number_functions_install_independently_without_defaults() {
        for (api, text) in [
            (
                math::native_api(),
                "use std::math; fn main() -> i32 { math::min(7, 3) }",
            ),
            (
                fixture_api::numbers::native_api(),
                "use game::numbers; fn main() -> i32 { numbers::smaller(7, 3) }",
            ),
        ] {
            let engine = KagariEngine::builder()
                .install_standard_library(false)
                .install(api)
                .build()
                .unwrap();
            assert_eq!(engine.native_declaration_sources().len(), 1);
            let artifact = engine
                .compile_to_artifact(
                    SourceFile::new("memory://one-number-package.kgr", text),
                    Default::default(),
                    Default::default(),
                )
                .unwrap();
            let product = PreparedProgram::from_artifact(
                KbcArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap(),
                &Default::default(),
                &Default::default(),
            )
            .unwrap();
            let context = ExecutionContext::default();
            let mut runtime = engine.runtime(context.clone());
            let loaded = runtime.load_program(&product, Default::default()).unwrap();
            assert_eq!(
                runtime
                    .execute(&loaded, "main", &[], &context)
                    .unwrap()
                    .return_value,
                Value::I32(3)
            );
            assert_eq!(runtime.runtime().gc().active_roots(), 0);
        }
    }

    #[test]
    fn regenerated_source_and_artifact_are_exact_and_navigation_has_closed_bounds() {
        let engine = engine();
        let artifact = engine
            .compile_to_artifact(
                SourceFile::new(
                    "memory://native-math-complete.kgr",
                    include_str!("fixtures/native_math.kgr"),
                ),
                Default::default(),
                Default::default(),
            )
            .unwrap();
        assert_eq!(artifact.to_bytes().unwrap(), ARTIFACT);
        let text = "use game::numbers; fn main() -> i32 { numbers::smaller(7, 3) }";
        let file = engine
            .set_source(
                "memory://math-navigation.kgr",
                text.into(),
                SourceLayer::Base,
            )
            .unwrap();
        let snapshot = engine
            .analyze(
                engine.source_snapshot(),
                Default::default(),
                &Default::default(),
            )
            .unwrap();
        let offset = text.rfind("smaller").unwrap();
        let definition = snapshot.definition_at(file, offset).unwrap();
        let source = snapshot.source(definition.location.file).unwrap();
        assert_eq!(source.name(), "kagari://native/game/numbers.kgr");
        assert_eq!(
            &source.text()[definition.location.range.start..definition.location.range.end],
            "smaller"
        );
        let docs = snapshot.documentation_at(file, offset).unwrap();
        assert!(docs.written_signature.contains("OrderedNumber"));
    }

    #[test]
    fn nonnumeric_unsigned_abs_mixed_widths_and_unbounded_generic_calls_are_rejected() {
        let engine = engine();
        for text in [
            "fn main() { std::math::min(true, false); }",
            "fn main() { game::numbers::smaller(\"a\", \"b\"); }",
            "struct User {} fn main() { std::math::max(User {}, User {}); }",
            "fn main() { std::math::abs(1u32); }",
            "fn main() { game::numbers::magnitude(1usize); }",
            "fn main() { std::math::min(1i8, 2i16); }",
            "fn main() { std::math::sin(1f32); }",
            "fn bad<T>(x: T) -> T { std::math::min(x, x) }",
            "fn bad<T: OrderedNumber>(x: T) -> T { game::numbers::magnitude(x) }",
        ] {
            let text = format!(
                "use std::math; use game::numbers; {}",
                text.replace("std::math::", "math::")
                    .replace("game::numbers::", "numbers::")
            );
            assert!(
                engine
                    .compile_source(
                        SourceFile::new("memory://invalid-closed-math.kgr", text.clone()),
                        Default::default()
                    )
                    .is_err(),
                "{text}"
            );
        }
        let ordered_user = r#"
            use game::numbers;
            use std::cmp::{PartialEq, Eq, PartialOrd, Ord, Ordering};
            use std::option::Option;
            use std::option::Option::Some;
            struct User { val key: i32 }
            impl PartialEq for User { fn eq(self, other: Self) -> bool { self.key == other.key } }
            impl Eq for User {}
            impl PartialOrd for User { fn partial_cmp(self, other: Self) -> Option<Ordering> { Some(self.cmp(other)) } }
            impl Ord for User { fn cmp(self, other: Self) -> Ordering { Ordering::Equal } }
            fn ordinary<T: Ord>(x: T) -> T { x }
            fn main() -> User { ordinary(User { key: 1 }) }
        "#;
        engine
            .compile_source(
                SourceFile::new("memory://valid-user-order.kgr", ordered_user),
                Default::default(),
            )
            .unwrap();
        let rejected = format!(
            "{ordered_user}\nfn sealed() -> User {{ numbers::smaller(User {{ key: 1 }}, User {{ key: 2 }}) }}"
        );
        assert!(
            engine
                .compile_source(
                    SourceFile::new("memory://invalid-user-order.kgr", rejected),
                    Default::default()
                )
                .is_err()
        );
    }
}
