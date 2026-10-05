#![cfg(feature = "source")]
#[path = "support/native_enums.rs"]
mod provider;
use kagari_embed::{
    BytecodeArtifact,
    engine::{EngineConfig, KagariEngine},
    program::PreparedProgram,
};
use kagari_runtime::value::Value;
use kagari_source::source::SourceFile;

fn engine() -> KagariEngine {
    let mut builder = KagariEngine::builder().unwrap();
    let mut config = EngineConfig::default();
    config.default_runtime.gc.collection_threshold = Some(1);
    builder.config(config);
    builder.install(provider::module().unwrap()).unwrap();
    builder.build().unwrap()
}

fn artifact(engine: &KagariEngine, body: &str) -> BytecodeArtifact {
    engine
        .compile_to_artifact(
            SourceFile::new(
                "memory://native-enums.kgr",
                format!("use external::enums;\n{body}"),
            ),
            Default::default(),
        )
        .unwrap()
}

#[test]
fn native_registered_enums_execute_from_serialized_artifacts_with_traced_payloads() {
    let compiler = engine();
    for body in [
        "fn main() -> i32 { match enums::make(40, 2) { enums::Event::Data(value, sequence) => value + sequence, enums::Event::Closed => 0 } }",
        "fn main() -> i32 { val event = enums::roundtrip(enums::make([40], 2)); match event { enums::Event::Data(value, sequence) => value[0usize] + sequence, enums::Event::Closed => 0 } }",
        "struct Payload { val value: i32 } fn wrap<T>(value: T) -> enums::Event<T> { enums::make(value, 2) } fn main() -> i32 { val payload = Payload { value: 40 }; match wrap(payload) { enums::Event::Data(value, sequence) => value.value + sequence, enums::Event::Closed => 0 } }",
        "fn main() -> i32 { val event = enums::Event::Data(40, 2); if enums::is_data(event) && !enums::is_data(enums::closed(0, 0)) { enums::sequence(enums::roundtrip(event)) + 40 } else { 0 } }",
        "fn main() -> i32 { val nested = enums::make(enums::make([38], 2), 2); match enums::roundtrip(nested) { enums::Event::Data(inner, outer) => match inner { enums::Event::Data(value, sequence) => value[0usize] + sequence + outer, enums::Event::Closed => 0 }, enums::Event::Closed => 0 } }",
    ] {
        let original = artifact(&compiler, body);
        let decoded = BytecodeArtifact::from_bytes(&original.to_bytes().unwrap()).unwrap();
        let program =
            PreparedProgram::from_artifact(decoded, &Default::default(), &Default::default())
                .unwrap();
        // A fresh Engine loads the executable; it never analyzes the source above.
        let mut runtime = engine().runtime(Default::default());
        let loaded = runtime.load_program(&program, Default::default()).unwrap();
        for _ in 0..3 {
            let report = runtime
                .execute(&loaded, "main", &[], &Default::default())
                .unwrap();
            assert_eq!(
                report
                    .return_value
                    .value(runtime.runtime().gc())
                    .expect("retained execution result"),
                Value::I32(42),
                "{body}"
            );
            drop(report);
            assert_eq!(runtime.runtime().gc().active_roots(), 0);
            runtime.runtime().collect_garbage().unwrap();
            assert_eq!(runtime.runtime().gc().allocated_objects(), 0);
        }
    }
}

#[test]
fn native_enum_operations_reject_wrong_payloads_members_and_indices() {
    for body in [
        "fn main() { enums::wrong_payload(40, 2); }",
        "fn main() { enums::wrong_count(40, 2); }",
        "fn main() { enums::foreign_variant(40, 2); }",
        "fn main() { enums::wrong_field(enums::make(40, 2)); }",
        "fn main() { enums::wrong_variant(enums::make(40, 2)); }",
    ] {
        let engine = engine();
        let program = PreparedProgram::from_artifact(
            artifact(&engine, body),
            &Default::default(),
            &Default::default(),
        )
        .unwrap();
        let mut runtime = engine.runtime(Default::default());
        let loaded = runtime.load_program(&program, Default::default()).unwrap();
        assert!(
            runtime
                .execute(&loaded, "main", &[], &Default::default())
                .is_err(),
            "{body}"
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
