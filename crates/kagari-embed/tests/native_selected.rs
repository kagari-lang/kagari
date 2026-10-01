//! Typed selected dependencies share checked applications and rooted callbacks.
// Test/cross-target sharing keeps the fixture registration authoritative.
#[path = "fixtures/native_selected_api.rs"]
mod fixture_api;
use fixture_api::{consumer, selected};
use kagari_abi::{
    scalar::BuiltinType,
    types::{AbiType, PublicAbiItem},
};
use kagari_bytecode::artifact::KbcArtifact;
use kagari_common::identity::DefinitionKind;
use kagari_embed::{
    context::ExecutionContext,
    engine::{EngineConfig, KagariEngine},
    program::PreparedProgram,
};
use kagari_native_macros::native_module;
use kagari_runtime::{
    Runtime, RuntimeConfig,
    native::{api::NativeApi, catalog::NativeCatalog},
    value::Value,
};
use std::collections::BTreeMap;

const ARTIFACT: &[u8] = include_bytes!("fixtures/native_selected.kbc");

#[test]
fn carried_trait_contracts_match_registered_native_declarations() {
    let artifact = KbcArtifact::from_bytes(ARTIFACT).unwrap();
    let api = fixture_api::api().unwrap();
    for module in api.modules() {
        for expected in &module.traits {
            let actual = artifact
                .program
                .modules
                .iter()
                .filter(|actual| actual.identity == module.identity)
                .flat_map(|actual| &actual.public_items)
                .find_map(|item| match item {
                    PublicAbiItem::Trait(contract) if contract.name == expected.name => {
                        Some(contract)
                    }
                    _ => None,
                })
                .unwrap();
            assert_eq!(actual, expected);
        }
    }
}

fn trait_only_provider(change_type: bool) -> NativeApi {
    let provider = selected::native_api().unwrap();
    let mut module = provider.modules()[0].as_ref().clone();
    module.types.clear();
    module.implementations.clear();
    module.functions.clear();
    module.callable_requirements.clear();
    let parameter = &mut module.traits[0].methods[1].params[1];
    if change_type {
        parameter.ty = AbiType::Builtin(BuiltinType::Bool);
    } else {
        parameter.name = "adjustment".into();
    }
    NativeApi::new(vec![module], vec![], Default::default()).unwrap()
}

#[test]
fn external_catalogs_reject_missing_conflicting_and_rust_incompatible_contracts() {
    let provider = selected::native_api().unwrap();
    assert!(consumer::native_api(&NativeCatalog::default()).is_err());
    let consumer = consumer::native_api(&provider.catalog()).unwrap();
    assert!(NativeApi::combine(vec![consumer.clone()]).is_err());
    let changed = trait_only_provider(true);
    assert!(NativeCatalog::from_apis(&[&provider, &changed]).is_err());
    assert!(consumer::native_api(&changed.catalog()).is_err());
    assert!(NativeApi::combine(vec![changed, consumer]).is_err());
    let shared = NativeCatalog::from_apis(&[&provider, &provider]).unwrap();
    assert!(consumer::native_api(&shared).is_ok());
}

#[test]
fn failed_dependency_installation_does_not_publish_partial_handlers_or_traits() {
    let provider = selected::native_api().unwrap();
    let consumer = consumer::native_api(&provider.catalog()).unwrap();
    let mut runtime = Runtime::new(RuntimeConfig::default());
    assert!(consumer.install(&mut runtime).is_err());
    provider.install(&mut runtime).unwrap();
    consumer.install(&mut runtime).unwrap();
    assert!(consumer.install(&mut runtime).is_err());
}

#[test]
fn external_implementation_signatures_are_checked_before_package_publication() {
    let provider = selected::native_api().unwrap();
    let consumer = consumer::native_api(&provider.catalog()).unwrap();
    for case in 0..3 {
        let mut module = consumer.modules()[0].as_ref().clone();
        match case {
            0 => {
                module.implementations[0].methods[0].return_type =
                    AbiType::Builtin(BuiltinType::Bool)
            }
            1 => {
                module.implementations[0].methods.pop();
            }
            2 => {
                module.implementations[0].methods[1].params[1].ty =
                    AbiType::Builtin(BuiltinType::Bool)
            }
            _ => unreachable!(),
        }
        // A raw module's foreign method contracts are resolved at composition.
        // Compare against the registered provider without executing a factory.
        let mut contracts = BTreeMap::new();
        for declared in provider.modules() {
            for contract in &declared.traits {
                contracts.insert(
                    declared.definition(DefinitionKind::Trait, &contract.name),
                    contract.clone(),
                );
            }
        }
        assert!(
            module.validate_trait_implementations(&contracts).is_err(),
            "{case}"
        );
    }
}

fn engine(defaults: bool) -> KagariEngine {
    let mut config = EngineConfig::default();
    config.default_runtime.gc.collection_threshold = Some(1);
    KagariEngine::builder()
        .config(config)
        .install_standard_library(defaults)
        .install(fixture_api::api())
        .build()
        .unwrap()
}
fn prepared(bytes: &[u8]) -> PreparedProgram {
    PreparedProgram::from_artifact(
        KbcArtifact::from_bytes(bytes).unwrap(),
        &Default::default(),
        &Default::default(),
    )
    .unwrap()
}

#[test]
fn typed_requirements_generate_bounds_and_exclude_injected_arguments() {
    let api = selected::native_api().unwrap();
    let module = &api.modules()[0];
    let source = &api.declaration_sources()[0].text;
    assert!(source.contains("fn invoke<T0>(value: T0)"));
    assert!(
        source.contains("where T0: Echo<Output = Bag<i32>>"),
        "{source}"
    );
    assert!(!source.contains("NativeSelected"));
    assert_eq!(module.callable_requirements.len(), 3);
    let both = module
        .functions
        .iter()
        .find(|function| function.name == "invoke_both")
        .unwrap();
    assert_eq!(both.bounds.len(), 1);
    let function = module
        .functions
        .iter()
        .find(|function| function.name == "invoke")
        .unwrap();
    assert_eq!(function.params.len(), 1);
    assert_eq!(function.bounds.len(), 1);
}

#[test]
fn typed_native_and_private_script_requirements_execute_offline_without_defaults() {
    let engine = engine(false);
    let program = prepared(ARTIFACT);
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime.load_program(&program, Default::default()).unwrap();
    for entry in [
        "script_main",
        "native_main",
        "method_main",
        "both_script_main",
        "both_native_main",
        "external_script_main",
        "external_native_main",
        "external_method_main",
        "external_impl_main",
        "external_dynamic_main",
    ] {
        assert_eq!(
            runtime
                .execute(&loaded, entry, &[], &context)
                .unwrap()
                .return_value,
            Value::I32(42)
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
fn typed_selected_budget_cuts_release_retained_arguments_results_and_frames() {
    let engine = engine(false);
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime
        .load_program(&prepared(ARTIFACT), Default::default())
        .unwrap();
    for entry in [
        "script_main",
        "native_main",
        "method_main",
        "both_script_main",
        "both_native_main",
        "external_script_main",
        "external_native_main",
        "external_method_main",
        "external_impl_main",
        "external_dynamic_main",
    ] {
        let mut finished = false;
        for limit in 0..100 {
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
        assert!(finished, "budget sweep reaches {entry}");
    }
}

#[native_module("game::wrong_selected")]
pub mod wrong_selected {
    use kagari_runtime::native_value::{NativeValue, selected::NativeSelected};
    #[native_trait]
    pub trait Check {
        fn check(&self);
    }
    #[native]
    pub fn wrong<T: NativeValue>(
        value: T,
        #[selected(T: Check::check)] check: NativeSelected<(T,), bool>,
    ) {
        drop((value, check));
    }
}

#[native_module("game::missing_selected")]
pub mod missing_selected {
    use kagari_runtime::native_value::{NativeValue, selected::NativeSelected};
    #[native_trait]
    pub trait Check {
        fn check(&self);
    }
    #[native]
    pub fn wrong<T: NativeValue>(
        value: T,
        #[selected(T: Check::missing)] check: NativeSelected<(T,), ()>,
    ) {
        drop((value, check));
    }
}

#[native_module("game::wrong_selected_pack")]
pub mod wrong_selected_pack {
    use kagari_runtime::native_value::{NativeValue, selected::NativeSelected};
    #[native_trait]
    pub trait Check {
        fn check(&self);
    }
    #[native]
    pub fn wrong<T: NativeValue>(
        value: T,
        #[selected(T: Check::check)] check: NativeSelected<(), ()>,
    ) {
        drop((value, check));
    }
}

#[test]
fn typed_selection_rejects_wrong_results_members_and_receiver_packs_at_registration() {
    assert!(wrong_selected::native_api().is_err());
    assert!(missing_selected::native_api().is_err());
    assert!(wrong_selected_pack::native_api().is_err());
}

#[cfg(feature = "source")]
mod source {
    use super::*;
    use kagari_common::source::SourceFile;
    use kagari_embed::error::EmbeddingError;
    const SOURCE: &str = include_str!("fixtures/native_selected.kgr");

    #[test]
    fn source_emission_matches_typed_selected_fixture() {
        let artifact = engine(true)
            .compile_to_artifact(
                SourceFile::new("memory://native-selected.kgr", SOURCE),
                Default::default(),
                Default::default(),
            )
            .unwrap();
        assert_eq!(artifact.to_bytes().unwrap(), ARTIFACT);
    }

    #[test]
    fn typed_selected_private_targets_keep_their_generation_after_reload() {
        let engine = engine(true);
        let artifact = engine
            .compile_to_artifact(
                SourceFile::new(
                    "memory://native-selected.kgr",
                    SOURCE.replace("{ [self] }", "{ [self + 1] }"),
                ),
                Default::default(),
                Default::default(),
            )
            .unwrap();
        let candidate = prepared(&artifact.to_bytes().unwrap());
        let context = ExecutionContext::default();
        let mut runtime = engine.runtime(context.clone());
        let old = runtime
            .load_program(&prepared(ARTIFACT), Default::default())
            .unwrap();
        let current = runtime
            .reload_program(&old, &candidate, Default::default())
            .unwrap();
        for (program, expected) in [(&old, 42), (&current, 43)] {
            for entry in ["script_main", "external_script_main"] {
                assert_eq!(
                    runtime
                        .execute(program, entry, &[], &context)
                        .unwrap()
                        .return_value,
                    Value::I32(expected)
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
    }

    #[test]
    fn a_checked_product_cannot_replace_the_registered_dependency_contract() {
        let changed = trait_only_provider(false);
        let consumer = consumer::native_api(&changed.catalog()).unwrap();
        let compiler = KagariEngine::builder()
            .install_standard_library(false)
            .install(Ok(changed))
            .install(Ok(consumer))
            .build()
            .unwrap();
        let artifact = compiler.compile_to_artifact(
            SourceFile::new("memory://changed-native-contract.kgr", "use game::external_selected::external_scalar; fn main() -> i32 { external_scalar(true) }"),
            Default::default(), Default::default(),
        ).unwrap();
        let program = prepared(&artifact.to_bytes().unwrap());
        let mut runtime = engine(false).runtime(ExecutionContext::default());
        let error = runtime
            .load_program(&program, Default::default())
            .unwrap_err();
        assert!(
            matches!(&error, EmbeddingError::Load { error } if error.to_string().contains("native dependency differs from its registered trait contract")),
            "{error:?}"
        );
        assert_eq!(runtime.runtime().gc().active_roots(), 0);
        assert_eq!(
            runtime.runtime().resources().counters().current_call_depth,
            0
        );
    }

    #[test]
    fn inferred_selection_bounds_reject_missing_or_wrong_associated_outputs() {
        let engine = engine(true);
        for body in ["invoke(true)", "invoke([42])", "[true].invoke_first()"] {
            let text = format!("use game::selected::invoke; fn main() {{ {body}; }}");
            assert!(
                engine
                    .compile_to_artifact(
                        SourceFile::new("memory://invalid-selection.kgr", text),
                        Default::default(),
                        Default::default()
                    )
                    .is_err(),
                "{body}"
            );
        }
    }

    #[test]
    fn typed_selected_callback_traps_and_factory_failures_release_frames_and_roots() {
        let engine = engine(true);
        let text = SOURCE.replace(
            "fn echo(self) -> ArrayList<i32> { [self] }",
            "fn echo(self) -> ArrayList<i32> { val values = [self]; values[99usize]; values }",
        );
        let text = format!(
            "{text}\nfn empty_method() -> i32 {{ val values: ArrayList<ArrayList<ArrayList<i32>>> = []; values.invoke_first()[0usize] }}"
        );
        let artifact = engine
            .compile_to_artifact(
                SourceFile::new("memory://selected-trap.kgr", text),
                Default::default(),
                Default::default(),
            )
            .unwrap();
        let program = prepared(&artifact.to_bytes().unwrap());
        let context = ExecutionContext::default();
        let mut runtime = engine.runtime(context.clone());
        let loaded = runtime.load_program(&program, Default::default()).unwrap();
        for entry in ["script_main", "empty_method"] {
            assert!(runtime.execute(&loaded, entry, &[], &context).is_err());
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
