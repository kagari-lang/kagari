use crate::{
    BytecodeArtifact,
    engine::{EngineConfig, KagariEngine},
    program::PreparedProgram,
};
use kagari_runtime::{module::entry::ProgramEntry, value::Value};
use kagari_source::source::SourceFile;

#[path = "../../../../../tests/support/try_carrier.rs"]
mod native_provider;

#[test]
fn native_carriers_link_into_a_fresh_artifact_only_runtime() {
    fn engine() -> KagariEngine {
        let mut builder = KagariEngine::builder().unwrap();
        let mut config = EngineConfig::default();
        config.default_runtime.gc.collection_threshold = Some(1);
        builder.config(config);
        builder.install(native_provider::module().unwrap()).unwrap();
        builder.build().unwrap()
    }
    let compiler = engine();
    for source in [
        "use external::try_carrier::Carrier; fn forward(value: Carrier<i32>) -> Carrier<String> { value?; Carrier::Data(\"unused\") } fn main()->i32 { match forward(Carrier::Stop) { Carrier::Stop => 42, Carrier::Data(_) => 0 } }",
        "use external::try_carrier::Carrier; struct Payload { val value: Vec<i32> } fn forward<T>(value: Carrier<T>) -> Carrier<T> { Carrier::Data(value?) } fn main()->i32 { val value = Payload { value: Vec::from([42]) }; match forward(Carrier::Data(value)) { Carrier::Data(item) => item.value[0u64], Carrier::Stop => 0 } }",
    ] {
        let artifact = compiler
            .compile_to_artifact(
                SourceFile::new("memory://native-try.kgr", source),
                Default::default(),
            )
            .unwrap();
        let artifact = BytecodeArtifact::from_bytes(
            &artifact.records().to_bytes(artifact.definitions()).unwrap(),
        )
        .unwrap();
        let prepared =
            PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default())
                .unwrap();
        let mut runtime = engine().runtime(Default::default());
        let loaded = runtime.load_program(&prepared, Default::default()).unwrap();
        assert_eq!(
            runtime
                .execute(&loaded, ProgramEntry::Main, &[], &Default::default())
                .unwrap()
                .return_value
                .value(runtime.runtime().gc())
                .expect("retained execution result"),
            Value::I32(42)
        );
        assert_eq!(runtime.runtime().gc().active_roots(), 0);
        runtime.runtime().collect_garbage().unwrap();
        assert_eq!(runtime.runtime().gc().allocated_objects(), 0);
    }
}
