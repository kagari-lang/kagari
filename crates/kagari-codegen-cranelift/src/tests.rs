// Reuse explicit verified-loader fixture preparation at the Runtime boundary.
#[path = "../../kagari-runtime/tests/support/bytecode.rs"]
mod bytecode_fixture;
use super::*;
use bytecode_fixture::load_bytecode;
use kagari::engine::KagariEngine;
use kagari_abi::native::NativeLinkDescription;
use kagari_compiler::{bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir};
use kagari_hir::compile_input::program::CheckedCompilation;
use kagari_mir::{program::VerifiedMirProgram, verify::VerifiedMirModule};
use kagari_runtime::{
    Runtime, RuntimeConfig,
    backend::{BackendInvocationError, native::InstalledNativeFunction},
    error::RuntimeErrorKind,
    module::retention::RetentionOrigin,
    native::module::NativeModule,
    resource::RuntimeLimits,
    value::carriers::RootedValue,
};
use kagari_source::source::SourceFile;
use kagari_stdlib as stdlib;
use rstest::rstest;
use std::{sync::Arc, thread};
use {
    kagari_abi::{native::ExecutableEntryPoint, native_call::JIT_POLL_EXECUTION_SYMBOL},
    kagari_contract::native::ExecutableSafepointKind,
};

fn mir(source: &str) -> VerifiedMirProgram {
    let checked = KagariEngine::with_native_modules(Default::default(), stdlib::modules().unwrap())
        .unwrap()
        .compile_source(SourceFile::new("native.kgr", source))
        .unwrap();
    lower_program_to_mir(
        &CheckedCompilation::from_program(checked.program(), &Default::default()).unwrap(),
        &Default::default(),
    )
    .unwrap()
}

fn root(program: &VerifiedMirProgram) -> &VerifiedMirModule {
    program
        .modules()
        .iter()
        .find(|module| &module.identity == program.root())
        .unwrap()
}

fn runtime() -> Runtime {
    let mut runtime = Runtime::new(RuntimeConfig {
        limits: RuntimeLimits {
            ..Default::default()
        },
        ..Default::default()
    });
    NativeModule::install_all(&stdlib::modules().unwrap(), &mut runtime).unwrap();
    runtime
}

fn compile(
    backend: &mut CraneliftBackend,
    mir: &VerifiedMirProgram,
) -> Result<NativeCompilationProduct, BackendCompileError> {
    let links = NativeLinkDescription::scalar();
    let module = root(mir);
    let main = module
        .functions
        .iter()
        .find(|function| function.name == "main")
        .unwrap();
    backend.compile_function(BackendFunctionInput::new(module, main.id, &links).unwrap())
}

#[test]
fn cranelift_backend_initializes_host_target_without_leaking_backend_types() {
    let backend = CraneliftBackend::for_host().unwrap();
    let config = backend.configuration();
    assert_eq!(config.backend.as_str(), "cranelift");
    assert!(!config.target.triple.is_empty());
    assert!(matches!(config.target.pointer_width, 32 | 64));
    assert!(!config.options.is_empty());
    assert_eq!(
        config,
        CraneliftBackend::for_host().unwrap().configuration()
    );
}

#[test]
fn finalized_code_and_installed_runtimes_move_execute_and_release_on_other_threads() {
    fn shareable<T: Send + Sync>() {}
    shareable::<NativeCompilationProduct>();
    shareable::<InstalledNativeFunction>();
    let mir = mir("fn main() -> i32 { 40 + 2 }");
    let bytecode = lower_program_to_bytecode(&mir, &Default::default())
        .map(|program| program.into_unverified())
        .unwrap();
    let mut backend = CraneliftBackend::for_host().unwrap();
    let product = Arc::new(compile(&mut backend, &mir).unwrap());
    let owner = Arc::downgrade(&product.owner);
    let second = product.clone();
    drop((backend, mir));
    let spawn = |product: Arc<NativeCompilationProduct>| {
        let mut runtime = runtime();
        let loaded = load_bytecode(&mut runtime, "threaded-native", bytecode.clone()).unwrap();
        let _loaded_program = loaded;
        let loaded = runtime.select_program(&_loaded_program).unwrap();
        // SAFETY: this product was compiled from this exact bytecode's MIR;
        // the finalized owner retains immutable pages through invocation.
        let installed =
            unsafe { runtime.install_native_function(&loaded, product, RetentionOrigin::Current) }
                .unwrap();
        assert_eq!(
            runtime.invoke_native_function(&installed).unwrap().as_i32(),
            Some(42)
        );
        thread::spawn(move || {
            let baseline = runtime.gc_stats().root_registrations;
            assert_eq!(
                runtime.invoke_native_function(&installed).unwrap().as_i32(),
                Some(42)
            );
            assert_eq!(runtime.gc_stats().root_registrations, baseline);
            drop(runtime);
            drop(installed);
        })
    };
    let first = spawn(product);
    let second = spawn(second);
    first.join().unwrap();
    second.join().unwrap();
    assert!(
        owner.upgrade().is_none(),
        "last receiving thread releases the code owner"
    );
}

#[test]
fn cranelift_backend_compiles_scalar_mir_and_products_outlive_the_backend() {
    let mut backend = CraneliftBackend::for_host().unwrap();
    let mut products = Vec::new();
    for (body, result) in [
        ("40 + 2", RootedValue::from(42)),
        ("!false", RootedValue::from(true)),
        ("-42", RootedValue::from(-42)),
        ("40 < 42", RootedValue::from(true)),
        ("true == false", RootedValue::from(false)),
    ] {
        let ty = if result.as_bool().is_some() {
            "bool"
        } else {
            "i32"
        };
        let mir = mir(&format!("fn main() -> {ty} {{ {body} }}"));
        let code = compile(&mut backend, &mir).unwrap();
        assert!(
            matches!(code.artifact.code.entry, ExecutableEntryPoint::Native { address, .. } if address != 0)
        );
        let expected_points = root(&mir).functions[0].blocks[0].instructions.len() + 1;
        assert_eq!(code.artifact.safepoints.len(), expected_points);
        for (offset, point) in code.artifact.safepoints.iter().enumerate() {
            assert_eq!(point.instruction_offset, offset);
            assert_eq!(
                point.kind,
                ExecutableSafepointKind::RuntimeHelperCall {
                    helper: JIT_POLL_EXECUTION_SYMBOL.into()
                }
            );
            assert!(point.stack_map.live_slots.is_empty());
        }
        products.push((mir, Arc::new(code), result, expected_points));
    }
    // Later compilations and backend destruction cannot retire earlier products.
    drop(backend);
    for (mir, code, expected, _points) in products {
        let mut runtime = runtime();
        let module = load_bytecode(
            &mut runtime,
            "native",
            lower_program_to_bytecode(&mir, &Default::default())
                .map(|program| program.into_unverified())
                .unwrap(),
        )
        .unwrap();
        let _module_program = module;
        let module = runtime.select_program(&_module_program).unwrap();
        let installed = unsafe {
            runtime.install_native_function(&module, code.clone(), RetentionOrigin::Current)
        }
        .unwrap();
        drop(code);
        let baseline = runtime.gc_stats().root_registrations;
        let actual = runtime.invoke_native_function(&installed).unwrap();
        assert_eq!(actual.as_i32(), expected.as_i32());
        assert_eq!(actual.as_bool(), expected.as_bool());

        assert_eq!(runtime.gc_stats().root_registrations, baseline);
    }
}

#[rstest]
#[case::branch("fn main() -> i32 { if true { 1 } else { 2 } }")]
#[case::local("fn main() -> i32 { val x = 1; x }")]
#[case::division("fn main() -> i32 { 42 / 2 }")]
#[case::char_literal("fn main() -> char { '馃檪' }")]
#[case::char_comparison("fn main() -> bool { 'a' == 'z' }")]
#[case::reference_parameter("struct Player {} fn main(value: Ref<Player>) -> i32 { 42 }")]
#[case::static_read("static val value: i32 = 7; fn main() -> i32 { value }")]
#[case::static_write("static var value: i32 = 7; fn main() -> i32 { value = 9; value }")]
fn cranelift_backend_reports_unsupported_instructions_before_native_entry(#[case] source: &str) {
    let mut backend = CraneliftBackend::for_host().unwrap();
    let error = compile(&mut backend, &mir(source)).unwrap_err();
    assert!(error.is_unsupported(), "{error:?}");
    // A failed compile cannot poison subsequent compilation.
    compile(&mut backend, &mir("fn main() -> i32 { 42 }")).unwrap();
}

#[test]
fn cranelift_backend_requires_precise_stack_maps_for_gc_values() {
    let error = compile(
        &mut CraneliftBackend::for_host().unwrap(),
        &mir("fn main() -> [i32] { [1] }"),
    )
    .unwrap_err();
    assert!(error.is_unsupported());
    assert!(error.diagnostics[0].message.contains("stack maps"));
}

#[rstest]
#[case::missing_helper(|links: &mut NativeLinkDescription| links.helpers.clear())]
#[case::unknown_symbol(|links: &mut NativeLinkDescription| links.helpers[0].symbol = "unknown".into())]
#[case::wrong_parameters(|links: &mut NativeLinkDescription| links.helpers[0].parameters.clear())]
#[case::duplicate_helper(
    |links: &mut NativeLinkDescription| links.helpers.push(links.helpers[0].clone())
)]
fn malformed_helper_links_fail_compilation_without_becoming_fallback(
    #[case] corrupt: fn(&mut NativeLinkDescription),
) {
    let mir = mir("fn main() -> i32 { 42 }");
    let mut backend = CraneliftBackend::for_host().unwrap();
    let mut links = NativeLinkDescription::scalar();
    corrupt(&mut links);
    let error = backend
        .compile_function(
            BackendFunctionInput::new(root(&mir), root(&mir).functions[0].id, &links).unwrap(),
        )
        .unwrap_err();
    assert!(!error.is_unsupported());
}

#[rstest]
#[case::add_overflow("2147483647 + 1")]
#[case::subtract_overflow("(-2147483647 - 1) - 1")]
#[case::multiply_overflow("50000 * 50000")]
#[case::negate_minimum("-(-2147483647 - 1)")]
fn checked_i32_traps_keep_the_exact_mir_point(#[case] expression: &str) {
    let mir = mir(&format!("fn main() -> i32 {{ {expression} }}"));
    let code = Arc::new(compile(&mut CraneliftBackend::for_host().unwrap(), &mir).unwrap());
    let overflow_offset = code.artifact.code.traps.last().unwrap().instruction_offset;
    {
        let mut runtime = runtime();
        let bytecode = lower_program_to_bytecode(&mir, &Default::default())
            .map(|program| program.into_unverified())
            .unwrap();
        let function = &bytecode.records().modules[bytecode.records().root.index()].functions[0];
        let origin = function.metadata.origins[overflow_offset].clone();
        let module_program = load_bytecode(&mut runtime, "native", bytecode).unwrap();
        let module = runtime.select_program(&module_program).unwrap();
        let installed = unsafe {
            runtime.install_native_function(&module, code.clone(), RetentionOrigin::Current)
        }
        .unwrap();
        let baseline = runtime.gc_stats().root_registrations;
        let failure = runtime.invoke_native_function(&installed).unwrap_err();
        let BackendInvocationError::RuntimeFailure(error) = failure.error else {
            panic!("runtime trap")
        };
        assert_eq!(error.kind(), RuntimeErrorKind::ScriptTrap, "{expression}");
        assert_eq!(
            failure.trace.frames[0].semantic_origin,
            Some(overflow_offset)
        );
        assert_eq!(
            failure.trace.frames[0].instruction_offset,
            origin.instruction
        );
        assert_eq!(failure.trace.frames[0].source_span, Some(origin.span));
        assert_eq!(runtime.resources().counters().current_call_depth, 0);
        assert_eq!(runtime.gc_stats().root_registrations, baseline);
    }
}
