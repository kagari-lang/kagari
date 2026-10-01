//! Native bounds remain checked contracts through source and offline execution.
// Share the registration with the source-only fixture generator; no production facade.
#[path = "fixtures/native_bounds_api.rs"]
mod fixture_api;

use kagari_abi::{
    native_import::binding_id,
    scalar::BuiltinType,
    standard::surface::StandardTypeConstraint,
    types::{AbiType, ConstraintAbi},
};
use kagari_bytecode::artifact::KbcArtifact;
use kagari_embed::{
    context::ExecutionContext,
    engine::{EngineConfig, KagariEngine},
    program::PreparedProgram,
};
use kagari_runtime::value::Value;
use std::{cell::Cell, rc::Rc};

#[cfg(feature = "source")]
const SOURCE: &str = include_str!("fixtures/native_bounds.kgr");
const ARTIFACT: &[u8] = include_bytes!("fixtures/native_bounds.kbc");

fn engine(calls: Rc<Cell<usize>>) -> KagariEngine {
    let mut config = EngineConfig::default();
    config.default_runtime.gc.collection_threshold = Some(1);
    KagariEngine::builder()
        .config(config)
        .install(Ok(fixture_api::api(fixture_api::module(), calls)))
        .build()
        .unwrap()
}

fn prepared(artifact: KbcArtifact) -> PreparedProgram {
    PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default()).unwrap()
}

#[test]
fn named_bounds_render_their_applied_arguments_and_exact_coordinates() {
    let module = fixture_api::module();
    module.validate().unwrap();
    let source = module.declaration_source().unwrap();
    assert!(source.text.contains(
        "fn echo<T0, T1>(value: T0, witness: T1) -> T0 where T0: Container<T1> + Marker;"
    ));
    let site = &source.sites[&module.native_declarations()[0].declaration];
    let bound = &site.bounds[0];
    let text = |span: kagari_common::span::Span| &source.text[span.start..span.end];
    assert_eq!(text(bound.target), "T0");
    assert_eq!(text(bound.constraints[0]), "Container<T1>");
    assert_eq!(text(bound.constraints[1]), "Marker");
}

#[test]
fn invalid_bound_templates_are_rejected_at_registration() {
    for mutation in 0..5 {
        let mut module = fixture_api::module();
        let owner = module.functions[0].generic_params[0].owner.clone();
        let bound = &mut module.functions[0].bounds[0];
        match mutation {
            0 => bound.constraints.clear(),
            1 => bound.constraints.push(bound.constraints[0].clone()),
            2 => {
                bound.ty = AbiType::Parameter {
                    owner: binding_id(&module.identity, "foreign"),
                    position: 0,
                }
            }
            3 => {
                bound.constraints =
                    vec![ConstraintAbi::Standard(StandardTypeConstraint::Comparable)]
            }
            4 => {
                if let ConstraintAbi::Trait(trait_type) = &mut bound.constraints[0] {
                    trait_type.arguments = vec![AbiType::Parameter {
                        owner,
                        position: 99,
                    }];
                }
            }
            _ => unreachable!(),
        }
        assert!(module.validate().is_err(), "invalid bound case {mutation}");
    }
}

#[test]
fn encoded_native_bounds_execute_with_application_owned_implementations_and_gc() {
    let calls = Rc::new(Cell::new(0));
    let engine = engine(calls.clone());
    let program = prepared(KbcArtifact::from_bytes(ARTIFACT).unwrap());
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime.load_program(&program, Default::default()).unwrap();
    assert_eq!(calls.get(), 0);
    assert_eq!(
        runtime
            .execute(&loaded, "main", &[], &context)
            .unwrap()
            .return_value,
        Value::I32(42)
    );
    assert_eq!(calls.get(), 1);
    assert_eq!(runtime.runtime().gc().active_roots(), 0);
    runtime.runtime().collect_garbage().unwrap();
    assert_eq!(runtime.runtime().gc().allocated_objects(), 0);
}

#[test]
fn offline_bound_execution_does_not_require_default_native_installation() {
    let calls = Rc::new(Cell::new(0));
    let engine = KagariEngine::builder()
        .install_standard_library(false)
        .install(Ok(fixture_api::api(fixture_api::module(), calls.clone())))
        .build()
        .unwrap();
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime
        .load_program(
            &prepared(KbcArtifact::from_bytes(ARTIFACT).unwrap()),
            Default::default(),
        )
        .unwrap();
    assert_eq!(
        runtime
            .execute(&loaded, "main", &[], &context)
            .unwrap()
            .return_value,
        Value::I32(42)
    );
    assert_eq!(calls.get(), 1);
    assert_eq!(runtime.runtime().gc().active_roots(), 0);
}

#[test]
fn a_verified_product_cannot_link_to_a_weaker_or_different_installed_template() {
    for variant in 0..2 {
        let calls = Rc::new(Cell::new(0));
        let mut module = fixture_api::module();
        if variant == 0 {
            module.functions[0].bounds.clear();
        } else {
            module.functions[0].bounds[0].constraints.remove(0);
        }
        let engine = KagariEngine::builder()
            .install(Ok(fixture_api::api(module, calls.clone())))
            .build()
            .unwrap();
        let mut runtime = engine.runtime(Default::default());
        assert!(
            runtime
                .load_program(
                    &prepared(KbcArtifact::from_bytes(ARTIFACT).unwrap()),
                    Default::default()
                )
                .is_err()
        );
        assert_eq!(calls.get(), 0);
        assert_eq!(runtime.runtime().gc().active_roots(), 0);
    }
}

#[test]
fn missing_or_forged_applied_bound_requirements_are_rejected_offline() {
    for variant in 0..2 {
        let mut program = KbcArtifact::from_bytes(ARTIFACT).unwrap().program;
        let import = program
            .modules
            .iter_mut()
            .flat_map(|module| &mut module.native_imports)
            .find(|import| import.binding.module.package.0 == "game")
            .unwrap();
        assert_eq!(import.requirements.len(), 1);
        if variant == 0 {
            import.requirements.clear();
        } else {
            import.requirements[0].ty = AbiType::Builtin(BuiltinType::I32);
        }
        assert!(KbcArtifact::from_program(program, Default::default()).is_err());
    }
}

#[cfg(feature = "source")]
mod source {
    use super::*;
    use kagari_common::source::SourceFile;

    #[test]
    fn source_emission_matches_the_offline_fixture() {
        let engine = engine(Rc::new(Cell::new(0)));
        let artifact = engine
            .compile_to_artifact(
                SourceFile::new("memory://native-bounds.kgr", SOURCE),
                Default::default(),
                Default::default(),
            )
            .unwrap();
        assert_eq!(artifact.to_bytes().unwrap(), ARTIFACT);
    }

    #[test]
    fn missing_trait_implementations_are_static_errors_before_native_execution() {
        let calls = Rc::new(Cell::new(0));
        let engine = engine(calls.clone());
        for source in [
            "fn main() -> i32 { game::bounds::echo(42, 0usize) }",
            "use game::bounds::{Marker, echo}; struct Token {} impl Marker for Token {} fn main() { echo(Token {}, 0usize); }",
            "use game::bounds::{Container, Marker, echo}; struct Token {} impl Container<i32> for Token {} impl Marker for Token {} fn main() { echo(Token {}, 0usize); }",
        ] {
            assert!(
                engine
                    .compile_source(
                        SourceFile::new("memory://invalid-native-bounds.kgr", source),
                        Default::default()
                    )
                    .is_err()
            );
        }
        assert_eq!(calls.get(), 0);
    }
}
