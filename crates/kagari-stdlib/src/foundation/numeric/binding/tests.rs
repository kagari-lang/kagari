//! Installed float imports retain checked carrier admission and domain errors.
use crate::namespaces;
use kagari_bytecode::{instruction::NativeImportId, module::CallableTarget};
use kagari_compiler::{bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir};
use kagari_hir::analysis::AnalysisDatabase;
use kagari_runtime::{
    Runtime, RuntimeConfig,
    error::RuntimeErrorKind,
    frame::driver::ReturnAction,
    gc::GcHeapConfig,
    module::ExecutableModule,
    native::{binding::NativeResult, context::ScriptInvoker, module::NativeModule},
    value::Value,
};
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use kagari_types::{scalar::BuiltinType, ty::Ty};
use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    sync::Arc,
};

// Use the ordinary frame driver, as in tests/support/native/invoke.rs. Its
// existing stdlib inclusion is private to the runtime-boundary test module.
fn invoke(
    runtime: &Runtime,
    owner: &ExecutableModule,
    slot: usize,
    arguments: &[Value],
) -> NativeResult<Value> {
    let stack = runtime.enter_execution_stack(owner)?;
    unsafe {
        stack.push_callable_transport(
            runtime,
            owner.clone(),
            CallableTarget::Native(NativeImportId::new(slot)),
            arguments,
            None,
        )
    }?;
    stack.start_native_entry(runtime, unsafe {
        // SAFETY: The callback rejects execution or returns only a scalar; it retains no raw edges.
        ScriptInvoker::new(|_, _, _, _| panic!("float scalar methods must not reenter script"))
    })?;
    let ReturnAction::Complete(value) = unsafe { stack.finish_native_return_transport(runtime) }?
    else {
        panic!("expected root native return");
    };
    Ok(value)
}

fn fixture() -> (Runtime, ExecutableModule) {
    let modules = crate::modules().unwrap();
    let mut analysis = AnalysisDatabase::default();
    analysis.set_native_modules(
        modules
            .iter()
            .map(|module| Arc::new(module.to_declaration()))
            .collect(),
    );
    let mut sources = SourceDatabase::default();
    let root = sources
        .set(
            "float-carriers.kgr",
            r#"
pub fn unary32(value: f32) -> f32 { value.abs() }
pub fn binary32(value: f32, other: f32) -> f32 { value.min(other) }
pub fn clamp32(value: f32, min: f32, max: f32) -> f32 { value.clamp(min, max) }
pub fn bits32(bits: u32) -> f32 { f32::from_bits(bits) }
pub fn unary64(value: f64) -> f64 { value.abs() }
pub fn binary64(value: f64, other: f64) -> f64 { value.min(other) }
pub fn clamp64(value: f64, min: f64, max: f64) -> f64 { value.clamp(min, max) }
"#
            .into(),
            SourceLayer::Base,
        )
        .unwrap();
    let snapshot = analysis
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let checked = snapshot.check_program(root, &Default::default()).unwrap();
    let mir = lower_program_to_mir(&checked, &Default::default()).unwrap();
    let program = lower_program_to_bytecode(&mir, &Default::default())
        .unwrap()
        .into_unverified();
    let mut runtime = Runtime::new(RuntimeConfig {
        gc: GcHeapConfig {
            collection_threshold: Some(1),
            ..Default::default()
        },
        ..Default::default()
    });
    NativeModule::install_all(&modules, &mut runtime).unwrap();
    let loaded = runtime.load_program("float-carriers", program).unwrap();
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();
    (runtime, loaded)
}

fn import(
    loaded: &ExecutableModule,
    method: &str,
    parameter: BuiltinType,
) -> (ExecutableModule, usize) {
    let mut matches = Vec::new();
    for owner in loaded.members() {
        for (slot, import) in owner.bytecode.native_imports.iter().enumerate() {
            let path = owner
                .definitions()
                .resolve(import.instance.declaration)
                .unwrap();
            if path.last().is_some_and(|segment| segment.name == method)
                && import.signature.params.first() == Some(&Ty::Builtin(parameter))
            {
                assert_eq!(path.module(), &namespaces::module("core", "num"));
                assert_eq!(import.binding, import.instance.declaration);
                matches.push((owner.clone(), slot));
            }
        }
    }
    assert_eq!(matches.len(), 1, "{parameter:?}::{method}");
    matches.pop().unwrap()
}

fn assert_idle(runtime: &Runtime) {
    assert_eq!(runtime.gc().active_roots(), 0);
    assert_eq!(runtime.resources().counters().current_call_depth, 0);
    assert!(!runtime.is_quarantined());
    assert_eq!(runtime.collect_garbage().unwrap().live_objects, 0);
}

fn reject_then_call(
    runtime: &Runtime,
    target: &(ExecutableModule, usize),
    malformed: &[Value],
    valid: &[Value],
    expected: Value,
) {
    let (owner, slot) = target;
    let error = catch_unwind(AssertUnwindSafe(|| {
        invoke(runtime, owner, *slot, malformed)
    }))
    .unwrap_or_else(|_| panic!("native invocation panicked for {malformed:?}"))
    .unwrap_err();
    assert_eq!(
        error.kind(),
        RuntimeErrorKind::ModuleValidation,
        "{malformed:?}: {error:?}"
    );
    // Public native-frame admission checks variants and semantic ranges before
    // constructing CallContext; deliberately do not bypass that safety boundary.
    assert_eq!(
        error.message(),
        "module validation failed: invalid frame argument type or window"
    );
    assert_idle(runtime);
    assert_eq!(invoke(runtime, owner, *slot, valid).unwrap(), expected);
    assert_idle(runtime);
}

#[test]
fn installed_float_imports_reject_malformed_carriers_and_remain_callable() {
    let (runtime, loaded) = fixture();
    for (ty, value, low, high, wrong) in [
        (
            BuiltinType::F32,
            Value::F32(0.5),
            Value::F32(-1.0),
            Value::F32(1.0),
            Value::F64(0.5),
        ),
        (
            BuiltinType::F64,
            Value::F64(0.5),
            Value::F64(-1.0),
            Value::F64(1.0),
            Value::F32(0.5),
        ),
    ] {
        for (method, valid, expected) in [
            ("abs", vec![value], value),
            ("min", vec![value, low], low),
            ("clamp", vec![high, low, value], value),
        ] {
            let target = import(&loaded, method, ty);
            for index in 0..valid.len() {
                let mut malformed = valid.clone();
                malformed[index] = wrong;
                reject_then_call(&runtime, &target, &malformed, &valid, expected);
            }
        }
        let target = import(&loaded, "abs", ty);
        reject_then_call(&runtime, &target, &[Value::Bool(true)], &[low], high);
    }
    let target = import(&loaded, "from_bits", BuiltinType::U32);
    for malformed in [
        Value::F32(1.0),
        Value::U64(0),
        Value::I32(0),
        Value::I64(-1),
        Value::I64(1i64 << 32),
    ] {
        reject_then_call(
            &runtime,
            &target,
            &[malformed],
            &[Value::I64(0x3f80_0000)],
            Value::F32(1.0),
        );
    }
}

#[test]
fn installed_clamp_distinguishes_carrier_errors_from_domain_traps() {
    let (runtime, loaded) = fixture();
    for (ty, value, low, high, nan, wrong) in [
        (
            BuiltinType::F32,
            Value::F32(0.5),
            Value::F32(-1.0),
            Value::F32(1.0),
            Value::F32(f32::NAN),
            Value::F64(1.0),
        ),
        (
            BuiltinType::F64,
            Value::F64(0.5),
            Value::F64(-1.0),
            Value::F64(1.0),
            Value::F64(f64::NAN),
            Value::F32(1.0),
        ),
    ] {
        let target = import(&loaded, "clamp", ty);
        let valid = [value, low, high];
        reject_then_call(&runtime, &target, &[value, nan, wrong], &valid, value);
        for arguments in [[value, nan, high], [value, high, low]] {
            let error = catch_unwind(AssertUnwindSafe(|| {
                invoke(&runtime, &target.0, target.1, &arguments)
            }))
            .expect("invalid clamp bounds must not panic")
            .unwrap_err();
            assert_eq!(error.kind(), RuntimeErrorKind::ScriptTrap);
            assert!(
                error
                    .message()
                    .contains("clamp requires non-NaN bounds with min <= max")
            );
            assert_idle(&runtime);
            assert_eq!(
                invoke(&runtime, &target.0, target.1, &valid).unwrap(),
                value
            );
            assert_idle(&runtime);
        }
    }
}
