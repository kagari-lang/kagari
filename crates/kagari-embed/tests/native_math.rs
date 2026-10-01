//! Math and array packages share registration, artifact execution and tooling.
use kagari_bytecode::{
    artifact::KbcArtifact,
    instruction::{BytecodeInstruction, ConstantOperand},
};
use kagari_embed::{
    context::ExecutionContext,
    engine::{EngineConfig, KagariEngine},
    error::{EmbeddingError, RuntimeFailureKind},
    program::PreparedProgram,
};
use kagari_runtime::value::Value;

const ARTIFACT: &[u8] = include_bytes!("fixtures/native_provider.kbc");

fn fixture() -> PreparedProgram {
    PreparedProgram::from_artifact(
        KbcArtifact::from_bytes(ARTIFACT).unwrap(),
        &Default::default(),
        &Default::default(),
    )
    .unwrap()
}

fn math_input(input: f64) -> PreparedProgram {
    let mut program = KbcArtifact::from_bytes(ARTIFACT).unwrap().program;
    let root = program.root.index();
    let mut replaced = 0;
    for function in &mut program.modules[root].functions {
        if !["floor_value", "ceil_value", "sqrt_value"].contains(&function.name.as_str()) {
            continue;
        }
        for instruction in &mut function.instructions {
            if let BytecodeInstruction::LoadConst {
                constant: ConstantOperand::F64(value),
                ..
            } = instruction
            {
                *value = input;
                replaced += 1;
            }
        }
    }
    assert_eq!(replaced, 3);
    program.modules[root]
        .constants
        .push(ConstantOperand::F64(input));
    // Build and decode a fresh bytecode-only product, including non-finite wire
    // values that source numeric literals intentionally cannot express.
    let artifact = KbcArtifact::from_program(program, Default::default()).unwrap();
    PreparedProgram::from_artifact(
        KbcArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap(),
        &Default::default(),
        &Default::default(),
    )
    .unwrap()
}

#[test]
fn encoded_math_preserves_rounding_signed_zero_and_domain_checks() {
    let context = ExecutionContext::default();
    let mut config = EngineConfig::default();
    config.default_runtime.gc.collection_threshold = Some(1);
    let mut runtime = KagariEngine::new(config).runtime(context.clone());
    for (entry, input, expected) in [
        ("floor_value", 2.75, 2.0),
        ("floor_value", -2.75, -3.0),
        ("ceil_value", 2.25, 3.0),
        ("ceil_value", -2.25, -2.0),
        ("sqrt_value", 9.0, 3.0),
        ("sqrt_value", f64::MAX, f64::MAX.sqrt()),
        ("floor_value", -0.0, -0.0),
        ("ceil_value", -0.0, -0.0),
        ("sqrt_value", -0.0, -0.0),
    ] {
        let loaded = runtime
            .load_program(&math_input(input), Default::default())
            .unwrap();
        let result = runtime
            .execute(&loaded, entry, &[], &context)
            .unwrap()
            .return_value;
        let Value::F64(result) = result else {
            panic!("math returned a non-f64 value");
        };
        assert_eq!(result.to_bits(), expected.to_bits());
    }
    for entry in ["floor_value", "ceil_value", "sqrt_value"] {
        for input in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let loaded = runtime
                .load_program(&math_input(input), Default::default())
                .unwrap();
            let error = runtime.execute(&loaded, entry, &[], &context).unwrap_err();
            assert!(matches!(
                error,
                EmbeddingError::Runtime {
                    kind: RuntimeFailureKind::ScriptTrap,
                    ..
                }
            ));
            assert_eq!(runtime.runtime().gc().active_roots(), 0);
            assert_eq!(
                runtime.runtime().resources().counters().current_call_depth,
                0
            );
        }
    }
    let loaded = runtime
        .load_program(&math_input(-1.0), Default::default())
        .unwrap();
    assert!(
        runtime
            .execute(&loaded, "sqrt_value", &[], &context)
            .is_err()
    );
    let loaded = runtime
        .load_program(&fixture(), Default::default())
        .unwrap();
    assert_eq!(
        runtime
            .execute(&loaded, "main", &[], &context)
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

#[test]
fn optional_packages_keep_generated_views_and_require_runtime_installation() {
    let engine = KagariEngine::default();
    let sources = engine.native_declaration_sources();
    let math = sources
        .iter()
        .find(|source| source.uri == "kagari://native/kagari-std/math.kgr")
        .unwrap();
    assert_eq!(math.text, include_str!("../../../stdlib/math.kgr"));
    assert!(math.text.contains("pub fn sqrt(value: f64) -> f64;"));
    let engine = KagariEngine::builder()
        .install_standard_library(false)
        .build()
        .unwrap();
    assert!(engine.native_declaration_sources().is_empty());
    let mut runtime = engine.runtime(Default::default());
    assert!(
        runtime
            .load_program(&fixture(), Default::default())
            .is_err()
    );
}

#[cfg(feature = "source")]
mod source {
    use super::*;
    use kagari_common::{source::SourceFile, source_database::SourceLayer};
    use kagari_native_macros::native_module;

    #[native_module("game::math")]
    mod math {
        /// Square an application-owned number.
        #[native]
        pub fn square(value: f64) -> f64 {
            value * value
        }
    }

    #[test]
    fn application_and_default_math_compose_with_array_and_check_signatures() {
        let engine = KagariEngine::builder()
            .install(math::native_api())
            .build()
            .unwrap();
        let artifact = engine
            .compile_to_artifact(
                SourceFile::new(
                    "memory://math-composition.kgr",
                    "use game::math::square; fn main() -> f64 { val a = [square(3.0), std::math::sqrt(16.0)]; std::math::ceil(a[0usize]) + std::math::floor(a[1usize]) }",
                ),
                Default::default(),
                Default::default(),
            )
            .unwrap();
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
            Value::F64(13.0)
        );
        for text in [
            "fn main() -> f64 { std::math::sqrt(9usize) }",
            "fn main() -> usize { std::math::floor(2.5) }",
        ] {
            assert!(
                engine
                    .compile_source(
                        SourceFile::new("memory://invalid-math.kgr", text),
                        Default::default()
                    )
                    .is_err()
            );
        }
    }

    #[test]
    fn generated_math_navigation_and_documentation_use_the_registered_owner() {
        let engine = KagariEngine::default();
        let text = "fn main() -> f64 { std::math::sqrt(9.0) }";
        let file = engine
            .set_source("memory://math-tooling.kgr", text.into(), SourceLayer::Base)
            .unwrap();
        let snapshot = engine
            .analyze(
                engine.source_snapshot(),
                Default::default(),
                &Default::default(),
            )
            .unwrap();
        let offset = text.find("sqrt").unwrap();
        let declaration = snapshot.definition_at(file, offset).unwrap();
        let source = snapshot.source(declaration.location.file).unwrap();
        assert_eq!(source.name(), "kagari://native/kagari-std/math.kgr");
        assert_eq!(
            &source.text()[declaration.location.range.start..declaration.location.range.end],
            "sqrt"
        );
        let docs = snapshot.documentation_at(file, offset).unwrap();
        assert!(docs.documentation.contains("negative or non-finite"));
        assert!(docs.written_signature.contains("sqrt(value: f64) -> f64"));
    }
}
