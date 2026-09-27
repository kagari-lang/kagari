use super::*;
use kagari_abi::native::{ExecutableEntryPoint, ExecutableSafepointKind};
use kagari_abi::native_call::JIT_CONSUME_INSTRUCTION_STEP_SYMBOL;
use kagari_bytecode::{BytecodeProgram, ModuleRef};
use kagari_common::SourceFile;
use kagari_compiler::{
    bytecode::lower_to_bytecode, native_links::build_native_links,
    source::program::lower_program_to_mir,
};
use kagari_embed::KagariEngine;
use kagari_mir::VerifiedMirModule;
use kagari_runtime::{
    BackendInvocationError, CapabilitySet, LanguageProfile, ResourcePolicy, Runtime, RuntimeConfig,
    RuntimeErrorKind, SecurityContext, jit_abi::native_helper_symbols, value::Value,
};
use std::rc::Rc;

fn mir(source: &str) -> VerifiedMirModule {
    let checked = KagariEngine::default()
        .compile_source(SourceFile::new("native.kgr", source), Default::default())
        .unwrap();
    lower_program_to_mir(checked.program(), &Default::default())
        .unwrap()
        .modules()[0]
        .clone()
}
fn runtime(limit: Option<u64>) -> Runtime {
    Runtime::new(RuntimeConfig {
        security: SecurityContext {
            profile: LanguageProfile {
                allow_jit: true,
                ..Default::default()
            },
            capabilities: CapabilitySet {
                jit: true,
                ..Default::default()
            },
        },
        resources: ResourcePolicy {
            max_instruction_steps: limit,
            ..Default::default()
        },
        ..Default::default()
    })
}
fn compile(
    backend: &mut CraneliftBackend,
    mir: &VerifiedMirModule,
) -> Result<NativeCompilationProduct, BackendCompileError> {
    let links = build_native_links(&native_helper_symbols()).unwrap();
    backend.compile_function(BackendFunctionInput::new(mir, mir.functions[0].id, &links).unwrap())
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
            matches!(code.artifact.entry, ExecutableEntryPoint::Native { address, .. } if address != 0)
        );
        let expected_points = mir.functions[0].blocks[0].instructions.len() + 1;
        assert_eq!(code.artifact.safepoints.len(), expected_points);
        for (offset, point) in code.artifact.safepoints.iter().enumerate() {
            assert_eq!(point.instruction_offset, offset);
            assert_eq!(
                point.kind,
                ExecutableSafepointKind::RuntimeHelperCall {
                    helper: JIT_CONSUME_INSTRUCTION_STEP_SYMBOL.into()
                }
            );
            assert!(point.stack_map.live_slots.is_empty());
        }
        products.push((mir, Rc::new(code), result, expected_points));
    }
    // Later compilations and backend destruction cannot retire earlier products.
    drop(backend);
    for (mir, code, expected, points) in products {
        let mut runtime = runtime(None);
        let module = runtime
            .load_program(
                "native",
                BytecodeProgram {
                    root: ModuleRef::new(0),
                    modules: vec![lower_to_bytecode(&mir).unwrap()],
                },
            )
            .unwrap();
        let installed = unsafe { runtime.install_native_function(&module, code.clone()) }.unwrap();
        drop(code);
        assert_eq!(
            runtime.invoke_native_function(&installed).unwrap(),
            expected
        );
        assert_eq!(
            runtime.resources().counters().instruction_steps,
            points as u64
        );
        assert_eq!(runtime.gc().active_roots(), 0);
    }
}

#[test]
fn cranelift_backend_calls_runtime_resource_helper_and_reports_failure() {
    let mir = mir("fn main() -> bool { true }");
    let code = Rc::new(compile(&mut CraneliftBackend::for_host().unwrap(), &mir).unwrap());
    let mut runtime = runtime(Some(1));
    let module = runtime
        .load_program(
            "native",
            BytecodeProgram {
                root: ModuleRef::new(0),
                modules: vec![lower_to_bytecode(&mir).unwrap()],
            },
        )
        .unwrap();
    let installed = unsafe { runtime.install_native_function(&module, code) }.unwrap();
    let failure = runtime.invoke_native_function(&installed).unwrap_err();
    assert!(
        matches!(failure.error, BackendInvocationError::RuntimeFailure(ref error) if error.kind() == RuntimeErrorKind::ResourceLimitExceeded)
    );
    assert!(failure.error.message().contains("instruction steps"));
    assert_eq!(failure.trace.frames[0].instruction_offset, 1);
    assert_eq!(runtime.resources().counters().instruction_steps, 1);
    assert_eq!(runtime.gc().active_roots(), 0);
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
        &mir("fn main() -> ArrayList<i32> { [1] }"),
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
            .compile_function(BackendFunctionInput::new(&mir, mir.functions[0].id, &links).unwrap())
            .unwrap_err();
        assert!(!error.is_unsupported());
    }
}

#[test]
fn checked_i32_traps_and_budget_failures_keep_the_exact_mir_point() {
    for expression in [
        "2147483647 + 1",
        "(-2147483647 - 1) - 1",
        "50000 * 50000",
        "-(-2147483647 - 1)",
    ] {
        let mir = mir(&format!("fn main() -> i32 {{ {expression} }}"));
        let code = Rc::new(compile(&mut CraneliftBackend::for_host().unwrap(), &mir).unwrap());
        let overflow_offset = code.artifact.traps.last().unwrap().instruction_offset;
        for limit in 0..=overflow_offset + 2 {
            let mut runtime = runtime(Some(limit as u64));
            let module = runtime
                .load_program(
                    "native",
                    BytecodeProgram {
                        root: ModuleRef::new(0),
                        modules: vec![lower_to_bytecode(&mir).unwrap()],
                    },
                )
                .unwrap();
            let installed =
                unsafe { runtime.install_native_function(&module, code.clone()) }.unwrap();
            let failure = runtime.invoke_native_function(&installed).unwrap_err();
            let BackendInvocationError::RuntimeFailure(error) = failure.error else {
                panic!("runtime trap")
            };
            let (kind, offset, steps) = if limit <= overflow_offset {
                (RuntimeErrorKind::ResourceLimitExceeded, limit, limit)
            } else {
                (
                    RuntimeErrorKind::ScriptTrap,
                    overflow_offset,
                    overflow_offset + 1,
                )
            };
            assert_eq!(error.kind(), kind, "{expression} at budget {limit}");
            assert_eq!(failure.trace.frames[0].instruction_offset, offset);
            assert_eq!(
                runtime.resources().counters().instruction_steps,
                steps as u64
            );
            assert_eq!(runtime.resources().counters().current_call_depth, 0);
            assert_eq!(runtime.gc().active_roots(), 0);
        }
    }
}
