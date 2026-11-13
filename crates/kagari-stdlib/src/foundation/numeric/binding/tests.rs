//! Installed float imports retain checked carrier admission and domain errors.
use crate::namespaces;
use kagari_bytecode::{instruction::NativeImportId, module::CallableTarget};
use kagari_common::identity::map::DefinitionContext;
use kagari_compiler::{bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir};
use kagari_gc::heap::HeapStats;
use kagari_hir::{analysis::AnalysisDatabase, compile_input::program::CheckedCompilation};
use kagari_runtime::{
    Runtime, RuntimeConfig,
    error::RuntimeErrorKind,
    execution::memory::GcHeapConfig,
    frame::driver::ReturnAction,
    module::{ExecutableModule, VerifiedProgram},
    native::{binding::NativeResult, conversion::context::ConversionContext, module::NativeModule},
    value::{carriers::RootedValue, view::ValueView},
};
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use kagari_types::{scalar::BuiltinType, ty::Ty};
use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    sync::Arc,
};

// Use the real Native frame driver, including complete scalar admission and
// result retention before the execution source retires.
fn invoke(
    runtime: &mut Runtime,
    owner: &ExecutableModule,
    slot: usize,
    arguments: &[ValueView<'_>],
) -> NativeResult<RootedValue> {
    let mut conversion = ConversionContext::new(runtime, owner)?;
    let arguments = conversion.argument_values(arguments)?;
    let mut stack_activation = conversion.runtime_mut().enter_execution_stack(owner)?;
    stack_activation.with(|runtime, stack| {
        stack.push_callable(
            runtime,
            owner.clone(),
            CallableTarget::Native(NativeImportId::new(slot)),
            arguments,
            None,
        )?;
        stack.start_native_entry(runtime, |_, _, _, _, _| {
            panic!("float scalar methods must not reenter script")
        })?;
        let ReturnAction::Complete(value) = stack.finish_native_return(runtime)? else {
            panic!("expected root native return");
        };
        stack.retain_result(runtime, value)
    })
}

fn fixture() -> (Runtime, ExecutableModule, HeapStats) {
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
            include_str!("../../../../tests/fixtures/native/float-native-carriers.kgr").into(),
            SourceLayer::Base,
        )
        .unwrap();
    let snapshot = analysis
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let checked = snapshot.check_program(root, &Default::default()).unwrap();
    let mir = lower_program_to_mir(
        &CheckedCompilation::from_program(&checked, &Default::default()).unwrap(),
        &Default::default(),
    )
    .unwrap();
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
    let program = VerifiedProgram::new(program, &DefinitionContext::new().unwrap()).unwrap();
    let loaded = runtime
        .load_verified_program("float-carriers", program)
        .unwrap();
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();
    runtime.collect_garbage().unwrap();
    let baseline = runtime.gc_stats();
    (runtime, loaded, baseline)
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

fn assert_idle(runtime: &mut Runtime, baseline: &HeapStats) {
    assert_eq!(
        runtime.gc_stats().root_registrations,
        baseline.root_registrations
    );
    assert_eq!(runtime.resources().counters().current_call_depth, 0);
    assert!(!runtime.is_quarantined());
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc_stats().objects, baseline.objects);
}

fn assert_float(actual: &RootedValue, expected: ValueView<'_>) {
    if let Some(expected) = expected.as_f32() {
        assert_eq!(actual.as_f32(), Some(expected));
    } else {
        assert_eq!(
            actual.as_f64(),
            Some(expected.as_f64().expect("float witness"))
        );
    }
}

fn reject_then_call(
    runtime: &mut Runtime,
    target: &(ExecutableModule, usize),
    malformed: &[ValueView<'_>],
    valid: &[ValueView<'_>],
    expected: ValueView<'_>,
    baseline: &HeapStats,
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
    // constructing NativeCall; deliberately do not bypass that safety boundary.
    assert_eq!(
        error.to_string(),
        "module validation failed: the representation does not match the admitted type or layout"
    );
    assert_idle(runtime, baseline);
    assert_float(&invoke(runtime, owner, *slot, valid).unwrap(), expected);
    assert_idle(runtime, baseline);
}

#[test]
fn installed_float_imports_reject_malformed_carriers_and_remain_callable() {
    let (mut runtime, loaded, baseline) = fixture();
    for (ty, value, low, high, wrong) in [
        (
            BuiltinType::F32,
            ValueView::from(0.5f32),
            ValueView::from(-1.0f32),
            ValueView::from(1.0f32),
            ValueView::from(0.5f64),
        ),
        (
            BuiltinType::F64,
            ValueView::from(0.5f64),
            ValueView::from(-1.0f64),
            ValueView::from(1.0f64),
            ValueView::from(0.5f32),
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
                reject_then_call(
                    &mut runtime,
                    &target,
                    &malformed,
                    &valid,
                    expected,
                    &baseline,
                );
            }
        }
        let target = import(&loaded, "abs", ty);
        reject_then_call(
            &mut runtime,
            &target,
            &[ValueView::from(true)],
            &[low],
            high,
            &baseline,
        );
    }
    let target = import(&loaded, "from_bits", BuiltinType::U32);
    for malformed in [
        ValueView::from(1.0f32),
        ValueView::from(0u64),
        ValueView::from(0i32),
        ValueView::from(-1i64),
        ValueView::from(1i64 << 32),
    ] {
        reject_then_call(
            &mut runtime,
            &target,
            &[malformed],
            &[ValueView::from(0x3f80_0000i64)],
            ValueView::from(1.0f32),
            &baseline,
        );
    }
}

#[test]
fn installed_clamp_distinguishes_carrier_errors_from_domain_traps() {
    let (mut runtime, loaded, baseline) = fixture();
    for (ty, value, low, high, nan, wrong) in [
        (
            BuiltinType::F32,
            ValueView::from(0.5f32),
            ValueView::from(-1.0f32),
            ValueView::from(1.0f32),
            ValueView::from(f32::NAN),
            ValueView::from(1.0f64),
        ),
        (
            BuiltinType::F64,
            ValueView::from(0.5f64),
            ValueView::from(-1.0f64),
            ValueView::from(1.0f64),
            ValueView::from(f64::NAN),
            ValueView::from(1.0f32),
        ),
    ] {
        let target = import(&loaded, "clamp", ty);
        let valid = [value, low, high];
        reject_then_call(
            &mut runtime,
            &target,
            &[value, nan, wrong],
            &valid,
            value,
            &baseline,
        );
        for arguments in [[value, nan, high], [value, high, low]] {
            let error = catch_unwind(AssertUnwindSafe(|| {
                invoke(&mut runtime, &target.0, target.1, &arguments)
            }))
            .expect("invalid clamp bounds must not panic")
            .unwrap_err();
            assert_eq!(error.kind(), RuntimeErrorKind::ScriptTrap);
            assert!(
                error
                    .to_string()
                    .contains("clamp requires non-NaN bounds with min <= max")
            );
            assert_idle(&mut runtime, &baseline);
            assert_float(
                &invoke(&mut runtime, &target.0, target.1, &valid).unwrap(),
                value,
            );
            assert_idle(&mut runtime, &baseline);
        }
    }
}
