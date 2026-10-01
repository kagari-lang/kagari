//! Portable native arithmetic/parsing coverage, including actual width boundaries.
use kagari_bytecode::{
    artifact::KbcArtifact,
    instruction::{BytecodeInstruction, ConstantOperand},
};
use kagari_common::identity::DefinitionKind;
use kagari_embed::{
    context::ExecutionContext,
    engine::{EngineConfig, KagariEngine},
    program::PreparedProgram,
};
use kagari_runtime::{
    gc::GcHeap,
    native::packages::standard_library,
    value::{EnumTag, Value},
};

const ARTIFACT: &[u8] = include_bytes!("fixtures/native_numeric.kbc");
const METHODS: [&str; 14] = [
    "wrapping_add",
    "wrapping_sub",
    "wrapping_mul",
    "checked_add",
    "checked_sub",
    "checked_mul",
    "checked_div",
    "checked_rem",
    "overflowing_add",
    "overflowing_sub",
    "overflowing_mul",
    "saturating_add",
    "saturating_sub",
    "saturating_mul",
];

fn engine() -> KagariEngine {
    let mut config = EngineConfig::default();
    config.default_runtime.gc.collection_threshold = Some(1);
    // Install the same ordinary packages explicitly, without default selection.
    KagariEngine::builder()
        .config(config)
        .install_standard_library(false)
        .install(Ok(standard_library()))
        .build()
        .unwrap()
}

fn prepared(patches: &[(&str, Vec<ConstantOperand>)]) -> PreparedProgram {
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
    let artifact = KbcArtifact::from_program(program, Default::default()).unwrap();
    PreparedProgram::from_artifact(
        KbcArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap(),
        &Default::default(),
        &Default::default(),
    )
    .unwrap()
}

fn checked(heap: &GcHeap, value: Value) -> Option<Value> {
    let Value::Enum(id) = value else {
        panic!("expected Option")
    };
    let snapshot = heap.enum_snapshot(id).unwrap();
    match (snapshot.tag, snapshot.fields.as_slice()) {
        (EnumTag::OptionNone, []) => None,
        (EnumTag::OptionSome, [value]) => Some(value.clone()),
        other => panic!("invalid Option: {other:?}"),
    }
}

fn parsed(heap: &GcHeap, value: Value) -> Result<Value, u8> {
    let Value::Enum(id) = value else {
        panic!("expected Result")
    };
    let snapshot = heap.enum_snapshot(id).unwrap();
    match (snapshot.tag, snapshot.fields.as_slice()) {
        (EnumTag::ResultOk, [value]) => Ok(value.clone()),
        (EnumTag::ResultErr, [Value::Enum(error)]) => {
            let error = heap.enum_snapshot(*error).unwrap();
            assert!(error.fields.is_empty());
            let EnumTag::ParseError(index) = error.tag else {
                panic!("expected ParseError")
            };
            Err(index)
        }
        other => panic!("invalid Result: {other:?}"),
    }
}

macro_rules! check_width {
    ($engine:expr, $script:literal, $rust:ty, $wire:ident, $cases:expr) => {{
        let context = ExecutionContext::default();
        for (left, right) in $cases {
            let operands = vec![
                ConstantOperand::$wire(left.into()),
                ConstantOperand::$wire(right.into()),
            ];
            let names: Vec<_> = METHODS
                .iter()
                .map(|method| format!("{}_{}", $script, method))
                .collect();
            let patches: Vec<_> = names
                .iter()
                .map(|name| (name.as_str(), operands.clone()))
                .collect();
            let mut runtime = $engine.runtime(context.clone());
            let loaded = runtime
                .load_program(&prepared(&patches), Default::default())
                .unwrap();
            let scalars = [
                left.wrapping_add(right),
                left.wrapping_sub(right),
                left.wrapping_mul(right),
            ];
            let optional = [
                left.checked_add(right),
                left.checked_sub(right),
                left.checked_mul(right),
                left.checked_div(right),
                left.checked_rem(right),
            ];
            let overflowing = [
                left.overflowing_add(right),
                left.overflowing_sub(right),
                left.overflowing_mul(right),
            ];
            let saturated = [
                left.saturating_add(right),
                left.saturating_sub(right),
                left.saturating_mul(right),
            ];
            for (index, entry) in names.iter().enumerate() {
                let value = runtime
                    .execute(&loaded, entry, &[], &context)
                    .unwrap()
                    .return_value;
                match index {
                    0..=2 => assert_eq!(
                        value,
                        Value::$wire(scalars[index].into()),
                        "{entry}: {left}, {right}"
                    ),
                    3..=7 => assert_eq!(
                        checked(runtime.runtime().gc(), value),
                        optional[index - 3].map(|value| Value::$wire(value.into())),
                        "{entry}: {left}, {right}"
                    ),
                    8..=10 => {
                        let (expected, overflow) = overflowing[index - 8];
                        assert_eq!(
                            value,
                            Value::Tuple(vec![
                                Value::$wire(expected.into()),
                                Value::Bool(overflow)
                            ]),
                            "{entry}: {left}, {right}"
                        );
                    }
                    _ => assert_eq!(
                        value,
                        Value::$wire(saturated[index - 11].into()),
                        "{entry}: {left}, {right}"
                    ),
                }
                assert_eq!(runtime.runtime().gc().active_roots(), 0);
                assert_eq!(
                    runtime.runtime().resources().counters().current_call_depth,
                    0
                );
                runtime.runtime().collect_garbage().unwrap();
                assert_eq!(runtime.runtime().gc().allocated_objects(), 0);
            }
        }
        for left in [<$rust>::MIN, <$rust>::MAX, 1 as $rust] {
            for count in [0, 1, <$rust>::BITS, <$rust>::BITS + 1, u32::MAX] {
                let names = [
                    format!("{}_rotate_left", $script),
                    format!("{}_rotate_right", $script),
                ];
                let operands = vec![
                    ConstantOperand::$wire(left.into()),
                    ConstantOperand::I64(count.into()),
                ];
                let patches = [
                    (names[0].as_str(), operands.clone()),
                    (names[1].as_str(), operands),
                ];
                let mut runtime = $engine.runtime(context.clone());
                let loaded = runtime
                    .load_program(&prepared(&patches), Default::default())
                    .unwrap();
                for (entry, expected) in names
                    .iter()
                    .zip([left.rotate_left(count), left.rotate_right(count)])
                {
                    assert_eq!(
                        runtime
                            .execute(&loaded, entry, &[], &context)
                            .unwrap()
                            .return_value,
                        Value::$wire(expected.into()),
                        "{entry}: {left}, {count}"
                    );
                    assert_eq!(runtime.runtime().gc().active_roots(), 0);
                }
            }
        }
    }};
}

#[test]
fn all_integer_widths_preserve_wrapping_checked_carry_saturation_and_rotations() {
    let engine = engine();
    macro_rules! signed {
        ($script:literal, $rust:ty, $wire:ident) => {
            check_width!(
                &engine,
                $script,
                $rust,
                $wire,
                [
                    (<$rust>::MIN, -1 as $rust),
                    (<$rust>::MAX, 1 as $rust),
                    (0 as $rust, 0 as $rust),
                    (0 as $rust, 1 as $rust),
                    (1 as $rust, 0 as $rust),
                    (3 as $rust, 2 as $rust),
                    (<$rust>::MIN, <$rust>::MAX),
                    (<$rust>::MAX, <$rust>::MIN),
                    (5 as $rust, -3 as $rust),
                ]
            );
        };
    }
    macro_rules! unsigned {
        ($script:literal, $rust:ty, $wire:ident) => {
            check_width!(
                &engine,
                $script,
                $rust,
                $wire,
                [
                    (<$rust>::MAX, 1 as $rust),
                    (0 as $rust, 0 as $rust),
                    (0 as $rust, 1 as $rust),
                    (1 as $rust, 0 as $rust),
                    (3 as $rust, 2 as $rust),
                    (<$rust>::MAX, <$rust>::MAX),
                ]
            );
        };
    }
    signed!("i8", i8, I32);
    signed!("i16", i16, I32);
    signed!("i32", i32, I32);
    signed!("i64", i64, I64);
    signed!("isize", i64, I64);
    unsigned!("u8", u8, I64);
    unsigned!("u16", u16, I64);
    unsigned!("u32", u32, I64);
    unsigned!("u64", u64, U64);
    unsigned!("usize", u64, U64);
}

#[test]
fn unsigned_signed_offsets_wrap_at_the_actual_receiver_width() {
    let engine = engine();
    let context = ExecutionContext::default();
    for (entry, unsigned, signed, expected) in [
        (
            "u8",
            ConstantOperand::I64(0),
            ConstantOperand::I32(-1),
            Value::I64(255),
        ),
        (
            "u16",
            ConstantOperand::I64(65535),
            ConstantOperand::I32(1),
            Value::I64(0),
        ),
        (
            "u32",
            ConstantOperand::I64(0),
            ConstantOperand::I32(i32::MIN),
            Value::I64(1 << 31),
        ),
        (
            "u64",
            ConstantOperand::U64(0),
            ConstantOperand::I64(-1),
            Value::U64(u64::MAX),
        ),
        (
            "usize",
            ConstantOperand::U64(u64::MAX),
            ConstantOperand::I64(1),
            Value::U64(0),
        ),
    ] {
        let entry = format!("{entry}_wrapping_add_signed");
        let mut runtime = engine.runtime(context.clone());
        let loaded = runtime
            .load_program(
                &prepared(&[(&entry, vec![unsigned, signed])]),
                Default::default(),
            )
            .unwrap();
        assert_eq!(
            runtime
                .execute(&loaded, &entry, &[], &context)
                .unwrap()
                .return_value,
            expected
        );
        assert_eq!(runtime.runtime().gc().active_roots(), 0);
    }
}

#[test]
fn all_primitive_parsers_execute_offline_and_keep_business_errors_distinct() {
    let engine = engine();
    let context = ExecutionContext::default();
    for (width, minimum, maximum, high, low, expected_min, expected_max) in [
        (
            "i8",
            "-128",
            "127",
            "128",
            "-129",
            Value::I32(-128),
            Value::I32(127),
        ),
        (
            "i16",
            "-32768",
            "32767",
            "32768",
            "-32769",
            Value::I32(-32768),
            Value::I32(32767),
        ),
        (
            "i32",
            "-2147483648",
            "2147483647",
            "2147483648",
            "-2147483649",
            Value::I32(i32::MIN),
            Value::I32(i32::MAX),
        ),
        (
            "i64",
            "-9223372036854775808",
            "9223372036854775807",
            "9223372036854775808",
            "-9223372036854775809",
            Value::I64(i64::MIN),
            Value::I64(i64::MAX),
        ),
        (
            "isize",
            "-9223372036854775808",
            "9223372036854775807",
            "9223372036854775808",
            "-9223372036854775809",
            Value::I64(i64::MIN),
            Value::I64(i64::MAX),
        ),
        (
            "u8",
            "0",
            "255",
            "256",
            "-1",
            Value::I64(0),
            Value::I64(255),
        ),
        (
            "u16",
            "0",
            "65535",
            "65536",
            "-1",
            Value::I64(0),
            Value::I64(65535),
        ),
        (
            "u32",
            "0",
            "4294967295",
            "4294967296",
            "-1",
            Value::I64(0),
            Value::I64(4294967295),
        ),
        (
            "u64",
            "0",
            "18446744073709551615",
            "18446744073709551616",
            "-1",
            Value::U64(0),
            Value::U64(u64::MAX),
        ),
        (
            "usize",
            "0",
            "18446744073709551615",
            "18446744073709551616",
            "-1",
            Value::U64(0),
            Value::U64(u64::MAX),
        ),
    ] {
        let negative_error = if width.starts_with('u') { 1 } else { 2 };
        for (text, radix, expected) in [
            (minimum, 10, Ok(expected_min)),
            (maximum, 10, Ok(expected_max.clone())),
            (high, 10, Err(2)),
            (low, 10, Err(negative_error)),
            ("", 10, Err(0)),
            ("+", 10, Err(1)),
            ("--1", 10, Err(1)),
            (" 1", 10, Err(1)),
            ("1 ", 10, Err(1)),
            ("1_0", 10, Err(1)),
            ("0x1", 10, Err(1)),
            ("2", 2, Err(1)),
            ("42", 0, Err(3)),
            ("42", 1, Err(3)),
            ("42", 37, Err(3)),
            ("42", u32::MAX, Err(3)),
        ] {
            let entry = format!("{width}_radix");
            let mut runtime = engine.runtime(context.clone());
            let loaded = runtime
                .load_program(
                    &prepared(&[(
                        &entry,
                        vec![
                            ConstantOperand::Str(text.into()),
                            ConstantOperand::I64(radix.into()),
                        ],
                    )]),
                    Default::default(),
                )
                .unwrap();
            let report = runtime.execute(&loaded, &entry, &[], &context).unwrap();
            assert_eq!(
                parsed(runtime.runtime().gc(), report.return_value),
                expected,
                "{entry}: {text}, {radix}"
            );
            assert_eq!(report.failure.is_some(), expected.is_err());
            assert_eq!(runtime.runtime().gc().active_roots(), 0);
            runtime.runtime().collect_garbage().unwrap();
            assert_eq!(runtime.runtime().gc().allocated_objects(), 0);
        }
        let entry = format!("{width}_parse");
        let mut runtime = engine.runtime(context.clone());
        let loaded = runtime
            .load_program(
                &prepared(&[(&entry, vec![ConstantOperand::Str(maximum.into())])]),
                Default::default(),
            )
            .unwrap();
        let value = runtime
            .execute(&loaded, &entry, &[], &context)
            .unwrap()
            .return_value;
        assert_eq!(parsed(runtime.runtime().gc(), value), Ok(expected_max));
    }
    for (width, text, expected) in [
        ("bool", "true", Ok(Value::Bool(true))),
        ("bool", "false", Ok(Value::Bool(false))),
        ("bool", "True", Err(4)),
        ("bool", " true", Err(4)),
        ("bool", "", Err(0)),
        ("f32", "-2.5e1", Ok(Value::F32(-25.0))),
        ("f64", "+2.5e1", Ok(Value::F64(25.0))),
        ("f32", "1e999", Ok(Value::F32(f32::INFINITY))),
        ("f64", "-Infinity", Ok(Value::F64(f64::NEG_INFINITY))),
        ("f32", "1.0 ", Err(4)),
        ("f64", " 1.0", Err(4)),
        ("f64", "", Err(0)),
    ] {
        let entry = format!("{width}_parse");
        let mut runtime = engine.runtime(context.clone());
        let loaded = runtime
            .load_program(
                &prepared(&[(&entry, vec![ConstantOperand::Str(text.into())])]),
                Default::default(),
            )
            .unwrap();
        let report = runtime.execute(&loaded, &entry, &[], &context).unwrap();
        assert_eq!(
            parsed(runtime.runtime().gc(), report.return_value),
            expected,
            "{entry}: {text}"
        );
        assert_eq!(report.failure.is_some(), expected.is_err());
        assert_eq!(runtime.runtime().gc().active_roots(), 0);
    }
}

#[test]
fn rooted_queries_evaluate_fallbacks_once_and_keep_error_creation_trace() {
    let context = ExecutionContext::default();
    let mut runtime = engine().runtime(context.clone());
    let loaded = runtime
        .load_program(&prepared(&[]), Default::default())
        .unwrap();
    for entry in ["option_queries", "result_queries"] {
        assert_eq!(
            runtime
                .execute(&loaded, entry, &[], &context)
                .unwrap()
                .return_value,
            Value::Bool(true)
        );
        assert_eq!(runtime.runtime().gc().active_roots(), 0);
        runtime.runtime().collect_garbage().unwrap();
        assert_eq!(runtime.runtime().gc().allocated_objects(), 0);
    }
    for entry in ["option_alias", "result_alias"] {
        let report = runtime.execute(&loaded, entry, &[], &context).unwrap();
        let Value::Tuple(values) = report.return_value else {
            panic!("payload handles expected")
        };
        assert!(matches!(values[0], Value::Struct(_)));
        assert_eq!(
            values[0], values[1],
            "{entry} retains the original heap identity"
        );
        assert_eq!(runtime.runtime().gc().active_roots(), 0);
        runtime.runtime().collect_garbage().unwrap();
        assert_eq!(runtime.runtime().gc().allocated_objects(), 0);
    }
    let report = runtime
        .execute(&loaded, "error_queries", &[], &context)
        .unwrap();
    let trace = runtime
        .runtime()
        .gc()
        .result_error_trace(&report.return_value)
        .unwrap();
    assert_eq!(trace.frames[0].function_name, "error_queries");
    assert!(report.failure.is_some());
}

#[test]
fn parsing_charges_input_work_and_every_budget_cut_releases_roots_and_frames() {
    let context = ExecutionContext::default();
    let mut runtime = engine().runtime(context.clone());
    let mut costs = vec![];
    for text in ["42".into(), format!("{}42", "0".repeat(64))] {
        let loaded = runtime
            .load_program(
                &prepared(&[("i32_parse", vec![ConstantOperand::Str(text)])]),
                Default::default(),
            )
            .unwrap();
        let before = runtime.runtime().resources().counters().instruction_steps;
        runtime
            .execute(&loaded, "i32_parse", &[], &context)
            .unwrap();
        costs.push(runtime.runtime().resources().counters().instruction_steps - before);
    }
    assert_eq!(costs[1] - costs[0], 64);
    for text in [format!("{}42", "0".repeat(64)), "invalid".into()] {
        let loaded = runtime
            .load_program(
                &prepared(&[("i32_parse", vec![ConstantOperand::Str(text)])]),
                Default::default(),
            )
            .unwrap();
        runtime.runtime().collect_garbage().unwrap();
        let before = runtime.runtime().resources().counters();
        let expected = runtime
            .execute(&loaded, "i32_parse", &[], &context)
            .unwrap();
        let expected = parsed(runtime.runtime().gc(), expected.return_value);
        let after = runtime.runtime().resources().counters();
        for allocation in [false, true] {
            let cost = if allocation {
                (after.allocation_units - before.allocation_units) as u64
            } else {
                after.instruction_steps - before.instruction_steps
            };
            let mut succeeded = false;
            for limit in 0..=cost {
                let mut limited = context.clone();
                if allocation {
                    limited.resources.max_allocation_units = Some(limit as usize);
                } else {
                    limited.resources.max_instruction_steps = Some(limit);
                }
                match runtime.execute(&loaded, "i32_parse", &[], &limited) {
                    Ok(report) => {
                        assert_eq!(
                            parsed(runtime.runtime().gc(), report.return_value),
                            expected
                        );
                        assert_eq!(limit, cost);
                        succeeded = true;
                    }
                    Err(error) => assert_eq!(error.code(), "KG_RUNTIME_RESOURCE_LIMIT_EXCEEDED"),
                }
                assert_eq!(runtime.runtime().gc().active_roots(), 0);
                assert_eq!(
                    runtime.runtime().resources().counters().current_call_depth,
                    0
                );
                assert!(!runtime.runtime().is_quarantined());
                runtime.runtime().collect_garbage().unwrap();
                assert_eq!(runtime.runtime().gc().allocated_objects(), 0);
            }
            assert!(succeeded);
        }
        let cancelled = ExecutionContext {
            cancellation: Default::default(),
            ..context.clone()
        };
        cancelled.cancellation.cancel();
        assert!(
            runtime
                .execute(&loaded, "i32_parse", &[], &cancelled)
                .is_err()
        );
        assert_eq!(runtime.runtime().gc().active_roots(), 0);
        let report = runtime
            .execute(&loaded, "i32_parse", &[], &context)
            .unwrap();
        assert_eq!(
            parsed(runtime.runtime().gc(), report.return_value),
            expected
        );
    }
}

#[test]
fn radix_digits_plus_sign_and_special_float_spellings_follow_complete_input_rules() {
    let context = ExecutionContext::default();
    let mut runtime = engine().runtime(context.clone());
    for (width, wire) in [
        ("i8", Value::I32(35)),
        ("u8", Value::I64(35)),
        ("i64", Value::I64(35)),
        ("usize", Value::U64(35)),
    ] {
        let entry = format!("{width}_radix");
        for text in ["z", "Z", "+z"] {
            let loaded = runtime
                .load_program(
                    &prepared(&[(
                        &entry,
                        vec![ConstantOperand::Str(text.into()), ConstantOperand::I64(36)],
                    )]),
                    Default::default(),
                )
                .unwrap();
            let result = runtime.execute(&loaded, &entry, &[], &context).unwrap();
            assert_eq!(
                parsed(runtime.runtime().gc(), result.return_value),
                Ok(wire.clone())
            );
        }
    }
    for width in ["f32", "f64"] {
        for text in ["NaN", "-nan", "+NAN", "inf", "+INFINITY", "-Inf", "-0"] {
            let entry = format!("{width}_parse");
            let loaded = runtime
                .load_program(
                    &prepared(&[(&entry, vec![ConstantOperand::Str(text.into())])]),
                    Default::default(),
                )
                .unwrap();
            let result = runtime.execute(&loaded, &entry, &[], &context).unwrap();
            let value = parsed(runtime.runtime().gc(), result.return_value).unwrap();
            match value {
                Value::F32(value) => {
                    let expected: f32 = text.parse().unwrap();
                    assert!(
                        expected.is_nan() && value.is_nan()
                            || expected.to_bits() == value.to_bits()
                    );
                }
                Value::F64(value) => {
                    let expected: f64 = text.parse().unwrap();
                    assert!(
                        expected.is_nan() && value.is_nan()
                            || expected.to_bits() == value.to_bits()
                    );
                }
                _ => panic!("floating result expected"),
            }
            assert_eq!(runtime.runtime().gc().active_roots(), 0);
        }
    }
}

#[test]
fn registered_numeric_inventory_and_generated_sources_have_actual_signature_owners() {
    let api = standard_library();
    let numeric = api
        .modules()
        .iter()
        .find(|module| module.identity.path == ["numeric"])
        .unwrap();
    assert_eq!(numeric.native_declarations().len(), 175);
    let string = api
        .modules()
        .iter()
        .find(|module| module.identity.path == ["string"])
        .unwrap();
    assert_eq!(string.implementations.len(), 14);
    let from_str = string.definition(DefinitionKind::Trait, "FromStr");
    assert_eq!(
        string
            .implementations
            .iter()
            .filter(|implementation| {
                implementation
                    .trait_type
                    .as_ref()
                    .is_some_and(|ty| ty.declaration == from_str)
            })
            .count(),
        13
    );
    let inherent: Vec<_> = string
        .implementations
        .iter()
        .filter(|implementation| implementation.trait_type.is_none())
        .collect();
    assert_eq!(inherent.len(), 1);
    assert_eq!(inherent[0].methods.len(), 27);
    assert_eq!(string.traits[0].name, "FromStr");
    assert_eq!(string.traits[0].methods[0].params.len(), 1);
    for source in api.declaration_sources() {
        let text = match source.uri.as_str() {
            "kagari://native/kagari-std/numeric.kgr" => include_str!("../../../stdlib/numeric.kgr"),
            "kagari://native/kagari-std/string.kgr" => include_str!("../../../stdlib/string.kgr"),
            "kagari://native/kagari-std/option.kgr" => include_str!("../../../stdlib/option.kgr"),
            "kagari://native/kagari-std/result.kgr" => include_str!("../../../stdlib/result.kgr"),
            _ => continue,
        };
        assert_eq!(source.text, text);
    }
    let mut module = numeric.as_ref().clone();
    module.validate().unwrap();
    module.variant_exports.insert("missing".into());
    assert!(module.validate().is_err());
    let mut module = string.as_ref().clone();
    module.variant_exports.insert("String".into());
    assert!(module.validate().is_err());
    let mut module = string.as_ref().clone();
    module.variant_exports.insert("ParseError".into());
    module.validate().unwrap();
    module.types[1].variants[0].name = "String".into();
    assert!(module.validate().is_err());
}

#[cfg(feature = "source")]
#[test]
fn source_emission_matches_portable_numeric_fixture() {
    use kagari_common::source::SourceFile;
    let artifact = engine()
        .compile_to_artifact(
            SourceFile::new(
                "memory://native-numeric.kgr",
                include_str!("fixtures/native_numeric.kgr"),
            ),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    assert_eq!(artifact.to_bytes().unwrap(), ARTIFACT);
}

#[cfg(feature = "source")]
#[test]
fn numeric_static_checks_reject_wrong_widths_signed_offsets_and_parse_shapes() {
    use kagari_common::source::SourceFile;
    let engine = engine();
    for body in [
        "(1i8).wrapping_add(1i16)",
        "(1u8).wrapping_add_signed(1i64)",
        "i8::from_str_radix(42, 10u32)",
        "i8::from_str_radix(\"42\", 10usize)",
        "i32::from_str(true)",
    ] {
        assert!(
            engine
                .compile_to_artifact(
                    SourceFile::new(
                        "memory://invalid-numeric.kgr",
                        format!(
                            "use std::numeric; use std::string::FromStr; fn main() {{ {body}; }}"
                        )
                    ),
                    Default::default(),
                    Default::default(),
                )
                .is_err(),
            "{body}"
        );
    }
    let empty = KagariEngine::builder()
        .install_standard_library(false)
        .build()
        .unwrap();
    assert!(
        empty
            .compile_to_artifact(
                SourceFile::new(
                    "memory://missing-numeric.kgr",
                    "fn main() { (1i8).wrapping_add(1i8); }"
                ),
                Default::default(),
                Default::default(),
            )
            .is_err()
    );
}

#[cfg(feature = "source")]
#[test]
fn numeric_methods_parsing_and_exported_variants_navigate_to_registered_sources() {
    use kagari_common::source_database::SourceLayer;
    let engine = engine();
    let text = "use std::numeric; use std::string::{FromStr, ParseError}; use std::option::Some; use std::result::{Result, Err}; fn main() -> bool { val p = i32::from_str(\"1\"); val o = Some((1i8).wrapping_add(1i8)); val e: Result<i32, ParseError> = Err(ParseError::InvalidDigit); p.is_ok() && o.is_some() && e.is_err() }";
    let file = engine
        .set_source(
            "memory://numeric-tooling.kgr",
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
    assert!(
        snapshot
            .file(file)
            .unwrap()
            .result()
            .diagnostics()
            .is_empty(),
        "{:?}",
        snapshot.file(file).unwrap().result().diagnostics()
    );
    for (name, module) in [
        ("from_str", "string"),
        ("wrapping_add", "numeric"),
        ("Some", "option"),
        ("Err", "result"),
        ("InvalidDigit", "string"),
    ] {
        let offset = if name == "Err" {
            text.find("Err(ParseError").unwrap()
        } else {
            text.rfind(name).unwrap()
        };
        let definition = snapshot.definition_at(file, offset).unwrap();
        let source = snapshot.source(definition.location.file).unwrap();
        assert_eq!(
            source.name(),
            format!("kagari://native/kagari-std/{module}.kgr")
        );
        assert_eq!(
            &source.text()[definition.location.range.start..definition.location.range.end],
            name
        );
    }
}
