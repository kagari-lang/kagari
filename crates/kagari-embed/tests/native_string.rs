//! Direct String behavior, checked construction and application reuse without source.
// Share the actual registration with the source-only generator.
#[path = "fixtures/native_string_api.rs"]
mod fixture_api;
use fixture_api::text;
use kagari_bytecode::{
    artifact::KbcArtifact,
    instruction::{BytecodeInstruction, ConstantOperand},
};
use kagari_embed::{
    context::ExecutionContext,
    engine::{EngineConfig, KagariEngine},
    error::EmbeddingError,
    program::PreparedProgram,
};
use kagari_runtime::{
    Runtime,
    native::packages::standard_library,
    value::{EnumTag, Value},
};

const ARTIFACT: &[u8] = include_bytes!("fixtures/native_string.kbc");

fn engine() -> KagariEngine {
    let mut config = EngineConfig::default();
    config.default_runtime.gc.collection_threshold = Some(1);
    KagariEngine::builder()
        .config(config)
        .install_standard_library(false)
        .install(Ok(standard_library()))
        .install(text::native_api())
        .build()
        .unwrap()
}

fn prepared(entry: &str, operands: Vec<ConstantOperand>) -> PreparedProgram {
    let mut program = KbcArtifact::from_bytes(ARTIFACT).unwrap().program;
    if !entry.is_empty() {
        let root = &mut program.modules[program.root.index()];
        let function = root
            .functions
            .iter_mut()
            .find(|function| function.name == entry)
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
            if !root.constants.contains(&operand) {
                root.constants.push(operand);
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

fn string(text: &str) -> ConstantOperand {
    ConstantOperand::Str(text.into())
}

fn option(runtime: &Runtime, value: Value) -> Option<Value> {
    let Value::Enum(id) = value else {
        panic!("expected Option");
    };
    let snapshot = runtime.gc().enum_snapshot(id).unwrap();
    match (snapshot.tag, snapshot.fields.as_slice()) {
        (EnumTag::OptionNone, []) => None,
        (EnumTag::OptionSome, [value]) => Some(value.clone()),
        other => panic!("invalid Option: {other:?}"),
    }
}

#[test]
fn utf8_lengths_boundaries_and_slices_observe_bytes_and_unicode_scalars() {
    let context = ExecutionContext::default();
    let mut runtime = engine().runtime(context.clone());
    for text in ["", "ASCII", "é😀x", "e\u{301}", "\0\r\n"] {
        for (method, expected) in [
            ("len_bytes", Value::U64(text.len() as u64)),
            ("len_chars", Value::U64(text.chars().count() as u64)),
            ("is_empty", Value::Bool(text.is_empty())),
            ("is_ascii", Value::Bool(text.is_ascii())),
        ] {
            let entry = format!("{method}_value");
            let loaded = runtime
                .load_program(&prepared(&entry, vec![string(text)]), Default::default())
                .unwrap();
            assert_eq!(
                runtime
                    .execute(&loaded, &entry, &[], &context)
                    .unwrap()
                    .return_value,
                expected,
                "{method}: {text:?}"
            );
        }
        let indices: Vec<_> = (0..=text.len() + 1)
            .map(|index| index as u64)
            .chain([u64::MAX])
            .collect();
        for index in &indices {
            let loaded = runtime
                .load_program(
                    &prepared(
                        "is_char_boundary_value",
                        vec![string(text), ConstantOperand::U64(*index)],
                    ),
                    Default::default(),
                )
                .unwrap();
            assert_eq!(
                runtime
                    .execute(&loaded, "is_char_boundary_value", &[], &context)
                    .unwrap()
                    .return_value,
                Value::Bool(text.is_char_boundary(*index as usize))
            );
        }
        for start in &indices {
            for end in &indices {
                let loaded = runtime
                    .load_program(
                        &prepared(
                            "slice_value",
                            vec![
                                string(text),
                                ConstantOperand::U64(*start),
                                ConstantOperand::U64(*end),
                            ],
                        ),
                        Default::default(),
                    )
                    .unwrap();
                let value = runtime
                    .execute(&loaded, "slice_value", &[], &context)
                    .unwrap()
                    .return_value;
                assert_eq!(
                    option(runtime.runtime(), value),
                    text.get(*start as usize..*end as usize)
                        .map(|text| Value::Str(text.into())),
                    "{text:?}: {start}..{end}"
                );
            }
        }
    }
    assert_eq!(runtime.runtime().gc().active_roots(), 0);
}

#[test]
fn substring_queries_split_and_strip_preserve_offsets_empty_patterns_and_overlap() {
    let context = ExecutionContext::default();
    let mut runtime = engine().runtime(context.clone());
    for (text, needle) in [
        ("", ""),
        ("abc", ""),
        ("é😀é", "é"),
        ("é😀é", "😀"),
        ("é😀é", "absent"),
        ("aaaa", "aa"),
        ("a=b=c", "="),
        ("\0x\0", "\0"),
        ("ab", "abcdef"),
    ] {
        for (method, expected) in [
            ("contains", text.contains(needle)),
            ("starts_with", text.starts_with(needle)),
            ("ends_with", text.ends_with(needle)),
            ("eq_ignore_ascii_case", text.eq_ignore_ascii_case(needle)),
        ] {
            let entry = format!("{method}_value");
            let loaded = runtime
                .load_program(
                    &prepared(&entry, vec![string(text), string(needle)]),
                    Default::default(),
                )
                .unwrap();
            assert_eq!(
                runtime
                    .execute(&loaded, &entry, &[], &context)
                    .unwrap()
                    .return_value,
                Value::Bool(expected)
            );
        }
        for (method, expected) in [("find", text.find(needle)), ("rfind", text.rfind(needle))] {
            let entry = format!("{method}_value");
            let loaded = runtime
                .load_program(
                    &prepared(&entry, vec![string(text), string(needle)]),
                    Default::default(),
                )
                .unwrap();
            let value = runtime
                .execute(&loaded, &entry, &[], &context)
                .unwrap()
                .return_value;
            assert_eq!(
                option(runtime.runtime(), value),
                expected.map(|index| Value::U64(index as u64))
            );
        }
        for (method, expected) in [
            ("strip_prefix", text.strip_prefix(needle)),
            ("strip_suffix", text.strip_suffix(needle)),
        ] {
            let entry = format!("{method}_value");
            let loaded = runtime
                .load_program(
                    &prepared(&entry, vec![string(text), string(needle)]),
                    Default::default(),
                )
                .unwrap();
            let value = runtime
                .execute(&loaded, &entry, &[], &context)
                .unwrap()
                .return_value;
            assert_eq!(
                option(runtime.runtime(), value),
                expected.map(|text| Value::Str(text.into()))
            );
        }
        for (method, expected) in [
            ("split_once", text.split_once(needle)),
            ("rsplit_once", text.rsplit_once(needle)),
        ] {
            let entry = format!("{method}_value");
            let loaded = runtime
                .load_program(
                    &prepared(&entry, vec![string(text), string(needle)]),
                    Default::default(),
                )
                .unwrap();
            let value = runtime
                .execute(&loaded, &entry, &[], &context)
                .unwrap()
                .return_value;
            assert_eq!(
                option(runtime.runtime(), value),
                expected.map(|(left, right)| Value::Tuple(vec![
                    Value::Str(left.into()),
                    Value::Str(right.into())
                ]))
            );
        }
    }
}

#[test]
fn unicode_casing_trimming_and_ascii_comparison_preserve_context_and_input_contents() {
    let context = ExecutionContext::default();
    let mut runtime = engine().runtime(context.clone());
    for text in [
        "",
        "ASCIIé",
        "　 é😀 x  ",
        "　 \r\n\t",
        "straße İİ",
        "ΟΣ",
        "ΟΣΑ",
        "ΟΣ\u{301}",
        "ǅſKﬃ",
        "\0abc\0",
    ] {
        for (method, expected) in [
            ("trim", text.trim().to_owned()),
            ("trim_start", text.trim_start().to_owned()),
            ("trim_end", text.trim_end().to_owned()),
            ("to_ascii_lowercase", text.to_ascii_lowercase()),
            ("to_ascii_uppercase", text.to_ascii_uppercase()),
            ("to_lowercase", text.to_lowercase()),
            ("to_uppercase", text.to_uppercase()),
            ("reverse", text.chars().rev().collect()),
        ] {
            let entry = format!("{method}_value");
            let loaded = runtime
                .load_program(&prepared(&entry, vec![string(text)]), Default::default())
                .unwrap();
            assert_eq!(
                runtime
                    .execute(&loaded, &entry, &[], &context)
                    .unwrap()
                    .return_value,
                Value::Str(expected),
                "{method}: {text:?}"
            );
        }
    }
    for (method, input, expected) in [
        ("to_lowercase", "ΟΣ", "ος"),
        ("to_lowercase", "ΟΣΑ", "οσα"),
        ("to_lowercase", "ΟΣ\u{301}", "ος\u{301}"),
        ("to_lowercase", "İ", "i\u{307}"),
        ("to_uppercase", "straße", "STRASSE"),
        ("to_uppercase", "ﬃ", "FFI"),
        ("to_ascii_lowercase", "AbÉ", "abÉ"),
        ("to_ascii_uppercase", "Abé", "ABé"),
        ("trim", "　 é😀 x  ", "é😀 x"),
    ] {
        let entry = format!("{method}_value");
        let loaded = runtime
            .load_program(&prepared(&entry, vec![string(input)]), Default::default())
            .unwrap();
        assert_eq!(
            runtime
                .execute(&loaded, &entry, &[], &context)
                .unwrap()
                .return_value,
            Value::Str(expected.into()),
            "{method}: {input:?}"
        );
    }
    for (left, right, expected) in [
        ("AbC", "aBc", true),
        ("É", "é", false),
        ("İ", "i", false),
        ("éABC", "éabc", true),
    ] {
        let loaded = runtime
            .load_program(
                &prepared(
                    "eq_ignore_ascii_case_value",
                    vec![string(left), string(right)],
                ),
                Default::default(),
            )
            .unwrap();
        assert_eq!(
            runtime
                .execute(&loaded, "eq_ignore_ascii_case_value", &[], &context)
                .unwrap()
                .return_value,
            Value::Bool(expected)
        );
    }
    let loaded = runtime
        .load_program(&prepared("", vec![]), Default::default())
        .unwrap();
    assert_eq!(
        runtime
            .execute(&loaded, "immutable_value", &[], &context)
            .unwrap()
            .return_value,
        Value::Tuple(vec![Value::Str("É😀".into()), Value::Str("é😀".into())])
    );
    assert_eq!(
        runtime
            .execute(&loaded, "chain_value", &[], &context)
            .unwrap()
            .return_value,
        Value::U64(7)
    );
    assert_eq!(runtime.runtime().gc().active_roots(), 0);
}

#[test]
fn replacement_repetition_and_application_output_cover_empty_and_unicode_cases() {
    let context = ExecutionContext::default();
    let mut runtime = engine().runtime(context.clone());
    for (text, from, to) in [
        ("", "", "x"),
        ("é😀", "", "|"),
        ("aaa", "aa", "x"),
        ("aba", "a", "é"),
        ("é😀é", "é", ""),
        ("a", "absent", "x"),
        ("a\0b", "\0", "😀"),
    ] {
        for count in [0, 1, 2, 5, u64::MAX] {
            let loaded = runtime
                .load_program(
                    &prepared(
                        "replacen_value",
                        vec![
                            string(text),
                            string(from),
                            string(to),
                            ConstantOperand::U64(count),
                        ],
                    ),
                    Default::default(),
                )
                .unwrap();
            assert_eq!(
                runtime
                    .execute(&loaded, "replacen_value", &[], &context)
                    .unwrap()
                    .return_value,
                Value::Str(text.replacen(from, to, count as usize))
            );
        }
        let loaded = runtime
            .load_program(
                &prepared(
                    "replace_value",
                    vec![string(text), string(from), string(to)],
                ),
                Default::default(),
            )
            .unwrap();
        assert_eq!(
            runtime
                .execute(&loaded, "replace_value", &[], &context)
                .unwrap()
                .return_value,
            Value::Str(text.replace(from, to))
        );
    }
    for text in ["", "ab", "é😀", "\0"] {
        for count in [0, 1, 2, 7] {
            let loaded = runtime
                .load_program(
                    &prepared(
                        "repeat_value",
                        vec![string(text), ConstantOperand::U64(count)],
                    ),
                    Default::default(),
                )
                .unwrap();
            assert_eq!(
                runtime
                    .execute(&loaded, "repeat_value", &[], &context)
                    .unwrap()
                    .return_value,
                Value::Str(text.repeat(count as usize))
            );
        }
        for rhs in ["", "x", "😀"] {
            for (method, expected) in [
                ("concat", format!("{text}{rhs}")),
                ("prefix", format!("{rhs}{text}")),
            ] {
                let entry = format!("{method}_value");
                let loaded = runtime
                    .load_program(
                        &prepared(&entry, vec![string(text), string(rhs)]),
                        Default::default(),
                    )
                    .unwrap();
                assert_eq!(
                    runtime
                        .execute(&loaded, &entry, &[], &context)
                        .unwrap()
                        .return_value,
                    Value::Str(expected)
                );
            }
        }
    }
}

#[test]
fn oversized_output_and_bad_capacity_fail_without_leaking_frames_or_roots() {
    let context = ExecutionContext::default();
    let mut runtime = engine().runtime(context.clone());
    for text in ["x", "é"] {
        let loaded = runtime
            .load_program(
                &prepared(
                    "repeat_value",
                    vec![string(text), ConstantOperand::U64(u64::MAX)],
                ),
                Default::default(),
            )
            .unwrap();
        let error = runtime
            .execute(&loaded, "repeat_value", &[], &context)
            .unwrap_err();
        assert_eq!(error.code(), "KG_RUNTIME_RESOURCE_LIMIT_EXCEEDED");
        assert_eq!(runtime.runtime().gc().active_roots(), 0);
        assert!(!runtime.runtime().is_quarantined());
    }
    let loaded = runtime
        .load_program(
            &prepared(
                "repeat_value",
                vec![string(""), ConstantOperand::U64(u64::MAX)],
            ),
            Default::default(),
        )
        .unwrap();
    assert_eq!(
        runtime
            .execute(&loaded, "repeat_value", &[], &context)
            .unwrap()
            .return_value,
        Value::Str("".into())
    );
    let loaded = runtime
        .load_program(&prepared("", vec![]), Default::default())
        .unwrap();
    let error = runtime
        .execute(&loaded, "overfill_value", &[], &context)
        .unwrap_err();
    assert!(
        matches!(&error, EmbeddingError::Runtime { message, .. } if message.contains("reserved capacity")),
        "{error:?}"
    );
    assert_eq!(runtime.runtime().gc().active_roots(), 0);
    assert_eq!(
        runtime.runtime().resources().counters().current_call_depth,
        0
    );
}

#[test]
fn every_instruction_and_option_allocation_cut_cleans_up_and_preserves_eager_effects() {
    let context = ExecutionContext::default();
    let mut runtime = engine().runtime(context.clone());
    for (entry, operands) in [
        ("to_lowercase_value", vec![string("ΟΣ İ")]),
        ("to_uppercase_value", vec![string("straße")]),
        ("repeat_value", vec![string("é"), ConstantOperand::U64(4)]),
        (
            "replace_value",
            vec![string("é😀"), string(""), string("|")],
        ),
        ("split_once_value", vec![string("é😀é"), string("😀")]),
        ("strip_prefix_value", vec![string("éx"), string("absent")]),
        (
            "eager_value",
            vec![
                string("aa"),
                string("a"),
                string("é"),
                ConstantOperand::U64(1),
            ],
        ),
    ] {
        let loaded = runtime
            .load_program(&prepared(entry, operands), Default::default())
            .unwrap();
        let before = runtime.runtime().resources().counters();
        let expected = runtime
            .execute(&loaded, entry, &[], &context)
            .unwrap()
            .return_value;
        let after = runtime.runtime().resources().counters();
        let expected = if matches!(expected, Value::Enum(_)) {
            option(runtime.runtime(), expected)
        } else {
            Some(expected)
        };
        let expected_events = text::take_events();
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
                match runtime.execute(&loaded, entry, &[], &limited) {
                    Ok(report) => {
                        let value = if matches!(report.return_value, Value::Enum(_)) {
                            option(runtime.runtime(), report.return_value)
                        } else {
                            Some(report.return_value)
                        };
                        assert_eq!(value, expected);
                        assert_eq!(limit, cost);
                        succeeded = true;
                    }
                    Err(error) => assert_eq!(error.code(), "KG_RUNTIME_RESOURCE_LIMIT_EXCEEDED"),
                }
                let events = text::take_events();
                assert!(expected_events.starts_with(&events), "{entry}: {events:?}");
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
        assert!(runtime.execute(&loaded, entry, &[], &cancelled).is_err());
        assert!(text::take_events().is_empty());
        assert_eq!(runtime.runtime().gc().active_roots(), 0);
    }
}

#[test]
fn cancellation_after_text_reservation_is_observed_by_append_and_releases_state() {
    let context = ExecutionContext::default();
    let mut runtime = engine().runtime(context.clone());
    let loaded = runtime
        .load_program(&prepared("", vec![]), Default::default())
        .unwrap();
    let cancelled = ExecutionContext {
        cancellation: Default::default(),
        ..context.clone()
    };
    text::cancel_with(cancelled.cancellation.clone());
    let error = runtime
        .execute(&loaded, "cancel_value", &[], &cancelled)
        .unwrap_err();
    assert_eq!(error.code(), "KG_RUNTIME_CANCELLED");
    assert!(text::take_events().is_empty());
    assert_eq!(runtime.runtime().gc().active_roots(), 0);
    assert_eq!(
        runtime.runtime().resources().counters().current_call_depth,
        0
    );
    assert!(!runtime.runtime().is_quarantined());
    assert_eq!(
        runtime
            .execute(&loaded, "cancel_value", &[], &context)
            .unwrap()
            .return_value,
        Value::Str("x".into())
    );
    assert_eq!(text::take_events(), vec!["appended"]);
}

#[test]
fn length_dependent_native_work_is_charged_before_output_and_eager_arguments_are_once_only() {
    let context = ExecutionContext::default();
    let mut runtime = engine().runtime(context.clone());
    let mut costs = vec![];
    for text in ["x", "xxxxxxxxx"] {
        let loaded = runtime
            .load_program(
                &prepared("repeat_value", vec![string(text), ConstantOperand::U64(4)]),
                Default::default(),
            )
            .unwrap();
        let before = runtime.runtime().resources().counters().instruction_steps;
        runtime
            .execute(&loaded, "repeat_value", &[], &context)
            .unwrap();
        costs.push(runtime.runtime().resources().counters().instruction_steps - before);
    }
    assert_eq!(costs[1] - costs[0], 32);
    let loaded = runtime
        .load_program(&prepared("", vec![]), Default::default())
        .unwrap();
    assert_eq!(
        runtime
            .execute(&loaded, "eager_value", &[], &context)
            .unwrap()
            .return_value,
        Value::Str("éa".into())
    );
    assert_eq!(text::take_events(), vec!["aa", "a", "é"]);
}

#[cfg(feature = "native")]
mod native_backend {
    use super::*;
    use kagari_codegen_cranelift::CraneliftBackend;
    use kagari_vm::vm::{JitExecutionStatus, native::PreparedNativeEntry};

    #[test]
    fn source_free_prepared_backend_fallback_preserves_string_values_and_native_effects() {
        let mut context = ExecutionContext::default();
        context.language_profile.allow_jit = true;
        context.capabilities.jit = true;
        let mut runtime = engine().runtime(context.clone());
        let mut backend = CraneliftBackend::for_host().unwrap();
        for (entry, operands, expected) in [
            (
                "to_uppercase_value",
                vec![string("straße")],
                Value::Str("STRASSE".into()),
            ),
            (
                "split_once_value",
                vec![string("é😀"), string("")],
                Value::Tuple(vec![Value::Str("".into()), Value::Str("é😀".into())]),
            ),
            (
                "replace_value",
                vec![string("é😀"), string(""), string("|")],
                Value::Str("|é|😀|".into()),
            ),
            (
                "reverse_value",
                vec![string("é😀x")],
                Value::Str("x😀é".into()),
            ),
            (
                "eager_value",
                vec![
                    string("aa"),
                    string("a"),
                    string("é"),
                    ConstantOperand::U64(1),
                ],
                Value::Str("éa".into()),
            ),
        ] {
            let program = prepared(entry, operands);
            let loaded = runtime.load_program(&program, Default::default()).unwrap();
            let native = runtime
                .prepare_native(
                    &program,
                    &loaded,
                    entry,
                    &mut backend,
                    &context.cancellation,
                )
                .unwrap();
            // The backend currently falls back for native calls and inline text.
            // Exercise that real prepared-entry route rather than assuming JIT support.
            assert!(matches!(native, PreparedNativeEntry::Unsupported { .. }));
            let report = runtime
                .execute_prepared(&loaded, entry, &[], &context, &native)
                .unwrap();
            assert_eq!(
                report.jit.unwrap().status,
                JitExecutionStatus::InterpreterFallback
            );
            let value = if matches!(report.return_value, Value::Enum(_)) {
                option(runtime.runtime(), report.return_value).unwrap()
            } else {
                report.return_value
            };
            assert_eq!(value, expected);
            let events = text::take_events();
            if entry == "eager_value" {
                assert_eq!(events, vec!["aa", "a", "é"]);
            } else {
                assert!(events.is_empty());
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
}

#[cfg(feature = "source")]
mod source {
    use super::*;
    use kagari_common::{source::SourceFile, source_database::SourceLayer};

    #[test]
    fn an_application_text_provider_builds_and_runs_without_any_library_installation() {
        let engine = KagariEngine::builder()
            .install_standard_library(false)
            .install(text::native_api())
            .build()
            .unwrap();
        assert_eq!(engine.native_declaration_sources().len(), 1);
        let source = "use game::text::{Text, reverse}; fn main() -> Text { reverse(\"é😀x\") }";
        let artifact = engine
            .compile_to_artifact(
                SourceFile::new("memory://application-text.kgr", source),
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
            Value::Str("x😀é".into())
        );
        assert_eq!(runtime.runtime().gc().active_roots(), 0);
    }

    #[test]
    fn exact_product_and_native_methods_navigation_follow_registered_string_owner() {
        let engine = engine();
        let artifact = engine
            .compile_to_artifact(
                SourceFile::new(
                    "memory://native-string.kgr",
                    include_str!("fixtures/native_string.kgr"),
                ),
                Default::default(),
                Default::default(),
            )
            .unwrap();
        // Avoid dumping the binary payload when a dependency fingerprint changes.
        assert!(artifact.to_bytes().unwrap() == ARTIFACT);
        let text = "use std::string::String; fn main() -> String { \"é\".to_uppercase() }";
        let file = engine
            .set_source(
                "memory://string-navigation.kgr",
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
        let offset = text.find("to_uppercase").unwrap();
        let declaration = snapshot.definition_at(file, offset).unwrap();
        let source = snapshot.source(declaration.location.file).unwrap();
        assert_eq!(source.name(), "kagari://native/kagari-std/string.kgr");
        assert_eq!(
            &source.text()[declaration.location.range.start..declaration.location.range.end],
            "to_uppercase"
        );
        let docs = snapshot.documentation_at(file, offset).unwrap();
        assert!(docs.documentation.contains("multi-scalar"));
        assert!(
            docs.written_signature
                .contains("to_uppercase(self) -> String")
        );
    }

    #[test]
    fn direct_string_calls_reject_wrong_parameter_types_and_return_contexts() {
        let engine = engine();
        for text in [
            "fn main() { \"x\".repeat(1i32); }",
            "fn main() { \"x\".contains(1usize); }",
            "fn main() -> String { \"é\".slice(0usize, 1usize) }",
            "fn main() -> usize { \"x\".to_lowercase() }",
            "fn main() { String::find(1i32, \"x\"); }",
        ] {
            let text = format!("use std::string::String; {text}");
            assert!(
                engine
                    .compile_source(
                        SourceFile::new("memory://invalid-string.kgr", text),
                        Default::default()
                    )
                    .is_err()
            );
        }
    }
}
