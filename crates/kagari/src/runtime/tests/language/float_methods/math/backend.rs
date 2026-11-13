#[cfg(feature = "native")]
use crate::program::native::NativeCompiler;
use crate::{
    context::{ExecutionContext, JitPolicy},
    runtime::tests::language::float_methods::{engine, programs},
};
use kagari_codegen_cranelift::CraneliftBackend;
use kagari_runtime::execution::{JitExecutionStatus, native::PreparedNativeEntry};
use kagari_runtime::module::entry::ProgramEntry;

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
    let loaded = runtime
        .load_program(program.program(), Default::default())
        .unwrap();
    runtime.collect_garbage().unwrap();
    let roots = runtime.gc_stats().root_registrations;
    let mut backend = CraneliftBackend::for_host().unwrap();
    let prepared = NativeCompiler::default()
        .prepare(
            &program,
            ProgramEntry::Main,
            &mut backend,
            &context.cancellation,
        )
        .and_then(|native_code| (native_code).install(&mut runtime, &loaded))
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
    let value = report
        .return_value
        .as_f64()
        .expect("math fallback must return f64");
    assert_eq!(value.to_bits(), 0.0_f64.to_bits());
    drop(report);
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc_stats().root_registrations, roots);
    assert_eq!(runtime.resources().counters().current_call_depth, 0);
    assert!(!runtime.is_quarantined());
}
