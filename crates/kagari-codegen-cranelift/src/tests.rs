use super::*;
use kagari_compiler::{
    bytecode::lower_program_to_bytecode, native_links::build_native_links,
    source::program::lower_program_to_mir,
};
use kagari_embed::engine::KagariEngine;
use kagari_mir::{program::VerifiedMirProgram, verify::VerifiedMirModule};
use kagari_runtime::{
    Runtime, RuntimeConfig,
    backend::{BackendInvocationError, native::InstalledNativeFunction},
    error::RuntimeErrorKind,
    jit_abi::native_helper_symbols,
    native::module::NativeModule,
    resource::RuntimeLimits,
    value::Value,
};
use kagari_source::source::SourceFile;
use kagari_stdlib as stdlib;
use std::{sync::Arc, thread};
use {
    kagari_abi::{native::ExecutableEntryPoint, native_call::JIT_POLL_EXECUTION_SYMBOL},
    kagari_contract::native::ExecutableSafepointKind,
};

fn mir(source: &str) -> VerifiedMirProgram {
    let checked = KagariEngine::default()
        .compile_source(SourceFile::new("native.kgr", source))
        .unwrap();
    lower_program_to_mir(checked.program(), &Default::default()).unwrap()
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
    let links = build_native_links(&native_helper_symbols()).unwrap();
    let module = root(mir);
    backend.compile_function(
        BackendFunctionInput::new(module, module.functions[0].id, &links).unwrap(),
    )
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
    let bytecode = lower_program_to_bytecode(&mir).unwrap();
    let mut backend = CraneliftBackend::for_host().unwrap();
    let product = Arc::new(compile(&mut backend, &mir).unwrap());
    let owner = Arc::downgrade(&product.owner);
    let second = product.clone();
    drop((backend, mir));
    let spawn = |product: Arc<NativeCompilationProduct>| {
        let mut runtime = runtime();
        let loaded = runtime
            .load_program("threaded-native", bytecode.clone())
            .unwrap();
        // SAFETY: this product was compiled from this exact bytecode's MIR;
        // the finalized owner retains immutable pages through invocation.
        let installed = unsafe { runtime.install_native_function(&loaded, product) }.unwrap();
        assert_eq!(
            runtime.invoke_native_function(&installed).unwrap(),
            Value::I32(42)
        );
        thread::spawn(move || {
            assert_eq!(
                runtime.invoke_native_function(&installed).unwrap(),
                Value::I32(42)
            );
            assert_eq!(runtime.gc().active_roots(), 0);
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
        ("40 + 2", Value::I32(42)),
        ("!false", Value::Bool(true)),
        ("-42", Value::I32(-42)),
        ("40 < 42", Value::Bool(true)),
        ("true == false", Value::Bool(false)),
    ] {
        let ty = if matches!(result, Value::Bool(_)) {
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
        let module = runtime
            .load_program("native", lower_program_to_bytecode(&mir).unwrap())
            .unwrap();
        let installed = unsafe { runtime.install_native_function(&module, code.clone()) }.unwrap();
        drop(code);
        assert_eq!(
            runtime.invoke_native_function(&installed).unwrap(),
            expected
        );

        assert_eq!(runtime.gc().active_roots(), 0);
    }
}

#[test]
fn cranelift_backend_reports_unsupported_instructions_before_native_entry() {
    let mut backend = CraneliftBackend::for_host().unwrap();
    for source in [
        "fn main() -> i32 { if true { 1 } else { 2 } }",
        "fn main() -> i32 { val x = 1; x }",
        "fn main() -> i32 { 42 / 2 }",
    ] {
        let error = compile(&mut backend, &mir(source)).unwrap_err();
        assert!(error.is_unsupported(), "{error:?}");
    }
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

#[test]
fn malformed_helper_links_fail_compilation_without_becoming_fallback() {
    let mir = mir("fn main() -> i32 { 42 }");
    let mut backend = CraneliftBackend::for_host().unwrap();
    let valid = build_native_links(&native_helper_symbols()).unwrap();
    for kind in 0..4 {
        let mut links = valid.clone();
        match kind {
            0 => links.helpers.clear(),
            1 => links.helpers[0].address = 0,
            2 => links.helpers[0].parameters.clear(),
            _ => links.helpers.push(links.helpers[0].clone()),
        }
        let error = backend
            .compile_function(
                BackendFunctionInput::new(root(&mir), root(&mir).functions[0].id, &links).unwrap(),
            )
            .unwrap_err();
        assert!(!error.is_unsupported());
    }
}

#[test]
fn checked_i32_traps_keep_the_exact_mir_point() {
    for expression in [
        "2147483647 + 1",
        "(-2147483647 - 1) - 1",
        "50000 * 50000",
        "-(-2147483647 - 1)",
    ] {
        let mir = mir(&format!("fn main() -> i32 {{ {expression} }}"));
        let code = Arc::new(compile(&mut CraneliftBackend::for_host().unwrap(), &mir).unwrap());
        let overflow_offset = code.artifact.code.traps.last().unwrap().instruction_offset;
        {
            let mut runtime = runtime();
            let module = runtime
                .load_program("native", lower_program_to_bytecode(&mir).unwrap())
                .unwrap();
            let installed =
                unsafe { runtime.install_native_function(&module, code.clone()) }.unwrap();
            let failure = runtime.invoke_native_function(&installed).unwrap_err();
            let BackendInvocationError::RuntimeFailure(error) = failure.error else {
                panic!("runtime trap")
            };
            assert_eq!(error.kind(), RuntimeErrorKind::ScriptTrap, "{expression}");
            assert_eq!(failure.trace.frames[0].instruction_offset, overflow_offset);
            assert_eq!(runtime.resources().counters().current_call_depth, 0);
            assert_eq!(runtime.gc().active_roots(), 0);
        }
    }
}
