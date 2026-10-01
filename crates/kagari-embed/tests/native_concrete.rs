//! Concrete default obligations use actual implementations in foreign packages.
#[path = "fixtures/native_concrete_api.rs"]
pub mod fixture_api;

use kagari_abi::{scalar::BuiltinType, types::AbiType};
use kagari_bytecode::{artifact::KbcArtifact, instruction::BytecodeInstruction};
use kagari_embed::{
    context::ExecutionContext,
    engine::{EngineConfig, KagariEngine},
    program::PreparedProgram,
};
use kagari_runtime::{
    Runtime, RuntimeConfig,
    native::{api::NativeApi, catalog::NativeCatalog},
    value::Value,
};

const ARTIFACT: &[u8] = include_bytes!("fixtures/native_concrete.kbc");
const ENTRIES: [&str; 5] = [
    "direct_native",
    "dynamic_native",
    "direct_script",
    "dynamic_script",
    "selected_default",
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

fn prepared() -> PreparedProgram {
    PreparedProgram::from_artifact(
        KbcArtifact::from_bytes(ARTIFACT).unwrap(),
        &Default::default(),
        &Default::default(),
    )
    .unwrap()
}

#[test]
fn a_trait_declaration_alone_cannot_discharge_a_concrete_default_obligation() {
    let contract = fixture_api::contract::native_api().unwrap();
    assert!(fixture_api::defaults::native_api(&contract.catalog()).is_err());
    let provider = fixture_api::provider::native_api(&contract.catalog()).unwrap();
    let defaults = fixture_api::defaults::native_api(&provider.catalog()).unwrap();
    assert!(NativeApi::combine(vec![contract.clone(), defaults.clone()]).is_err());
    assert!(NativeApi::combine(vec![provider.clone(), defaults.clone()]).is_err());
    assert!(NativeApi::combine(vec![contract, provider, defaults]).is_ok());
}

#[test]
fn actual_implementation_headers_are_part_of_the_retained_contract() {
    let contract = fixture_api::contract::native_api().unwrap();
    let provider = fixture_api::provider::native_api(&contract.catalog()).unwrap();
    let changed = fixture_api::changed_provider::native_api(&contract.catalog()).unwrap();
    assert_eq!(provider.modules()[0].traits, changed.modules()[0].traits);
    assert!(NativeCatalog::from_apis(&[&provider, &changed]).is_err());
    assert!(fixture_api::defaults::native_api(&changed.catalog()).is_err());
    let defaults = fixture_api::defaults::native_api(&provider.catalog()).unwrap();
    assert!(NativeApi::combine(vec![contract, changed, defaults]).is_err());
}

#[test]
fn failed_dependency_installation_does_not_publish_entries() {
    let contract = fixture_api::contract::native_api().unwrap();
    let provider = fixture_api::provider::native_api(&contract.catalog()).unwrap();
    let defaults = fixture_api::defaults::native_api(&provider.catalog()).unwrap();
    let mut runtime = Runtime::new(RuntimeConfig::default());
    assert!(defaults.install(&mut runtime).is_err());
    contract.install(&mut runtime).unwrap();
    assert!(defaults.install(&mut runtime).is_err());
    provider.install(&mut runtime).unwrap();
    defaults.install(&mut runtime).unwrap();
    assert!(defaults.install(&mut runtime).is_err());

    let changed = fixture_api::changed_provider::native_api(&contract.catalog()).unwrap();
    let mut other = Runtime::new(RuntimeConfig::default());
    contract.install(&mut other).unwrap();
    changed.install(&mut other).unwrap();
    assert!(defaults.install(&mut other).is_err());
}

#[test]
fn offline_direct_dynamic_and_selected_defaults_keep_concrete_callback_facts() {
    let engine = engine();
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime
        .load_program(&prepared(), Default::default())
        .unwrap();
    for entry in ENTRIES {
        assert_eq!(
            runtime
                .execute(&loaded, entry, &[], &context)
                .unwrap()
                .return_value,
            Value::I32(42),
            "{entry}"
        );
        assert_eq!(runtime.runtime().gc().active_roots(), 0);
        assert_eq!(
            runtime.runtime().resources().counters().current_call_depth,
            0
        );
        runtime.runtime().collect_garbage().unwrap();
        assert_eq!(runtime.runtime().gc().allocated_objects(), 0);
    }
}

#[test]
fn every_budget_cut_cleans_up_the_nested_default_and_callback() {
    let engine = engine();
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime
        .load_program(&prepared(), Default::default())
        .unwrap();
    for entry in ENTRIES {
        let mut finished = false;
        for limit in 0..150 {
            let mut limited = context.clone();
            limited.resources.max_instruction_steps = Some(limit);
            match runtime.execute(&loaded, entry, &[], &limited) {
                Ok(report) => {
                    assert_eq!(report.return_value, Value::I32(42));
                    finished = true;
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
            if finished {
                break;
            }
        }
        assert!(finished, "{entry}");
    }
}

#[test]
fn dependency_edges_preserve_the_provider_without_importing_unrelated_functions() {
    let artifact = KbcArtifact::from_bytes(ARTIFACT).unwrap();
    assert!(
        artifact
            .program
            .modules
            .iter()
            .any(|module| module.identity.path == ["fact_provider"])
    );
    assert!(
        artifact
            .program
            .modules
            .iter()
            .flat_map(|module| &module.native_imports)
            .all(|import| import
                .instance
                .declaration
                .path
                .iter()
                .all(|part| part.name != "unrelated"))
    );
}

#[test]
fn concrete_boxing_preserves_integer_width_and_rejects_a_forged_input_type() {
    let mut artifact = KbcArtifact::from_bytes(ARTIFACT).unwrap();
    let function = artifact
        .program
        .modules
        .iter_mut()
        .flat_map(|module| &mut module.functions)
        .find(|function| function.name == "dynamic_native")
        .unwrap();
    let register = function
        .instructions
        .iter()
        .find_map(|instruction| {
            if let BytecodeInstruction::MakeInterface { value, .. } = instruction {
                Some(value.index())
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(
        function.metadata.semantic.registers[&register],
        AbiType::Builtin(BuiltinType::USize)
    );
    function
        .metadata
        .semantic
        .registers
        .insert(register, AbiType::Builtin(BuiltinType::U64));
    // Isolate the portable bytecode verifier; absence of an optional MIR payload
    // is a supported input in every SDK feature route.
    artifact.portable_mir = None;
    let error = PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default())
        .unwrap_err();
    assert!(
        format!("{error:?}").contains("invalid collection access flow"),
        "{error:?}"
    );
}

#[cfg(feature = "source")]
mod source {
    use super::*;
    use kagari_common::source::SourceFile;

    #[test]
    fn source_emission_matches_the_concrete_default_product() {
        let artifact = engine()
            .compile_to_artifact(
                SourceFile::new(
                    "memory://native-concrete.kgr",
                    include_str!("fixtures/native_concrete.kgr"),
                ),
                Default::default(),
                Default::default(),
            )
            .unwrap();
        assert_eq!(artifact.to_bytes().unwrap(), ARTIFACT);
    }

    #[test]
    fn registered_fixed_predicates_do_not_change_source_bound_target_rules() {
        let error = engine()
            .compile_to_artifact(
                SourceFile::new(
                    "memory://invalid-concrete-bound.kgr",
                    "use game::fact_contract::Check; fn main() -> i32 where bool: Check { 42 }",
                ),
                Default::default(),
                Default::default(),
            )
            .unwrap_err();
        assert!(
            format!("{error:?}").contains("KG_TYPE_INVALID_BOUND_TARGET"),
            "{error:?}"
        );
    }
}
