use crate::{
    context::{ExecutionContext, JitPolicy},
    runtime::tests::language::float_methods::{engine, programs},
};
use kagari_codegen_cranelift::CraneliftBackend;
use kagari_runtime::{module::entry::ProgramEntry, value::Value};
use kagari_vm::vm::{JitExecutionStatus, native::PreparedNativeEntry};

#[test]
fn math_body_is_rejected_before_native_entry_then_runs_in_interpreter_fallback() {
    // A single direct body, not a wrapper whose ordinary script call could itself
    // force fallback. The encoded route retains portable MIR, unlike the helper's
    // default direct preparation, so missing input cannot explain rejection.
    let [_direct, (route, program)] = programs("fn main() -> f64 { (0.0f64).sin() }");
    assert_eq!(route, "encoded");
    assert!(program.has_native_input());
    let consumer = engine();
    let context = ExecutionContext {
        jit_policy: JitPolicy::Enabled,
        ..Default::default()
    };
    let mut runtime = consumer.runtime(context.clone());
    let loaded = runtime.load_program(&program, Default::default()).unwrap();
    let mut backend = CraneliftBackend::for_host().unwrap();
    let prepared = runtime
        .prepare_native(
            &program,
            &loaded,
            ProgramEntry::Main,
            &mut backend,
            &context.cancellation,
        )
        .unwrap();
    let PreparedNativeEntry::Unsupported { diagnostics, .. } = &prepared else {
        panic!("expected pre-entry rejection of the float math body, got {prepared:?}");
    };
    // Exclude policy-disabled or missing-portable-input fallback: it must be
    // the real backend's capability rejection of this body.
    assert!(
        diagnostics
            .iter()
            .any(|message| message.contains("Cranelift baseline")),
        "{diagnostics:?}"
    );
    let report = runtime
        .execute_prepared(&loaded, ProgramEntry::Main, &[], &context, &prepared)
        .unwrap();
    let jit = report.jit.as_ref().unwrap();
    assert_eq!(jit.status, JitExecutionStatus::InterpreterFallback);
    assert!(jit.artifact.is_none(), "fallback is not native execution");
    assert_eq!(&jit.diagnostics, diagnostics);
    let Value::F64(value) = report
        .return_value
        .inspect(runtime.runtime(), |view| view.as_f64().map(Value::F64))
        .unwrap()
    else {
        panic!("math fallback must return f64");
    };
    assert_eq!(value.to_bits(), 0.0_f64.to_bits());
    drop(report);
    assert_eq!(runtime.runtime().gc().active_roots(), 0);
    assert_eq!(
        runtime.runtime().resources().counters().current_call_depth,
        0
    );
    assert!(!runtime.runtime().is_quarantined());
}
