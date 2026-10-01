//! External defaults retain parents and private template contracts even without callbacks.
#[path = "fixtures/native_default_external_api.rs"]
pub mod fixture_api;

use kagari_bytecode::artifact::KbcArtifact;
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

const ARTIFACT: &[u8] = include_bytes!("fixtures/native_default_external.kbc");

fn engine(changed: bool) -> KagariEngine {
    let mut config = EngineConfig::default();
    config.default_runtime.gc.collection_threshold = Some(1);
    KagariEngine::builder()
        .config(config)
        .install_standard_library(false)
        .install(fixture_api::api(changed))
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
fn parents_and_templates_require_actual_providers_without_selected_parameters() {
    let parent = fixture_api::game::parent::native_api().unwrap();
    for child in [
        fixture_api::child::native_api(&NativeCatalog::default()),
        fixture_api::plain::native_api(&NativeCatalog::default()),
    ] {
        assert!(child.is_err());
    }
    let catalog = parent.catalog();
    let child = fixture_api::child::native_api(&catalog).unwrap();
    let plain = fixture_api::plain::native_api(&catalog).unwrap();
    assert!(NativeApi::combine(vec![child.clone()]).is_err());
    assert!(NativeApi::combine(vec![plain.clone()]).is_err());
    assert!(NativeApi::combine(vec![child.clone(), plain.clone()]).is_err());
    // A consumer's view retains the expected parent but cannot install its entries.
    let indirect = fixture_api::plain::native_api(&child.catalog()).unwrap();
    assert!(NativeApi::combine(vec![child, indirect]).is_err());
    assert!(NativeApi::combine(vec![plain, parent]).is_ok());
}

#[test]
fn an_owned_default_cannot_publish_without_its_actual_template_declaration() {
    let parent = fixture_api::game::parent::native_api().unwrap();
    let mut missing = parent.modules()[0].as_ref().clone();
    missing.functions.clear();
    missing.private_functions.clear();
    missing.callable_requirements.clear();
    missing.validate().unwrap();
    let error = NativeApi::new(vec![missing], vec![], NativeCatalog::default()).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("native default template dependency is absent"),
        "{error}"
    );
}

#[test]
fn equal_trait_signatures_do_not_hide_a_changed_private_template_contract() {
    let parent = fixture_api::game::parent::native_api().unwrap();
    let changed = fixture_api::changed_parent::native_api().unwrap();
    assert_eq!(parent.modules()[0].traits, changed.modules()[0].traits);
    assert_ne!(
        parent.modules()[0].native_declarations(),
        changed.modules()[0].native_declarations()
    );
    assert!(NativeCatalog::from_apis(&[&parent, &changed]).is_err());
    assert!(NativeCatalog::from_apis(&[&parent, &parent]).is_ok());
    let child = fixture_api::child::native_api(&parent.catalog()).unwrap();
    let plain = fixture_api::plain::native_api(&parent.catalog()).unwrap();
    assert!(NativeApi::combine(vec![changed.clone(), child]).is_err());
    assert!(NativeApi::combine(vec![changed, plain]).is_err());
    assert!(fixture_api::api(true).is_ok());
}

#[test]
fn missing_or_changed_dependencies_do_not_publish_partial_installations() {
    let parent = fixture_api::game::parent::native_api().unwrap();
    let child = fixture_api::child::native_api(&parent.catalog()).unwrap();
    let plain = fixture_api::plain::native_api(&parent.catalog()).unwrap();
    let mut runtime = Runtime::new(RuntimeConfig::default());
    assert!(child.install(&mut runtime).is_err());
    assert!(plain.install(&mut runtime).is_err());
    parent.install(&mut runtime).unwrap();
    child.install(&mut runtime).unwrap();
    plain.install(&mut runtime).unwrap();

    let changed = fixture_api::changed_parent::native_api().unwrap();
    let changed_plain = fixture_api::plain::native_api(&changed.catalog()).unwrap();
    let mut other = Runtime::new(RuntimeConfig::default());
    changed.install(&mut other).unwrap();
    assert!(plain.install(&mut other).is_err());
    // Failed dependency checking did not leave the trait or its constant entry behind.
    changed_plain.install(&mut other).unwrap();
    assert!(changed_plain.install(&mut other).is_err());
}

#[test]
fn external_defaults_execute_offline_for_two_instantiations_and_nested_callbacks() {
    let engine = engine(false);
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime
        .load_program(&prepared(ARTIFACT), Default::default())
        .unwrap();
    for entry in [
        "direct_main",
        "parent_main",
        "dynamic_main",
        "selected_main",
        "native_main",
        "native_dynamic_main",
        "native_selected_main",
        "pure_main",
        "heap_main",
        "heap_dynamic_main",
    ] {
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
fn external_nested_defaults_clean_up_at_every_budget_cut() {
    let engine = engine(false);
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime
        .load_program(&prepared(ARTIFACT), Default::default())
        .unwrap();
    for entry in ["native_selected_main", "heap_main", "heap_dynamic_main"] {
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

#[cfg(feature = "source")]
mod source {
    use super::*;
    use kagari_common::source::SourceFile;
    use kagari_embed::error::EmbeddingError;

    #[test]
    fn source_emission_matches_the_external_default_product() {
        let artifact = engine(false)
            .compile_to_artifact(
                SourceFile::new(
                    "memory://native-default-external.kgr",
                    include_str!("fixtures/native_default_external.kgr"),
                ),
                Default::default(),
                Default::default(),
            )
            .unwrap();
        assert_eq!(artifact.to_bytes().unwrap(), ARTIFACT);
    }

    #[test]
    fn a_constant_default_cannot_load_with_a_changed_transitive_template() {
        let source = "use game::parent::Parent; use game::plain_default::Plain;
            struct Item<T> { val value: T }
            impl<T> Parent<T> for Item<T> {
                fn read(self, by: T) -> T { self.value }
                fn shift(self, by: T) -> T { by }
            }
            impl<T> Plain<T> for Item<T> { fn mark(self) -> i32 { 42 } }
            fn main() -> i32 { Item { value: 40 }.constant(2) }";
        let changed = fixture_api::changed_parent::native_api().unwrap();
        let compiler = KagariEngine::builder()
            .install_standard_library(false)
            .install(fixture_api::plain::native_api(&changed.catalog()))
            .install(Ok(changed))
            .build()
            .unwrap();
        let artifact = compiler
            .compile_to_artifact(
                SourceFile::new("memory://changed-parent-template.kgr", source),
                Default::default(),
                Default::default(),
            )
            .unwrap();
        // The called default is constant: there is no executable Parent callback/import.
        assert!(
            artifact
                .program
                .modules
                .iter()
                .flat_map(|module| &module.native_imports)
                .all(|import| import.instance.declaration.module.path != ["parent"])
        );
        let program = prepared(&artifact.to_bytes().unwrap());
        let context = ExecutionContext::default();
        let mut runtime = engine(false).runtime(context.clone());
        let error = runtime
            .load_program(&program, Default::default())
            .unwrap_err();
        assert!(
            matches!(&error, EmbeddingError::Load { error } if error.to_string().contains("native dependency differs from its registered template contract")),
            "{error:?}"
        );
        assert_eq!(runtime.runtime().gc().active_roots(), 0);
        assert_eq!(
            runtime.runtime().resources().counters().current_call_depth,
            0
        );
        // The independently checked changed provider/product pair remains executable.
        let mut compatible = engine(true).runtime(context.clone());
        let loaded = compatible
            .load_program(&program, Default::default())
            .unwrap();
        assert_eq!(
            compatible
                .execute(&loaded, "main", &[], &context)
                .unwrap()
                .return_value,
            Value::I32(42)
        );
    }
}
