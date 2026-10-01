#![cfg(feature = "source")]
mod contracts;
mod external;
use contracts::alter_contracts;
use kagari_bytecode::KbcArtifact;
use kagari_common::SourceFile;
use kagari_embed::{EngineConfig, ExecutionContext, KagariEngine, program::PreparedProgram};
use kagari_runtime::value::Value;

fn artifact(source: &str) -> KbcArtifact {
    KagariEngine::default()
        .compile_to_artifact(
            SourceFile::new("memory://provider-reset.kgr", source),
            Default::default(),
            Default::default(),
        )
        .unwrap()
}

fn execute(source: &str) {
    let bytes = artifact(source).to_bytes().unwrap();
    let program = PreparedProgram::from_artifact(
        KbcArtifact::from_bytes(&bytes).unwrap(),
        &Default::default(),
        &Default::default(),
    )
    .unwrap();
    let context = ExecutionContext::default();
    let mut config = EngineConfig::default();
    config.default_runtime.gc.collection_threshold = Some(1);
    let mut runtime = KagariEngine::new(config).runtime(context.clone());
    let loaded = runtime.load_program(&program, Default::default()).unwrap();
    for _ in 0..3 {
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
}

#[test]
fn minimal_array_provider_executes_from_serialized_artifact() {
    execute(
        "fn main() -> i32 { val a: ArrayList<i32> = ArrayList::new(); a.push(20); a.push(22); if a.len() == 2usize { a[0usize] + a[1usize] } else { 0 } }",
    );
}

#[test]
fn provider_callback_retains_captures_partial_output_and_heap_results() {
    execute(
        r#"fn main() -> i32 {
        val count = [18];
        val arrays = ArrayList::from_fn(2usize, |index| {
            count[0usize] = count[0usize] + 2;
            [count[0usize]]
        });
        arrays[0usize][0usize] + arrays[1usize][0usize]
    }"#,
    );
}

#[test]
fn zero_length_initialization_does_not_call_the_callback() {
    execute(
        r#"fn main() -> i32 {
        val count = [42];
        val a = ArrayList::from_fn(0usize, |index| { count[0usize] = 0; 1 });
        if a.len() == 0usize { count[0usize] } else { 0 }
    }"#,
    );
}

#[test]
fn read_access_is_declared_by_the_provider_and_write_access_stays_checked() {
    execute(
        "fn main() -> i32 { val a = [20, 22]; val view: [i32] = a; if view.len() == 2usize { view[0usize] + view[1usize] } else { 0 } }",
    );
    assert!(
        KagariEngine::default()
            .compile_to_artifact(
                SourceFile::new(
                    "memory://readonly.kgr",
                    "fn main() { val view: [i32] = [1]; view.push(2); }"
                ),
                Default::default(),
                Default::default(),
            )
            .is_err()
    );
}

#[test]
fn structural_agreement_does_not_authorize_an_uninstalled_or_changed_provider() {
    for change in 0..3 {
        let mut original = artifact("fn main() -> usize { [1, 2].len() }");
        alter_contracts(&mut original, |contract| match change {
            0 => contract.key.provider ^= 1,
            1 => contract.version += 1,
            _ => contract.effects.allocates = !contract.effects.allocates,
        });
        // Consistent unsigned contract assertions remain structurally checkable.
        let forged = KbcArtifact::from_program(original.program, Default::default()).unwrap();
        let program =
            PreparedProgram::from_artifact(forged, &Default::default(), &Default::default())
                .unwrap();
        let context = ExecutionContext::default();
        let mut runtime = KagariEngine::default().runtime(context);
        assert!(runtime.load_program(&program, Default::default()).is_err());
        assert_eq!(runtime.runtime().gc().active_roots(), 0);
        assert_eq!(
            runtime.runtime().resources().counters().current_call_depth,
            0
        );
    }
}

#[test]
fn callbacks_abort_without_retaining_native_state_or_partial_arrays() {
    let original = artifact(
        "fn main() -> i32 { val values = ArrayList::from_fn(3usize, |i| { if i == 1usize { 1 / 0 } else { 20 } }); values[0usize] }",
    );
    let program =
        PreparedProgram::from_artifact(original, &Default::default(), &Default::default()).unwrap();
    let context = ExecutionContext::default();
    let mut runtime = KagariEngine::default().runtime(context.clone());
    let loaded = runtime.load_program(&program, Default::default()).unwrap();
    assert!(runtime.execute(&loaded, "main", &[], &context).is_err());
    assert_eq!(runtime.runtime().gc().active_roots(), 0);
    assert_eq!(
        runtime.runtime().resources().counters().current_call_depth,
        0
    );
    runtime.runtime().collect_garbage().unwrap();
    assert_eq!(runtime.runtime().gc().allocated_objects(), 0);
    assert!(!runtime.runtime().is_quarantined());
}

#[test]
fn every_instruction_budget_boundary_unwinds_nested_provider_callbacks() {
    let program = PreparedProgram::from_artifact(
        artifact("fn main() -> i32 { val values = ArrayList::from_fn(2usize, |i| { ArrayList::from_fn(1usize, |j| { [21] }) }); values[0usize][0usize][0usize] + values[1usize][0usize][0usize] }"),
        &Default::default(), &Default::default(),
    ).unwrap();
    let context = ExecutionContext::default();
    let mut runtime = KagariEngine::default().runtime(context.clone());
    let loaded = runtime.load_program(&program, Default::default()).unwrap();
    assert_eq!(
        runtime
            .execute(&loaded, "main", &[], &context)
            .unwrap()
            .return_value,
        Value::I32(42)
    );
    let steps = runtime.runtime().resources().counters().instruction_steps;
    assert!(steps > 20);
    for limit in 0..=steps {
        let mut limited = ExecutionContext::default();
        limited.resources.max_instruction_steps = Some(limit);
        let mut config = EngineConfig::default();
        config.default_runtime.gc.collection_threshold = Some(1);
        let mut runtime = KagariEngine::new(config).runtime(limited.clone());
        let loaded = runtime.load_program(&program, Default::default()).unwrap();
        let result = runtime.execute(&loaded, "main", &[], &limited);
        if limit == steps {
            assert_eq!(result.unwrap().return_value, Value::I32(42));
        } else {
            assert!(result.is_err(), "budget {limit}");
        }
        assert_eq!(runtime.runtime().gc().active_roots(), 0, "budget {limit}");
        assert_eq!(
            runtime.runtime().resources().counters().current_call_depth,
            0
        );
        runtime.runtime().collect_garbage().unwrap();
        assert_eq!(
            runtime.runtime().gc().allocated_objects(),
            0,
            "budget {limit}"
        );
        assert!(!runtime.runtime().is_quarantined());
    }
}
