//! Optional registered comparisons use real scalar and script selected targets.
// The regeneration example shares the actual application registration.
#[path = "fixtures/native_cmp_api.rs"]
pub mod fixture_api;
use kagari_abi::{
    scalar::BuiltinType,
    standard::surface::StandardEnum,
    types::{AbiType, TypeAbiKind, native::NativeTypeConstructor},
};
use kagari_bytecode::artifact::KbcArtifact;
use kagari_common::identity::{ModuleIdentity, PackageId};
use kagari_embed::{
    context::ExecutionContext,
    engine::{EngineConfig, KagariEngine},
    program::PreparedProgram,
};
use kagari_runtime::{
    native::{api::NativeApi, cmp_api::cmp, string_api::string},
    value::Value,
};
use std::collections::BTreeSet;

const ARTIFACT: &[u8] = include_bytes!("fixtures/native_cmp.kbc");
const ENTRIES: [&str; 8] = [
    "signed_main",
    "unsigned_main",
    "other_main",
    "script_main",
    "partial_main",
    "roundtrip_main",
    "incomparable_main",
    "composed_main",
];

#[test]
fn implicit_equality_product_has_no_native_imports_and_executes_without_packages() {
    let bytes = include_bytes!("fixtures/implicit_equality.kbc");
    let artifact = KbcArtifact::from_bytes(bytes).unwrap();
    assert!(
        artifact
            .program
            .modules
            .iter()
            .all(|module| module.native_imports.is_empty())
    );
    let engine = KagariEngine::builder()
        .install_standard_library(false)
        .build()
        .unwrap();
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime
        .load_program(&prepared(bytes), Default::default())
        .unwrap();
    assert_eq!(
        runtime
            .execute(&loaded, "main", &[], &context)
            .unwrap()
            .return_value,
        Value::Bool(true)
    );
    assert_eq!(runtime.runtime().gc().active_roots(), 0);
    assert_eq!(
        runtime.runtime().resources().counters().current_call_depth,
        0
    );
}

#[cfg(feature = "source")]
#[kagari_native_macros::native_module("game::text")]
pub mod text_api {
    use std::string::String;
    #[native_type]
    pub type Text = String;
}

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
fn prepared(bytes: &[u8]) -> PreparedProgram {
    PreparedProgram::from_artifact(
        KbcArtifact::from_bytes(bytes).unwrap(),
        &Default::default(),
        &Default::default(),
    )
    .unwrap()
}

#[test]
fn comparison_package_owns_complete_protocols_and_actual_scalar_implementations() {
    let api = cmp::native_api().unwrap();
    let module = &api.modules()[0];
    assert_eq!(module.package_alias.as_deref(), Some("std"));
    assert_eq!(
        module
            .traits
            .iter()
            .map(|item| item.name.as_str())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["PartialEq", "Eq", "PartialOrd", "Ord"])
    );
    assert_eq!(module.types.len(), 1);
    assert_eq!(
        module.types[0].kind,
        TypeAbiKind::Native(NativeTypeConstructor::Enum(StandardEnum::Ordering))
    );
    assert_eq!(
        module.types[0]
            .variants
            .iter()
            .map(|variant| variant.name.as_str())
            .collect::<Vec<_>>(),
        ["Less", "Equal", "Greater"]
    );
    assert_eq!(module.implementations.len(), 60);
    assert_eq!(module.native_declarations().len(), 46);
    let floats = [
        AbiType::Builtin(BuiltinType::F32),
        AbiType::Builtin(BuiltinType::F64),
    ];
    for implementation in &module.implementations {
        if floats.contains(&implementation.for_type) {
            let trait_name = &implementation
                .trait_type
                .as_ref()
                .unwrap()
                .declaration
                .path
                .last()
                .unwrap()
                .name;
            assert!(matches!(trait_name.as_str(), "PartialEq" | "PartialOrd"));
        }
    }
    let sources = KagariEngine::default().native_declaration_sources();
    assert_eq!(
        sources
            .iter()
            .find(|source| source.uri == "kagari://native/kagari-std/cmp.kgr")
            .unwrap()
            .text,
        include_str!("../../../stdlib/cmp.kgr")
    );
    assert!(
        sources
            .iter()
            .any(|source| source.uri == "kagari://native/kagari-std/cmp.kgr"
                && source.text.contains("pub trait Ord: Eq + PartialOrd"))
    );
    assert!(
        KagariEngine::builder()
            .install_standard_library(false)
            .build()
            .unwrap()
            .native_declaration_sources()
            .is_empty()
    );
}

#[test]
fn checked_comparisons_execute_offline_for_all_integer_widths_and_script_targets() {
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
            Value::Bool(true),
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
fn every_comparison_budget_cut_releases_nested_enum_values_and_frames() {
    let engine = engine();
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime
        .load_program(&prepared(ARTIFACT), Default::default())
        .unwrap();
    for entry in ENTRIES {
        let mut finished = false;
        for limit in 0..600 {
            let mut limited = context.clone();
            limited.resources.max_instruction_steps = Some(limit);
            match runtime.execute(&loaded, entry, &[], &limited) {
                Ok(report) => {
                    assert_eq!(report.return_value, Value::Bool(true));
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
fn portable_verification_rejects_a_changed_selected_scalar_comparator() {
    let mut artifact = KbcArtifact::from_bytes(ARTIFACT).unwrap();
    let compare = artifact
        .program
        .modules
        .iter_mut()
        .flat_map(|module| &mut module.native_imports)
        .find(|import| {
            import.instance.declaration.module.path == ["rank"]
                && import.instance.arguments == [AbiType::Builtin(BuiltinType::I8)]
        })
        .unwrap();
    compare.callables[0].signature.params[1] = AbiType::Builtin(BuiltinType::I16);
    artifact.portable_mir = None;
    assert!(
        PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default()).is_err()
    );
}

#[test]
fn a_declaration_catalog_cannot_replace_the_actual_comparison_provider() {
    let cmp = cmp::native_api().unwrap();
    let consumer = fixture_api::rank::native_api(&cmp.catalog()).unwrap();
    assert!(NativeApi::combine(vec![consumer]).is_err());
    let empty = KagariEngine::builder()
        .install_standard_library(false)
        .build()
        .unwrap();
    assert!(
        empty
            .runtime(Default::default())
            .load_program(&prepared(ARTIFACT), Default::default())
            .is_err()
    );
}

#[test]
fn conflicting_package_aliases_reject_composition_before_namespace_publication() {
    let api = string::native_api().unwrap();
    let mut module = api.modules()[0].as_ref().clone();
    // Relocate only the representations; trait members retain their own identity.
    module.traits.clear();
    module.implementations.clear();
    module.documentation.clear();
    module.identity = ModuleIdentity {
        package: PackageId("another".into()),
        path: vec!["text".into()],
    };
    module.package_alias = Some("std".into());
    let changed = NativeApi::new(vec![module.clone()], vec![], Default::default()).unwrap();
    assert!(NativeApi::combine(vec![cmp::native_api().unwrap(), changed]).is_err());
    module.package_alias = Some("not::a::package".into());
    assert!(NativeApi::new(vec![module], vec![], Default::default()).is_err());
}

#[cfg(feature = "source")]
mod source {
    use super::*;
    use kagari_common::{source::SourceFile, source_database::SourceLayer};
    use kagari_embed::error::EmbeddingError;
    use kagari_runtime::native::option_api::option;

    const SOURCE: &str = include_str!("fixtures/native_cmp.kgr");
    #[test]
    fn implicit_identity_and_composition_need_no_installed_protocol_declarations() {
        for engine in [
            KagariEngine::builder()
                .install_standard_library(false)
                .build()
                .unwrap(),
            engine(),
        ] {
            let artifact = engine
                .compile_to_artifact(
                    SourceFile::new(
                        "memory://implicit-equality.kgr",
                        include_str!("fixtures/implicit_equality.kgr"),
                    ),
                    Default::default(),
                    Default::default(),
                )
                .unwrap();
            assert!(
                artifact.to_bytes().unwrap() == include_bytes!("fixtures/implicit_equality.kbc")
            );
            let context = ExecutionContext::default();
            let mut runtime = engine.runtime(context.clone());
            let loaded = runtime
                .load_program(&prepared(&artifact.to_bytes().unwrap()), Default::default())
                .unwrap();
            assert_eq!(
                runtime
                    .execute(&loaded, "main", &[], &context)
                    .unwrap()
                    .return_value,
                Value::Bool(true)
            );
        }
    }
    #[test]
    fn manual_optional_packages_emit_the_exact_comparison_product() {
        let artifact = engine()
            .compile_to_artifact(
                SourceFile::new("memory://native-cmp.kgr", SOURCE),
                Default::default(),
                Default::default(),
            )
            .unwrap();
        assert!(artifact.to_bytes().unwrap() == ARTIFACT);
    }
    #[test]
    fn comparison_protocol_and_variant_navigation_use_registered_coordinates() {
        let engine = engine();
        let text = "use std::cmp::{Ord, Ordering}; fn main() -> Ordering { Ordering::Less }";
        let file = engine
            .set_source(
                "memory://comparison-tooling.kgr",
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
        for (offset, name) in [
            (text.find("Ord,").unwrap(), "Ord"),
            (text.rfind("Ordering::").unwrap(), "Ordering"),
            (text.rfind("Less").unwrap(), "Less"),
        ] {
            let definition = snapshot.definition_at(file, offset).unwrap();
            let source = snapshot.source(definition.location.file).unwrap();
            assert_eq!(source.name(), "kagari://native/kagari-std/cmp.kgr");
            assert_eq!(
                &source.text()[definition.location.range.start..definition.location.range.end],
                name
            );
        }
        assert!(
            snapshot
                .documentation_at(file, text.rfind("Ordering::").unwrap())
                .unwrap()
                .documentation
                .contains("total ordering comparison")
        );
    }

    #[test]
    fn application_alias_and_representation_name_resolve_from_actual_records() {
        let text = text_api::native_api().unwrap();
        let mut declaration = text.modules()[0].as_ref().clone();
        declaration.package_alias = Some("app".into());
        let text = NativeApi::new(vec![declaration], vec![], Default::default()).unwrap();
        let cmp = cmp::native_api().unwrap();
        let rank = fixture_api::rank::native_api(&cmp.catalog()).unwrap();
        let api = NativeApi::combine(vec![cmp, rank, text, option::native_api().unwrap()]).unwrap();
        let engine = KagariEngine::builder()
            .install_standard_library(false)
            .install(Ok(api))
            .build()
            .unwrap();
        let artifact = engine.compile_to_artifact(SourceFile::new("memory://application-text.kgr", "use app::text::Text; use game::rank::compare; use std::cmp::Ordering; fn main() -> bool { val left: Text = \"a\"; match compare(left, \"é\") { Ordering::Less => true, _ => false } }"), Default::default(), Default::default()).unwrap();
        let context = ExecutionContext::default();
        let mut runtime = engine.runtime(context.clone());
        let loaded = runtime
            .load_program(&prepared(&artifact.to_bytes().unwrap()), Default::default())
            .unwrap();
        assert_eq!(
            runtime
                .execute(&loaded, "main", &[], &context)
                .unwrap()
                .return_value,
            Value::Bool(true)
        );
        assert_eq!(runtime.runtime().gc().active_roots(), 0);
    }
    #[test]
    fn floats_do_not_gain_total_order_and_self_protocols_remain_static() {
        for text in [
            "fn invalid() -> Ordering { compare(1.0f32, 2.0f32) }",
            "fn invalid() -> Ordering { compare(1.0f64, 2.0f64) }",
            "fn invalid() { val value: Ord = Rank { key: 1 }; }",
        ] {
            let error = engine()
                .compile_to_artifact(
                    SourceFile::new(
                        "memory://invalid-comparison.kgr",
                        format!("{SOURCE}\n{text}"),
                    ),
                    Default::default(),
                    Default::default(),
                )
                .unwrap_err();
            assert!(matches!(error, EmbeddingError::Diagnostics { diagnostics }
                if diagnostics.iter().any(|diagnostic| matches!(diagnostic.code.as_str(), "KG_TYPE_GENERIC_BOUND_NOT_SATISFIED" | "KG_TYPE_INVALID_INTERFACE_TYPE"))));
        }
    }
    #[test]
    fn selected_script_comparisons_pin_generations_across_reload() {
        let engine = engine();
        let artifact = engine
            .compile_to_artifact(
                SourceFile::new(
                    "memory://native-cmp.kgr",
                    SOURCE.replace("self.key < other.key", "self.key > other.key"),
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
        for (loaded, expected) in [(&old, true), (&current, false)] {
            assert_eq!(
                runtime
                    .execute(loaded, "script_main", &[], &context)
                    .unwrap()
                    .return_value,
                Value::Bool(expected)
            );
            assert_eq!(runtime.runtime().gc().active_roots(), 0);
        }
    }

    #[test]
    fn selected_comparison_traps_release_receiver_values_and_frames() {
        let engine = engine();
        let artifact = engine
            .compile_to_artifact(
                SourceFile::new(
                    "memory://comparison-trap.kgr",
                    SOURCE.replace(
                        "if self.key < other.key",
                        "if self.key / (other.key - other.key) < other.key",
                    ),
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
                .any(|frame| frame.function_name == "cmp")
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
