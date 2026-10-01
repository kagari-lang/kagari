//! Compiler-independent fixtures for VM native-entry and fallback decisions.
use std::{ffi::c_void, rc::Rc};

use kagari_abi::{
    native::{
        BackendId, BackendTarget, ExecutableDebugInfo, ExecutableDebugPoint, ExecutableEntryPoint,
        ExecutableFunctionArtifact, NativeCodeOwner, NativeCompilationProduct,
    },
    native_call::{JIT_STATUS_OK, JitValue},
};
use kagari_bytecode::instruction::{BytecodeInstruction, ConstantOperand, Register};
use kagari_runtime::{
    Runtime, backend::native::InstalledNativeFunction, jit_abi::jit_consume_instruction_step,
    module::LoadedModule,
};

use crate::vm::native::PreparedNativeEntry;

pub(super) fn unsupported() -> PreparedNativeEntry {
    PreparedNativeEntry::Unsupported {
        backend: BackendId::new("test-unsupported-jit"),
        diagnostics: vec!["test preparation cannot compile this function".into()],
    }
}

#[derive(Debug)]
struct StaticCode;
impl NativeCodeOwner for StaticCode {}

unsafe extern "C" fn constant_i32<const VALUE: i32>(
    runtime: *const c_void,
    result: *mut JitValue,
) -> i32 {
    for offset in 0..2 {
        let status = unsafe { jit_consume_instruction_step(runtime.cast(), offset) };
        if status != JIT_STATUS_OK {
            return status;
        }
    }
    unsafe {
        result.write(JitValue::i32(VALUE));
    }
    JIT_STATUS_OK
}

pub(super) fn install_i32<const VALUE: i32>(
    runtime: &Runtime,
    module: &LoadedModule,
    debug_metadata: bool,
) -> InstalledNativeFunction {
    let function = module
        .bytecode
        .functions
        .iter()
        .find(|f| f.name == "main")
        .unwrap();
    assert_eq!(
        function.instructions,
        [
            BytecodeInstruction::LoadConst {
                dst: Register::new(0),
                constant: ConstantOperand::I32(VALUE)
            },
            BytecodeInstruction::Return(Some(Register::new(0))),
        ],
        "the static entry implements only this exact two-instruction function"
    );
    let mut artifact = ExecutableFunctionArtifact::new(
        BackendId::new("test-native-jit"),
        BackendTarget::new("host", usize::BITS as u8),
        function.id,
    );
    artifact.entry = ExecutableEntryPoint::Native {
        symbol: "constant_i32".into(),
        address: constant_i32::<VALUE> as *const () as usize,
    };
    // Deliberately claim complete metadata to prove that flags alone cannot
    // authorize native execution while the runtime has an attached observer.
    if debug_metadata {
        artifact.debug = ExecutableDebugInfo {
            has_line_tables: true,
            has_source_spans: true,
            has_live_value_locations: true,
            has_safe_debug_callbacks: true,
            safe_debug_points: function
                .metadata
                .debug
                .safe_debug_points
                .iter()
                .map(|point| ExecutableDebugPoint {
                    instruction_offset: point.instruction_offset,
                    debug_point: point.id,
                })
                .collect(),
        };
    }
    // SAFETY: the assertion above binds this static C-ABI implementation to the
    // exact bytecode. Each logical point uses the runtime helper for GC, budget,
    // cancellation and traps; static code remains callable for the process life.
    unsafe {
        runtime.install_native_function(
            module,
            Rc::new(NativeCompilationProduct {
                artifact,
                owner: Rc::new(StaticCode),
            }),
        )
    }
    .unwrap()
}
