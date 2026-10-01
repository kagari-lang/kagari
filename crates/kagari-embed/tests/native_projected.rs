//! Projected native dependencies retain real associated contracts and callbacks.
// The example and test share the actual Rust provider registration.
#[path = "fixtures/native_projected_api.rs"]
pub mod fixture_api;

use kagari_abi::types::{AbiType, ConstraintAbi};
use kagari_bytecode::artifact::KbcArtifact;
use kagari_common::span::Span;
use kagari_embed::{
    context::ExecutionContext,
    engine::{EngineConfig, KagariEngine},
    program::PreparedProgram,
};
use kagari_native_macros::native_module;
use kagari_runtime::{native::catalog::NativeCatalog, value::Value};

const ARTIFACT: &[u8] = include_bytes!("fixtures/native_projected.kbc");
const ENTRIES: [&str; 5] = [
    "native_main",
    "script_main",
    "mixed_main",
    "generic_main",
    "inspect_main",
];

fn engine() -> KagariEngine {
    let mut config = EngineConfig::default();
    config.default_runtime.gc.collection_threshold = Some(1);
    KagariEngine::builder()
        .config(config)
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
fn projected_templates_keep_the_registered_contract_and_implied_base_bound() {
    let api = fixture_api::api().unwrap();
    let artifact = KbcArtifact::from_bytes(ARTIFACT).unwrap();
    let module = api
        .modules()
        .iter()
        .find(|module| module.identity.path == ["projected"])
        .unwrap();
    let carried = artifact
        .program
        .modules
        .iter()
        .find(|carried| carried.identity == module.identity)
        .unwrap();
    assert_eq!(carried.native_declarations, module.native_declarations());
    let inspected = module
        .native_declarations()
        .into_iter()
        .find(|declaration| declaration.function.name == "inspect_output")
        .unwrap();
    assert_eq!(inspected.function.params.len(), 1);
    assert_eq!(inspected.callable_requirements.len(), 1);
    let AbiType::Projection {
        receiver,
        interface,
        member,
        ..
    } = &inspected.callable_requirements[0].receiver
    else {
        panic!("registered projection must not be erased");
    };
    assert_eq!(member.path.last().unwrap().name, "Output");
    assert!(inspected.function.bounds.iter().any(|bound| {
        &bound.ty == receiver.as_ref()
            && bound
                .constraints
                .contains(&ConstraintAbi::Trait(interface.as_ref().clone()))
    }));
    let source = module.declaration_source().unwrap();
    let site = &source.sites[&inspected.declaration];
    let text = |span: Span| &source.text[span.start..span.end];
    assert_eq!(text(site.name_span), "inspect_output");
    assert_eq!(site.bounds.len(), 2);
    assert_eq!(text(site.bounds[1].target), "T0");
    assert_eq!(
        text(site.bounds[1].constraints[0]),
        "game::selected::Echo<Output = T1>"
    );
    assert_eq!(
        text(site.bounds[0].target),
        "<T0 as game::selected::Echo<Output = T1>>::Output"
    );
    assert_eq!(
        text(site.bounds[0].constraints[0]),
        "game::selected::Echo<Output = i32>"
    );
    assert!(!source.text.contains("NativeSelected"));
}

#[test]
fn native_script_and_mixed_projection_chains_execute_offline_with_frequent_gc() {
    let engine = engine();
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime
        .load_program(&prepared(ARTIFACT), Default::default())
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
fn every_budget_cut_cleans_up_projected_arguments_and_intermediate_values() {
    let engine = engine();
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime
        .load_program(&prepared(ARTIFACT), Default::default())
        .unwrap();
    for entry in ENTRIES {
        let mut finished = false;
        for limit in 0..200 {
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
fn portable_verification_rejects_erased_projected_obligations_and_changed_targets() {
    for case in 0..2 {
        let mut artifact = KbcArtifact::from_bytes(ARTIFACT).unwrap();
        if case == 0 {
            let declaration = artifact
                .program
                .modules
                .iter_mut()
                .flat_map(|module| &mut module.native_declarations)
                .find(|declaration| declaration.function.name == "inspect_output")
                .unwrap();
            declaration
                .function
                .bounds
                .retain(|bound| !matches!(bound.ty, AbiType::Projection { .. }));
        } else {
            let target = artifact
                .program
                .modules
                .iter_mut()
                .flat_map(|module| &mut module.native_imports)
                .find(|import| {
                    import.instance.declaration.path.last().unwrap().name == "echo_twice"
                })
                .unwrap();
            assert_eq!(target.callables.len(), 2);
            target.callables[1].instance = target.callables[0].instance.clone();
        }
        artifact.portable_mir = None;
        assert!(
            PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default())
                .is_err(),
            "{case}"
        );
    }
}

#[native_module("game::invalid_projection", catalog)]
pub mod invalid_projection {
    use kagari_runtime::native_value::{NativeValue, selected::NativeSelected};
    #[native]
    pub fn inspect<T: NativeValue, U: NativeValue>(
        value: U,
        #[selected(<T as game::selected::Echo<Output = U>>::Missing: game::selected::Echo<Output = i32>::echo)]
        selected: NativeSelected<(U,), i32>,
    ) {
        drop((value, selected));
    }
}

#[native_module("game::wrong_projection_signature", catalog)]
pub mod wrong_projection_signature {
    use kagari_runtime::native_value::{NativeValue, selected::NativeSelected};
    #[native]
    pub fn inspect<T: NativeValue, U: NativeValue>(
        value: U,
        #[selected(<T as game::selected::Echo<Output = U>>::Output: game::selected::Echo<Output = i32>::echo)]
        selected: NativeSelected<(U,), bool>,
    ) {
        drop((value, selected));
    }
}

#[test]
fn registration_requires_actual_projected_members_and_the_typed_callback_signature() {
    let base = fixture_api::base::api().unwrap();
    assert!(fixture_api::projected::native_api(&NativeCatalog::default()).is_err());
    assert!(invalid_projection::native_api(&base.catalog()).is_err());
    assert!(wrong_projection_signature::native_api(&base.catalog()).is_err());
}

#[cfg(feature = "source")]
mod source {
    use super::*;
    use kagari_common::source::SourceFile;
    use kagari_embed::error::EmbeddingError;

    const SOURCE: &str = include_str!("fixtures/native_projected.kgr");

    #[test]
    fn source_emission_matches_the_projection_product() {
        let artifact = engine()
            .compile_to_artifact(
                SourceFile::new("memory://native-projected.kgr", SOURCE),
                Default::default(),
                Default::default(),
            )
            .unwrap();
        assert_eq!(artifact.to_bytes().unwrap(), ARTIFACT);
    }

    #[test]
    fn inference_cannot_supply_missing_bounds_or_incompatible_associated_equalities() {
        for text in [
            "fn invalid<T: Echo<Output = U>, U>(value: T) -> i32 { echo_twice(value) }",
            "fn invalid<T, U: Echo<Output = i32>>(value: T) -> i32 { echo_twice(value) }",
            "fn invalid<T: Echo<Output = bool>>(value: T) -> i32 { echo_twice::<T, ArrayList<i32>>(value) }",
            "fn invalid() -> i32 { inspect_output::<bool, ArrayList<i32>>([42]) }",
            "fn invalid() -> i32 { inspect_output::<String, ArrayList<i32>>([42]) }",
        ] {
            let error = engine()
                .compile_to_artifact(
                    SourceFile::new(
                        "memory://projection-negative.kgr",
                        format!("{SOURCE}\n{text}"),
                    ),
                    Default::default(),
                    Default::default(),
                )
                .unwrap_err();
            assert!(
                matches!(&error, EmbeddingError::Diagnostics { diagnostics }
                if diagnostics.iter().any(|diagnostic| diagnostic.code == "KG_TYPE_GENERIC_BOUND_NOT_SATISFIED"
                    || diagnostic.code == "KG_TYPE_CANNOT_INFER_GENERIC_ARGUMENT")),
                "{text}: {error:?}"
            );
        }
    }

    #[test]
    fn projected_script_callbacks_keep_their_generation_after_reload() {
        let engine = engine();
        let artifact = engine
            .compile_to_artifact(
                SourceFile::new(
                    "memory://native-projected.kgr",
                    SOURCE.replace(
                        "fn echo(self) -> i32 { self.value }",
                        "fn echo(self) -> i32 { self.value + 1 }",
                    ),
                ),
                Default::default(),
                Default::default(),
            )
            .unwrap();
        let context = ExecutionContext::default();
        let mut runtime = engine.runtime(context.clone());
        let old = runtime
            .load_program(&prepared(ARTIFACT), Default::default())
            .unwrap();
        let current = runtime
            .reload_program(
                &old,
                &prepared(&artifact.to_bytes().unwrap()),
                Default::default(),
            )
            .unwrap();
        for (loaded, expected) in [(&old, 42), (&current, 43)] {
            for entry in ["script_main", "generic_main", "inspect_main"] {
                assert_eq!(
                    runtime
                        .execute(loaded, entry, &[], &context)
                        .unwrap()
                        .return_value,
                    Value::I32(expected)
                );
                assert_eq!(runtime.runtime().gc().active_roots(), 0);
            }
        }
    }

    #[test]
    fn traps_in_either_projected_callback_release_roots_and_frames() {
        for (before, after) in [
            (
                "fn echo(self) -> Inner { self.value }",
                "fn echo(self) -> Inner { val values: ArrayList<Inner> = []; values[0usize] }",
            ),
            (
                "fn echo(self) -> i32 { self.value }",
                "fn echo(self) -> i32 { val values: ArrayList<i32> = []; values[0usize] }",
            ),
        ] {
            let engine = engine();
            let artifact = engine
                .compile_to_artifact(
                    SourceFile::new(
                        "memory://projection-trap.kgr",
                        SOURCE.replace(before, after),
                    ),
                    Default::default(),
                    Default::default(),
                )
                .unwrap();
            let context = ExecutionContext::default();
            let mut runtime = engine.runtime(context.clone());
            let loaded = runtime
                .load_program(&prepared(&artifact.to_bytes().unwrap()), Default::default())
                .unwrap();
            let error = runtime
                .execute(&loaded, "script_main", &[], &context)
                .unwrap_err();
            assert!(
                error
                    .error_trace()
                    .unwrap()
                    .frames
                    .iter()
                    .any(|frame| frame.function_name == "echo")
            );
            assert_eq!(runtime.runtime().gc().active_roots(), 0);
            assert_eq!(
                runtime.runtime().resources().counters().current_call_depth,
                0
            );
            assert!(!runtime.runtime().is_quarantined());
            runtime.runtime().collect_garbage().unwrap();
            assert_eq!(runtime.runtime().gc().allocated_objects(), 0);
        }
    }
}
