//! Structural native values preserve applied types, heap roots and error origins.
#![cfg(feature = "source")]
use kagari_bytecode::artifact::KbcArtifact;
use kagari_common::source::SourceFile;
use kagari_embed::{
    context::ExecutionContext,
    engine::{EngineConfig, KagariEngine},
    error::{EmbeddingError, RuntimeFailureKind},
    program::PreparedProgram,
};
use kagari_native_macros::native_module;
use kagari_runtime::value::Value;

#[native_module("game::values")]
mod values {
    use kagari_runtime::{
        error::{RuntimeError, RuntimeErrorKind},
        native_value::{NativeCall, NativeResult, NativeValue, result::NativeResultValue},
    };
    use std::cmp::Ordering;

    /// Keep the optional value while swapping the tuple's fields.
    #[native]
    pub fn reorder<T: NativeValue>(value: (String, Option<T>)) -> (Option<T>, String) {
        (value.1, value.0)
    }
    /// Preserve a nested tuple and its single-element tuple field.
    #[native]
    pub fn nested(value: ((usize,), (bool, String))) -> ((usize,), (bool, String)) {
        value
    }
    /// Compare integers and return the script Ordering representation.
    #[native]
    pub fn compare(left: i32, right: i32) -> Ordering {
        left.cmp(&right)
    }
    /// Preserve an Ordering received from script.
    #[native]
    pub fn ordering(value: Ordering) -> Ordering {
        value
    }
    /// Preserve the original script Result object and its error origin.
    #[native]
    pub fn echo_result<T: NativeValue, E: NativeValue>(
        value: NativeResultValue<T, E>,
    ) -> NativeResultValue<T, E> {
        value
    }
    /// Create a script Ok or Err; neither branch is a native execution failure.
    #[native]
    pub fn outcome(
        value: i32,
        #[context] call: &NativeCall,
    ) -> NativeResult<NativeResultValue<i32, String>> {
        NativeResultValue::from_result(
            call,
            if value >= 0 {
                Ok(value)
            } else {
                Err("negative".into())
            },
        )
    }
    /// Read the script Result branch through checked typed payloads.
    #[native]
    pub fn payload(value: NativeResultValue<i32, String>) -> NativeResult<Option<i32>> {
        Ok(value.payload()?.ok())
    }
    /// Demonstrate that a native execution failure remains an execution failure.
    #[native]
    pub fn trap() -> NativeResult<NativeResultValue<i32, String>> {
        Err(RuntimeError::new(
            RuntimeErrorKind::ScriptTrap,
            "native failure",
        ))
    }
}

fn engine() -> KagariEngine {
    let mut config = EngineConfig::default();
    config.default_runtime.gc.collection_threshold = Some(1);
    KagariEngine::builder()
        .config(config)
        .install(values::native_api())
        .build()
        .unwrap()
}

fn program(engine: &KagariEngine, source: &str) -> PreparedProgram {
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new("memory://native-values.kgr", source),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    PreparedProgram::from_artifact(
        KbcArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap(),
        &Default::default(),
        &Default::default(),
    )
    .unwrap()
}

#[test]
fn nested_tuple_values_and_ordering_round_trip_through_native_and_callback_frames() {
    let engine = engine();
    let sources = engine.native_declaration_sources();
    let text = &sources
        .iter()
        .find(|source| source.uri == "kagari://native/game/values.kgr")
        .unwrap()
        .text;
    assert!(text.contains("fn reorder<T0>(value: (String, Option<T0>)) -> (Option<T0>, String)"));
    assert!(
        text.contains("fn nested(value: ((usize,), (bool, String))) -> ((usize,), (bool, String))")
    );
    assert!(text.contains("fn compare(left: i32, right: i32) -> Ordering"));
    let program = program(
        &engine,
        r#"
        use game::values::{reorder, nested, compare, ordering};
        fn main() -> i32 {
            val values = ArrayList::from_fn(2usize, |index| reorder(("rooted", Some([21]))));
            val tuple = nested(((1usize,), (true, "nested")));
            match (values[0usize], values[1usize], tuple) {
                ((Some(first), label), (Some(second), _), ((index,), (flag, text))) => {
                    if label == "rooted" && index == 1usize
                        && flag && text == "nested"
                        && ordering(compare(1, 2)) == Ordering::Less
                        && ordering(compare(2, 2)) == Ordering::Equal
                        && ordering(compare(2, 1)) == Ordering::Greater {
                        first[0usize] + second[0usize]
                    } else { 0 }
                },
                _ => 0
            }
        }
    "#,
    );
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime.load_program(&program, Default::default()).unwrap();
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
    runtime.runtime().collect_garbage().unwrap();
    assert_eq!(runtime.runtime().gc().allocated_objects(), 0);
}

#[test]
fn script_result_branches_remain_values_and_native_failures_remain_traps() {
    let engine = engine();
    let program = program(
        &engine,
        r#"
        use game::values::{outcome, payload, trap};
        fn main() -> i32 {
            match (outcome(42), outcome(-1)) {
                (Ok(value), Err(error)) => {
                    if error == "negative" && payload(Ok(value)) == Some(42)
                        && payload(Err("empty")) == None { value } else { 0 }
                },
                _ => 0
            }
        }
        fn fails() -> Result<i32, String> { trap() }
        fn returned_error() -> Result<i32, String> { outcome(-1) }
    "#,
    );
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime.load_program(&program, Default::default()).unwrap();
    let error = runtime
        .execute(&loaded, "fails", &[], &context)
        .unwrap_err();
    assert!(
        matches!(error, EmbeddingError::Runtime { kind: RuntimeFailureKind::ScriptTrap, message, .. } if message.contains("native failure"))
    );
    let report = runtime
        .execute(&loaded, "returned_error", &[], &context)
        .unwrap();
    assert!(
        report
            .failure
            .as_ref()
            .unwrap()
            .message
            .contains("negative")
    );
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
fn native_result_round_trip_preserves_the_original_error_frame() {
    let engine = engine();
    let program = program(
        &engine,
        "use game::values::echo_result;\nfn fail() -> Result<i32, String> {\n    Err(\"original\")\n}\nfn main() -> Result<i32, String> { echo_result(fail()) }",
    );
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime.load_program(&program, Default::default()).unwrap();
    let report = runtime.execute(&loaded, "main", &[], &context).unwrap();
    let failure = report.failure.unwrap();
    assert!(failure.message.contains("original"));
    assert_eq!(failure.trace.frames[0].function_name, "fail");
    assert_eq!(failure.trace.frames[0].line, Some(3));
    assert_eq!(runtime.runtime().gc().active_roots(), 0);
    runtime.runtime().collect_garbage().unwrap();
    assert_eq!(runtime.runtime().gc().allocated_objects(), 0);
}

#[test]
fn tuple_and_enum_input_contracts_reject_wrong_shapes_statically() {
    let engine = engine();
    for text in [
        "fn main() { game::values::reorder((\"text\", 1usize)); }",
        "fn main() { game::values::nested((1usize, (true, \"text\"))); }",
        "fn main() { game::values::ordering(1i32); }",
        "fn main() { game::values::payload(Some(1)); }",
    ] {
        assert!(
            engine
                .compile_source(
                    SourceFile::new("memory://invalid-native-value.kgr", text),
                    Default::default()
                )
                .is_err()
        );
    }
}
