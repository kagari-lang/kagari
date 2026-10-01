//! Typed selected dependencies share checked applications and rooted callbacks.
// Test/cross-target sharing keeps the fixture registration authoritative.
#[path = "fixtures/native_selected_api.rs"]
mod fixture_api;
use fixture_api::selected;
use kagari_bytecode::artifact::KbcArtifact;
use kagari_embed::{
    context::ExecutionContext,
    engine::{EngineConfig, KagariEngine},
    program::PreparedProgram,
};
use kagari_native_macros::native_module;
use kagari_runtime::value::Value;

const ARTIFACT: &[u8] = include_bytes!("fixtures/native_selected.kbc");

fn engine(defaults: bool) -> KagariEngine {
    let mut config = EngineConfig::default();
    config.default_runtime.gc.collection_threshold = Some(1);
    KagariEngine::builder()
        .config(config)
        .install_standard_library(defaults)
        .install(selected::native_api())
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
            assert_eq!(
                runtime
                    .execute(program, "script_main", &[], &context)
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
