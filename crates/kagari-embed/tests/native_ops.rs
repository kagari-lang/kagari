//! Native range and enum aliases use ordinary authoring and checked execution.
#[path = "fixtures/native_ops_api.rs"]
mod fixture_api;

use fixture_api::shapes;
use kagari_abi::{
    scalar::BuiltinType,
    types::{AbiType, TypeAbiKind, native::NativeTypeConstructor},
};
use kagari_bytecode::artifact::KbcArtifact;
use kagari_embed::{
    context::ExecutionContext,
    engine::{EngineConfig, KagariEngine},
    program::PreparedProgram,
};
use kagari_runtime::{
    native::{ops_api::ops, packages::standard_library},
    value::Value,
};
use std::collections::BTreeSet;

const ARTIFACT: &[u8] = include_bytes!("fixtures/native_ops.kbc");

fn engine(defaults: bool) -> KagariEngine {
    let mut config = EngineConfig::default();
    config.default_runtime.gc.collection_threshold = Some(1);
    KagariEngine::builder()
        .config(config)
        .install_standard_library(defaults)
        .install(shapes::native_api())
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
fn optional_ops_provider_preserves_the_complete_public_contract_surface() {
    let api = ops::native_api().unwrap();
    let module = &api.modules()[0];
    assert_eq!(
        module
            .traits
            .iter()
            .map(|item| item.name.as_str())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([
            "Add",
            "Sub",
            "Mul",
            "Div",
            "Rem",
            "Neg",
            "Not",
            "Index",
            "BitAnd",
            "BitOr",
            "BitXor",
            "Shl",
            "Shr",
            "RangeBounds",
            "Fn",
        ])
    );
    assert_eq!(
        module
            .types
            .iter()
            .map(|item| item.name.as_str())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([
            "Range",
            "RangeInclusive",
            "RangeFrom",
            "RangeTo",
            "RangeToInclusive",
            "RangeFull",
            "Bound",
        ])
    );
    let bundled = standard_library();
    assert_eq!(
        bundled
            .modules()
            .iter()
            .find(|item| item.identity == module.identity)
            .unwrap()
            .traits,
        module.traits
    );
    let default_engine = KagariEngine::default();
    assert!(
        default_engine
            .native_declaration_sources()
            .iter()
            .any(|source| {
                source.uri == "kagari://native/kagari-std/ops.kgr"
                    && source.text.contains("pub trait Index<T0>")
                    && source.text.contains("pub enum Bound<T0>")
            })
    );
    let empty_engine = KagariEngine::builder()
        .install_standard_library(false)
        .build()
        .unwrap();
    assert!(empty_engine.native_declaration_sources().is_empty());
}

#[test]
fn range_aliases_and_generic_bound_payloads_execute_without_the_default_library() {
    for defaults in [false, true] {
        let context = ExecutionContext::default();
        let mut runtime = engine(defaults).runtime(context.clone());
        let loaded = runtime
            .load_program(&prepared(), Default::default())
            .unwrap();
        for (entry, expected) in [
            ("exclusive_main", Value::I32(42)),
            ("inclusive_main", Value::I32(42)),
            ("tail_main", Value::I32(42)),
            ("head_main", Value::I32(42)),
            ("closed_head_main", Value::I32(42)),
            ("full_main", Value::Bool(true)),
            ("heap_main", Value::I32(42)),
            ("unbounded_main", Value::I32(0)),
            ("width_main", Value::U64(u64::MAX)),
        ] {
            for _ in 0..3 {
                assert_eq!(
                    runtime
                        .execute(&loaded, entry, &[], &context)
                        .unwrap()
                        .return_value,
                    expected,
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
    }
}

#[test]
fn checked_bound_payloads_release_roots_at_every_budget_cut() {
    let context = ExecutionContext::default();
    let mut runtime = engine(false).runtime(context.clone());
    let loaded = runtime
        .load_program(&prepared(), Default::default())
        .unwrap();
    let mut finished = false;
    for limit in 0..100 {
        let mut limited = context.clone();
        limited.resources.max_instruction_steps = Some(limit);
        match runtime.execute(&loaded, "heap_main", &[], &limited) {
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
    assert!(finished);
}

#[test]
fn malformed_enum_payloads_and_representation_arity_reject_before_installation() {
    let api = shapes::native_api().unwrap();
    let original = &api.modules()[0];
    for case in 0..3 {
        let mut module = original.as_ref().clone();
        let ty = module
            .types
            .iter_mut()
            .find(|ty| matches!(ty.kind, TypeAbiKind::Native(NativeTypeConstructor::Enum(_))))
            .unwrap();
        match case {
            0 => ty.variants[0].payload[0] = AbiType::Builtin(BuiltinType::Bool),
            1 => {
                ty.variants.pop();
            }
            2 => ty.generic_params.clear(),
            _ => unreachable!(),
        }
        assert!(module.validate().is_err(), "{case}");
    }
}

#[cfg(feature = "source")]
mod source {
    use super::*;
    use kagari_common::{source::SourceFile, source_database::SourceLayer};

    #[test]
    fn registered_range_aliases_emit_the_exact_source_independent_product() {
        let artifact = engine(false)
            .compile_to_artifact(
                SourceFile::new(
                    "memory://native-ops.kgr",
                    include_str!("fixtures/native_ops.kgr"),
                ),
                Default::default(),
                Default::default(),
            )
            .unwrap();
        assert_eq!(artifact.to_bytes().unwrap(), ARTIFACT);
    }

    #[test]
    fn enum_aliases_preserve_registered_variant_navigation_and_documentation() {
        let engine = engine(false);
        let text = "use game::shapes::{Edge, identity}; fn main() -> i32 { match identity(Edge::Included(42)) { Edge::Included(value) => value, Edge::Excluded(value) => value, Edge::Unbounded => 0 } }";
        let file = engine
            .set_source(
                "memory://native-ops-tooling.kgr",
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
        for (offset, expected) in [
            (text.find("Edge::").unwrap(), "Edge"),
            (text.find("Included(").unwrap(), "Included"),
        ] {
            let definition = snapshot.definition_at(file, offset).unwrap();
            let source = snapshot.source(definition.location.file).unwrap();
            assert_eq!(source.name(), "kagari://native/game/shapes.kgr");
            assert_eq!(
                &source.text()[definition.location.range.start..definition.location.range.end],
                expected
            );
        }
        let doc = snapshot
            .documentation_at(file, text.find("Edge::").unwrap())
            .unwrap();
        assert!(doc.documentation.contains("Checked bound payloads"));
    }

    #[test]
    fn mismatched_range_and_bound_element_types_remain_static_errors() {
        for source in [
            "use game::shapes::{Edge, identity}; fn main() -> Edge<bool> { identity(Edge::Included(42)) }",
            "use game::shapes::span_identity; fn main() { span_identity(1..true) }",
        ] {
            assert!(
                engine(false)
                    .compile_to_artifact(
                        SourceFile::new("memory://native-ops-invalid.kgr", source),
                        Default::default(),
                        Default::default()
                    )
                    .is_err()
            );
        }
    }
}
