//! Registered scalar protocols, selected script overrides and retained primitives.
#[path = "fixtures/native_scalar_protocol_api.rs"]
pub mod fixture_api;
use kagari_abi::{
    contracts::verify_intrinsic, representation::ValueType, scalar::BuiltinType,
    standard::RuntimePrimitive, types::AbiType,
};
use kagari_bytecode::{
    artifact::KbcArtifact,
    instruction::{BytecodeInstruction, ConstantOperand},
};
use kagari_embed::{
    context::ExecutionContext,
    engine::{EngineConfig, KagariEngine},
    program::PreparedProgram,
};
use kagari_runtime::{
    Runtime,
    native::{fmt_api::fmt, hash_api::hash},
    value::{MapKey, Value},
    value_semantics::format_value,
};
use std::collections::BTreeSet;

const ARTIFACT: &[u8] = include_bytes!("fixtures/native_scalar_protocol.kbc");
const ENTRIES: [&str; 6] = [
    "signed", "unsigned", "other", "custom", "implicit", "dynamic",
];
fn engine() -> KagariEngine {
    let mut config = EngineConfig::default();
    config.default_runtime.gc.collection_threshold = Some(1);
    KagariEngine::builder()
        .config(config)
        .install_standard_library(false)
        .install(fixture_api::api())
        .build()
        .unwrap()
}
fn program() -> PreparedProgram {
    PreparedProgram::from_artifact(
        KbcArtifact::from_bytes(ARTIFACT).unwrap(),
        &Default::default(),
        &Default::default(),
    )
    .unwrap()
}
fn clean(runtime: &Runtime) {
    assert_eq!(runtime.gc().active_roots(), 0);
    assert_eq!(runtime.resources().counters().current_call_depth, 0);
    assert!(!runtime.is_quarantined());
}
fn text_program(entry: &str, text: &str) -> PreparedProgram {
    let mut artifact = KbcArtifact::from_bytes(ARTIFACT).unwrap();
    let root = &mut artifact.program.modules[artifact.program.root.index()];
    let function = root
        .functions
        .iter_mut()
        .find(|function| function.name == entry)
        .unwrap();
    let operand = ConstantOperand::Str(text.into());
    let mut patched = 0;
    for instruction in &mut function.instructions {
        if let BytecodeInstruction::LoadConst { constant, .. } = instruction {
            assert!(matches!(constant, ConstantOperand::Str(_)));
            *constant = operand.clone();
            patched += 1;
        }
    }
    assert_eq!(patched, 1);
    root.constants.push(operand);
    let artifact = KbcArtifact::from_program(artifact.program, Default::default()).unwrap();
    PreparedProgram::from_artifact(
        KbcArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap(),
        &Default::default(),
        &Default::default(),
    )
    .unwrap()
}
#[test]
fn registrations_own_complete_scalar_facts_and_generated_contract_views() {
    let hash = hash::native_api().unwrap();
    let fmt = fmt::native_api().unwrap();
    assert_eq!(hash.modules()[0].traits.len(), 1);
    assert_eq!(hash.modules()[0].implementations.len(), 14);
    assert_eq!(hash.modules()[0].native_declarations().len(), 14);
    assert_eq!(fmt.modules()[0].traits.len(), 2);
    assert_eq!(fmt.modules()[0].implementations.len(), 31);
    assert_eq!(fmt.modules()[0].native_declarations().len(), 31);
    assert_eq!(
        fmt.modules()[0]
            .traits
            .iter()
            .map(|item| item.name.as_str())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["Debug", "Display"])
    );
    let floats = [
        AbiType::Builtin(BuiltinType::F32),
        AbiType::Builtin(BuiltinType::F64),
    ];
    assert!(
        hash.modules()[0]
            .implementations
            .iter()
            .all(|item| !floats.contains(&item.for_type))
    );
    assert_eq!(
        hash.declaration_sources()[0].text,
        include_str!("../../../stdlib/hash.kgr")
    );
    assert_eq!(
        fmt.declaration_sources()[0].text,
        include_str!("../../../stdlib/fmt.kgr")
    );
    let artifact = KbcArtifact::from_bytes(ARTIFACT).unwrap();
    for owner in ["hash", "fmt"] {
        assert!(
            artifact
                .program
                .modules
                .iter()
                .flat_map(|module| &module.native_imports)
                .any(|import| import.instance.declaration.module.path == [owner])
        );
    }
}
#[test]
fn closed_hash_contract_accepts_unit_and_objects_and_rejects_float_and_host_operands() {
    for ty in [
        ValueType::Unit,
        ValueType::Bool,
        ValueType::I32,
        ValueType::I64,
        ValueType::U64,
        ValueType::Str,
        ValueType::HeapObject,
    ] {
        verify_intrinsic(Some(ValueType::I64), RuntimePrimitive::ValueHash, &[ty]).unwrap();
    }
    for ty in [ValueType::F32, ValueType::F64, ValueType::HostHandle] {
        assert!(
            verify_intrinsic(Some(ValueType::I64), RuntimePrimitive::ValueHash, &[ty]).is_err()
        );
    }
}
#[test]
fn offline_scalar_methods_selected_native_calls_script_overrides_and_implicit_composition_execute()
{
    let context = ExecutionContext::default();
    let mut runtime = engine().runtime(context.clone());
    let loaded = runtime
        .load_program(&program(), Default::default())
        .unwrap();
    for entry in ENTRIES {
        assert_eq!(
            runtime
                .execute(&loaded, entry, &[], &context)
                .unwrap()
                .return_value,
            Value::Bool(true),
            "{entry}"
        );
        clean(runtime.runtime());
        runtime.runtime().collect_garbage().unwrap();
        assert_eq!(runtime.runtime().gc().allocated_objects(), 0);
    }
}
#[test]
fn text_format_and_hash_results_match_language_rules_and_preserve_utf8_and_escapes() {
    let context = ExecutionContext::default();
    let mut runtime = engine().runtime(context.clone());
    for text in ["", "é😀", "quote\" slash\\\n\t\0", "line\r\n"] {
        let value = Value::Str(text.into());
        for (entry, expected) in [
            (
                "string_debug",
                Value::Str(format_value(runtime.runtime().gc(), &value, true).unwrap()),
            ),
            ("string_display", Value::Str(text.into())),
            (
                "string_hash",
                Value::I64(
                    MapKey::from_value(runtime.runtime().gc(), &value)
                        .unwrap()
                        .script_hash(),
                ),
            ),
        ] {
            let loaded = runtime
                .load_program(&text_program(entry, text), Default::default())
                .unwrap();
            assert_eq!(
                runtime
                    .execute(&loaded, entry, &[], &context)
                    .unwrap()
                    .return_value,
                expected
            );
            clean(runtime.runtime());
        }
    }
}
#[test]
fn every_budget_cut_releases_selected_calls_and_object_roots() {
    let context = ExecutionContext::default();
    let mut runtime = engine().runtime(context.clone());
    let loaded = runtime
        .load_program(&program(), Default::default())
        .unwrap();
    for entry in ENTRIES {
        let before = runtime.runtime().resources().counters().instruction_steps;
        runtime.execute(&loaded, entry, &[], &context).unwrap();
        let cost = runtime.runtime().resources().counters().instruction_steps - before;
        let mut finished = false;
        for limit in 0..=cost {
            let mut limited = context.clone();
            limited.resources.max_instruction_steps = Some(limit);
            match runtime.execute(&loaded, entry, &[], &limited) {
                Ok(report) => {
                    assert_eq!(limit, cost);
                    assert_eq!(report.return_value, Value::Bool(true));
                    finished = true;
                }
                Err(error) => assert_eq!(error.code(), "KG_RUNTIME_RESOURCE_LIMIT_EXCEEDED"),
            }
            clean(runtime.runtime());
            runtime.runtime().collect_garbage().unwrap();
            assert_eq!(runtime.runtime().gc().allocated_objects(), 0);
        }
        assert!(finished, "{entry}");
    }
}
#[test]
fn scalar_text_work_charges_input_and_escaped_output_and_format_limits_trap_cleanly() {
    let context = ExecutionContext::default();
    let mut runtime = engine().runtime(context.clone());
    for (entry, long, difference) in [
        ("string_hash", "x".repeat(64), 64),
        ("string_display", "x".repeat(64), 128),
        ("string_debug", "\n".repeat(64), 192),
    ] {
        let mut costs = vec![];
        for text in ["", long.as_str()] {
            let loaded = runtime
                .load_program(&text_program(entry, text), Default::default())
                .unwrap();
            let before = runtime.runtime().resources().counters().instruction_steps;
            runtime.execute(&loaded, entry, &[], &context).unwrap();
            costs.push(runtime.runtime().resources().counters().instruction_steps - before);
            clean(runtime.runtime());
        }
        assert_eq!(costs[1] - costs[0], difference, "{entry}");
    }
    for (entry, text) in [
        ("string_display", "x".repeat(1_048_577)),
        ("string_debug", "\"".repeat(600_000)),
    ] {
        let loaded = runtime
            .load_program(&text_program(entry, &text), Default::default())
            .unwrap();
        assert_eq!(
            runtime
                .execute(&loaded, entry, &[], &context)
                .unwrap_err()
                .code(),
            "KG_RUNTIME_SCRIPT_TRAP"
        );
        clean(runtime.runtime());
        let mut limited = context.clone();
        limited.resources.max_instruction_steps = Some(100);
        assert_eq!(
            runtime
                .execute(&loaded, entry, &[], &limited)
                .unwrap_err()
                .code(),
            "KG_RUNTIME_RESOURCE_LIMIT_EXCEEDED"
        );
        clean(runtime.runtime());
    }
}

#[cfg(feature = "native")]
mod backend {
    use super::*;
    use kagari_codegen_cranelift::CraneliftBackend;
    use kagari_vm::vm::{JitExecutionStatus, native::PreparedNativeEntry};
    #[test]
    fn prepared_native_fallback_preserves_registered_and_implicit_protocol_results() {
        let mut context = ExecutionContext::default();
        context.language_profile.allow_jit = true;
        context.capabilities.jit = true;
        let mut runtime = engine().runtime(context.clone());
        let program = program();
        let loaded = runtime.load_program(&program, Default::default()).unwrap();
        let mut backend = CraneliftBackend::for_host().unwrap();
        for entry in ENTRIES {
            let native = runtime
                .prepare_native(
                    &program,
                    &loaded,
                    entry,
                    &mut backend,
                    &context.cancellation,
                )
                .unwrap();
            assert!(matches!(native, PreparedNativeEntry::Unsupported { .. }));
            let report = runtime
                .execute_prepared(&loaded, entry, &[], &context, &native)
                .unwrap();
            assert_eq!(report.return_value, Value::Bool(true));
            assert_eq!(
                report.jit.unwrap().status,
                JitExecutionStatus::InterpreterFallback
            );
            clean(runtime.runtime());
        }
    }
}
#[test]
fn offline_verification_rejects_forged_selected_hash_return_types() {
    let mut artifact = KbcArtifact::from_bytes(ARTIFACT).unwrap();
    let import = artifact
        .program
        .modules
        .iter_mut()
        .flat_map(|module| &mut module.native_imports)
        .find(|import| import.instance.declaration.module.path == ["hash"])
        .unwrap();
    import.signature.result = AbiType::Builtin(BuiltinType::Bool);
    assert!(KbcArtifact::from_program(artifact.program, Default::default()).is_err());
}

#[cfg(feature = "source")]
mod source {
    use super::*;
    use kagari_common::{source::SourceFile, source_database::SourceLayer};
    #[test]
    fn source_product_and_tooling_use_actual_registration_signatures() {
        let engine = engine();
        let artifact = engine
            .compile_to_artifact(
                SourceFile::new(
                    "memory://native-scalar-protocol.kgr",
                    include_str!("fixtures/native_scalar_protocol.kgr"),
                ),
                Default::default(),
                Default::default(),
            )
            .unwrap();
        assert!(artifact.to_bytes().unwrap() == ARTIFACT);
        let text = "use std::hash::Hash; use std::fmt::Debug; fn main() -> i64 { \"é\".hash() }";
        let file = engine
            .set_source(
                "memory://scalar-navigation.kgr",
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
                .is_empty()
        );
        for (offset, name, uri) in [
            (text.find("Hash;").unwrap(), "Hash", "hash"),
            (text.find("Debug;").unwrap(), "Debug", "fmt"),
        ] {
            let definition = snapshot.definition_at(file, offset).unwrap();
            let source = snapshot.source(definition.location.file).unwrap();
            assert_eq!(
                source.name(),
                format!("kagari://native/kagari-std/{uri}.kgr")
            );
            assert_eq!(
                &source.text()[definition.location.range.start..definition.location.range.end],
                name
            );
        }
    }
    #[test]
    fn static_checks_preserve_hash_eligibility_and_format_result_types() {
        for body in [
            "1.0f32.hash()",
            "stamp(1.0f64)",
            "val value: i64 = 1i32.debug()",
            "val value: bool = 1i32.display()",
            "val value = Payload { number: 1 }; value.display()",
        ] {
            assert!(
                engine()
                    .compile_to_artifact(
                        SourceFile::new(
                            "memory://invalid-scalar-protocol.kgr",
                            format!(
                                "{} fn invalid() {{ {body}; }}",
                                include_str!("fixtures/native_scalar_protocol.kgr")
                            )
                        ),
                        Default::default(),
                        Default::default()
                    )
                    .is_err(),
                "{body}"
            );
        }
    }
    #[test]
    fn application_protocol_and_scalar_function_work_without_any_standard_package() {
        let engine = KagariEngine::builder()
            .install_standard_library(false)
            .install(fixture_api::presentation::native_api())
            .build()
            .unwrap();
        assert_eq!(engine.native_declaration_sources().len(), 1);
        let artifact = engine.compile_to_artifact(SourceFile::new("memory://application-render.kgr", "use game::presentation::{Text, Render, fingerprint}; fn main() -> Text { \"é😀\".render() } fn hash() -> i64 { fingerprint(\"é😀\") }"), Default::default(), Default::default()).unwrap();
        let program = PreparedProgram::from_artifact(
            KbcArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap(),
            &Default::default(),
            &Default::default(),
        )
        .unwrap();
        let context = ExecutionContext::default();
        let mut runtime = engine.runtime(context.clone());
        let loaded = runtime.load_program(&program, Default::default()).unwrap();
        assert_eq!(
            runtime
                .execute(&loaded, "main", &[], &context)
                .unwrap()
                .return_value,
            Value::Str("\"é😀\"".into())
        );
        assert_eq!(
            runtime
                .execute(&loaded, "hash", &[], &context)
                .unwrap()
                .return_value,
            Value::I64(
                MapKey::from_value(runtime.runtime().gc(), &Value::Str("é😀".into()))
                    .unwrap()
                    .script_hash()
            )
        );
        clean(runtime.runtime());
    }
}
