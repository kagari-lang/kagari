use crate::{
    BytecodeArtifact,
    context::ExecutionContext,
    engine::{EngineConfig, KagariEngine},
    program::PreparedProgram,
    test_support::cancellation as support,
};
use kagari_runtime::{module::entry::ProgramEntry, value::Value};
use kagari_source::source::SourceFile;

#[test]
fn operator_traps_and_cancellation_release_execution_roots() {
    let mut config = EngineConfig::default();
    config.default_runtime.gc.collection_threshold = Some(1);
    let engine = KagariEngine::new(config);
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new(
                "operator-failure.kgr",
                r#"use std::ops::{Add, Index};

struct Number {val value:i32}
impl Add<i32> for Number {type Output=i32;fn add(self,rhs:i32)->i32 {self.value/rhs}}
impl Index<i32> for Number {type Output=i32;fn index(self,rhs:i32)->i32 {[self.value][rhs]}}
pub fn fail_add()->i32 {Number{value:42}+0}
pub fn fail_index()->i32 {Number{value:42}[2]}
pub fn exhaust()->i32 {var n=1000;while n>0 {val a=Number{value:n};a+1;n-=1;}42}
fn main()->i32 {Number{value:42}+1}
"#,
            ),
            Default::default(),
        )
        .unwrap();
    let artifact =
        BytecodeArtifact::from_bytes(&artifact.records().to_bytes(artifact.definitions()).unwrap())
            .unwrap();
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded_program =
        PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
    let loaded = runtime
        .load_program(&loaded_program, Default::default())
        .unwrap();
    for entry in ["fail_add", "fail_index", "exhaust"] {
        let options = context.clone();
        let cancellation = (entry == "exhaust").then(|| {
            support::cancel_after(
                runtime.runtime(),
                &runtime.runtime().select_program(&loaded).unwrap(),
                30,
            )
        });
        let error = runtime.execute(&loaded, entry, &[], &options).unwrap_err();
        assert!(
            !format!("{error:?}").contains("MissingFunction"),
            "{entry}: {error:?}"
        );
        let expected = if entry == "exhaust" {
            "KG_RUNTIME_CANCELLED"
        } else {
            "KG_RUNTIME_SCRIPT_TRAP"
        };
        assert_eq!(error.code(), expected, "{entry}: {error:?}");
        drop(cancellation);
        assert_eq!(runtime.runtime().gc().active_roots(), 0);
        assert!(!runtime.runtime().is_quarantined());
        assert_eq!(
            runtime
                .execute(&loaded, ProgramEntry::Main, &[], &context)
                .unwrap()
                .return_value
                .value(runtime.runtime().gc())
                .expect("retained execution result"),
            Value::I32(42)
        );
    }
}
