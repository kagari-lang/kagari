//! Typed default templates share portable contracts, private ownership and callbacks.
#[path = "fixtures/native_default_typed_api.rs"]
mod fixture_api;
use kagari_abi::{
    callable::CallableImplementation,
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
use kagari_runtime::value::Value;

const ARTIFACT: &[u8] = include_bytes!("fixtures/native_default_typed.kbc");

fn engine() -> KagariEngine {
    let mut config = EngineConfig::default();
    config.default_runtime.gc.collection_threshold = Some(1);
    KagariEngine::builder()
        .config(config)
        .install_standard_library(false)
        .install(fixture_api::defaults::native_api())
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
fn signatures_and_binder_roles_come_from_actual_rust_templates() {
    let api = fixture_api::defaults::native_api().unwrap();
    let module = &api.modules()[0];
    let contract = &module.traits[0];
    let echo = contract
        .methods
        .iter()
        .find(|method| method.name == "echo")
        .unwrap();
    let CallableImplementation::NativeDefault(application) = &echo.implementation else {
        panic!("default application");
    };
    assert!(!echo.method_policy.override_allowed);
    assert!(matches!(
        application.arguments[0],
        AbiType::Projection { .. }
    ));
    assert_eq!(application.arguments[1], echo.params[0].ty);
    assert_eq!(
        application.arguments[2],
        contract.generic_params[0].as_type()
    );
    assert_eq!(echo.params.len(), 2);
    assert_eq!(echo.params[0].name, "self");
    assert_eq!(echo.return_type, application.arguments[0]);
    assert_eq!(module.implementations[0].methods.len(), 1);
    assert_eq!(module.implementations[0].methods[0].name, "read");
    let source = &api.declaration_sources()[0].text;
    assert!(source.contains("fn echo(self, by: T0)"), "{source}");
    assert!(source.contains("fn echo_template<"), "{source}");
    assert!(!source.contains("pub fn echo_template"));
    assert!(!source.contains("NativeSelected"));
    assert!(module.private_functions.contains(&application.declaration));
    assert_eq!(module.private_functions.len(), 2);
}

#[test]
fn typed_defaults_execute_offline_for_script_native_generic_and_heap_receivers() {
    let engine = engine();
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime
        .load_program(&prepared(ARTIFACT), Default::default())
        .unwrap();
    for entry in [
        "direct_main",
        "generic_main",
        "dynamic_main",
        "selected_main",
        "native_main",
        "native_dynamic_main",
        "native_selected_main",
        "heap_main",
        "heap_dynamic_main",
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
    }
    for module in &KbcArtifact::from_bytes(ARTIFACT).unwrap().program.modules {
        for item in &module.public_items {
            if let PublicAbiItem::Function(function) = item {
                assert!(!matches!(
                    function.name.as_str(),
                    "echo_template" | "alternate_template"
                ));
            }
        }
    }
}

#[test]
fn nested_default_templates_release_roots_frames_and_heap_at_every_budget_cut() {
    let engine = engine();
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime
        .load_program(&prepared(ARTIFACT), Default::default())
        .unwrap();
    for entry in [
        "selected_main",
        "native_selected_main",
        "heap_main",
        "heap_dynamic_main",
    ] {
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

#[native_module("game::unmapped_default")]
pub mod unmapped {
    use kagari_runtime::native_value::{NativeResult, NativeValue};
    #[native_trait]
    pub trait Source {
        fn read(&self) -> NativeResult<i32>;
    }
    #[native_default(T: Source::extra)]
    fn template<T: NativeValue, U: NativeValue>(value: T, extra: U) -> U {
        let _ = value;
        extra
    }
}

#[native_module("game::ambiguous_default")]
pub mod ambiguous {
    use kagari_runtime::native_value::{NativeResult, NativeValue};
    #[native_trait]
    pub trait Source {
        type Output: NativeValue;
        fn read(&self) -> NativeResult<Self::Output>;
    }
    #[native_default(T: Source<Output = T>::extra)]
    fn template<T: NativeValue>(value: T) -> T {
        value
    }
}

#[native_module("game::wrong_default_receiver")]
pub mod wrong_receiver {
    use kagari_runtime::native_value::{NativeResult, NativeValue};
    #[native_trait]
    pub trait Source {
        fn read(&self) -> NativeResult<i32>;
    }
    #[native_default(T: Source::extra)]
    fn template<T: NativeValue>(number: i32, value: T) -> i32 {
        let _ = value;
        number
    }
}

#[native_module("game::unproved_default")]
pub mod unproved {
    use kagari_runtime::native_value::{NativeResult, NativeValue, selected::NativeSelected};
    #[native_trait]
    pub trait Source {
        fn read(&self) -> NativeResult<i32>;
    }
    #[native_trait]
    pub trait Other {
        fn read(&self) -> NativeResult<i32>;
    }
    #[native_default(T: Source::extra)]
    fn template<T: NativeValue>(
        value: T,
        #[selected(T: Other::read)] other: NativeSelected<(T,), i32>,
    ) -> i32 {
        let _ = (value, other);
        0
    }
}

#[test]
fn invalid_or_unproved_default_mappings_reject_before_publication() {
    for api in [
        unmapped::native_api(),
        ambiguous::native_api(),
        wrong_receiver::native_api(),
        unproved::native_api(),
    ] {
        assert!(api.is_err());
    }
    let api = fixture_api::defaults::native_api().unwrap();
    let mut module = api.modules()[0].as_ref().clone();
    module
        .private_functions
        .insert(module.definition(DefinitionKind::Function, "absent"));
    assert!(module.validate().is_err());
}

#[cfg(feature = "source")]
mod source {
    use super::*;
    use kagari_common::{diagnostic::Severity, source::SourceFile, source_database::SourceLayer};
    use kagari_embed::error::EmbeddingError;
    const SOURCE: &str = include_str!("fixtures/native_default_typed.kgr");

    #[test]
    fn source_emission_matches_the_typed_default_product() {
        let artifact = engine()
            .compile_to_artifact(
                SourceFile::new("memory://native-default-typed.kgr", SOURCE),
                Default::default(),
                Default::default(),
            )
            .unwrap();
        assert_eq!(artifact.to_bytes().unwrap(), ARTIFACT);
    }

    #[test]
    fn generated_default_members_support_navigation_documentation_and_completion() {
        let engine = engine();
        let text = format!(
            "{SOURCE}\nfn tooling() {{ val value = Item {{ value: 40 }}; value.echo(2); value. }}"
        );
        let file = engine
            .set_source(
                "memory://native-default-tooling.kgr",
                text.clone(),
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
        let offset = text.rfind("echo(2)").unwrap();
        let declaration = snapshot.definition_at(file, offset).unwrap();
        let source = snapshot.source(declaration.location.file).unwrap();
        assert!(source.name().starts_with("kagari://native/"));
        assert_eq!(
            &source.text()[declaration.location.range.start..declaration.location.range.end],
            "echo"
        );
        let docs = snapshot.documentation_at(file, offset).unwrap();
        assert!(
            docs.documentation.contains("exact associated output"),
            "{docs:?}"
        );
        let completions = snapshot
            .file(file)
            .unwrap()
            .method_completions(text.rfind("value.").unwrap() + "value.".len());
        assert!(completions.iter().any(|method| method.name == "echo"));
        assert!(completions.iter().any(|method| method.name == "alternate"));
        assert!(
            completions
                .iter()
                .all(|method| !method.name.ends_with("_template"))
        );
    }

    #[test]
    fn final_overrides_and_private_helper_imports_reject() {
        for (text, expected_code) in [
            (
                SOURCE.replacen(
                    "fn read(self, by: i32)",
                    "fn echo(self, by: i32) -> i32 { 99 } fn read(self, by: i32)",
                    1,
                ),
                "KG_TYPE_TRAIT_METHOD_MISMATCH",
            ),
            (
                "use game::typed_defaults::echo_template; fn main() {}".into(),
                "KG_RESOLVE_IMPORT_NOT_PUBLIC",
            ),
        ] {
            let Err(EmbeddingError::Diagnostics { diagnostics }) = engine().compile_to_artifact(
                SourceFile::new("memory://native-default-invalid.kgr", text),
                Default::default(),
                Default::default(),
            ) else {
                panic!("invalid default/import must reject");
            };
            assert!(
                diagnostics
                    .iter()
                    .any(|diagnostic| diagnostic.severity == Severity::Error
                        && diagnostic.code == expected_code
                        && (expected_code != "KG_TYPE_TRAIT_METHOD_MISMATCH"
                            || diagnostic.message.contains("forbids overriding"))),
                "{diagnostics:?}"
            );
        }
    }

    #[test]
    fn overridable_default_dispatches_to_the_explicit_script_body() {
        let source = SOURCE.replace(
            "fn read(self, by: i32) -> i32 { self.value + by }",
            "fn read(self, by: i32) -> i32 { self.value + by } fn alternate(self, by: i32) -> i32 { 99 }",
        );
        let engine = engine();
        let artifact = engine
            .compile_to_artifact(
                SourceFile::new("memory://native-default-override.kgr", source),
                Default::default(),
                Default::default(),
            )
            .unwrap();
        let context = ExecutionContext::default();
        let mut runtime = engine.runtime(context.clone());
        let loaded = runtime
            .load_program(&prepared(&artifact.to_bytes().unwrap()), Default::default())
            .unwrap();
        for entry in ["dynamic_main", "selected_main"] {
            assert_eq!(
                runtime
                    .execute(&loaded, entry, &[], &context)
                    .unwrap()
                    .return_value,
                Value::I32(99)
            );
        }
    }
}
