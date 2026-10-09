//! Physical transfers derived from the caller and callee's sealed frame layouts.
use crate::module::execution::{
    ExecutionModule,
    layout::{FrameLayout, Location},
};
use kagari_bytecode::{
    instruction::{BytecodeInstruction, CallTarget},
    module::BytecodeModule,
    program::ModuleRef,
};
use kagari_common::identity::table::DefinitionId;
use kagari_contract::ids::FunctionRef;
use std::sync::Arc;

#[derive(Debug, Clone, Copy)]
pub(crate) struct ArgumentTransfer {
    pub source: Location,
    pub target: Location,
}

#[derive(Debug)]
pub(crate) struct PreparedScriptCall {
    pub module: ModuleRef,
    pub function: FunctionRef,
    pub registers: Arc<FrameLayout>,
    pub arguments: Box<[ArgumentTransfer]>,
    pub destination: Option<Location>,
}

impl ExecutionModule {
    /// Module slots refer to the containing verified program. Activation binds
    /// them through the caller's pinned descriptor, never the latest publication.
    pub(crate) fn prepare_calls(
        &mut self,
        slot: ModuleRef,
        module: &BytecodeModule<DefinitionId>,
        layouts: &[Vec<Arc<FrameLayout>>],
    ) {
        for (prepared, function) in self.functions.iter_mut().zip(&module.functions) {
            for (pc, instruction) in function.instructions.iter().enumerate() {
                let BytecodeInstruction::Call { dst, callee, args } = instruction else {
                    continue;
                };
                let (module, function) = match *callee {
                    CallTarget::Function(function) => (slot, function),
                    CallTarget::ModuleFunction { module, function } => (module, function),
                    _ => continue,
                };
                let registers = layouts[module.index()][function.index()].clone();
                let arguments = args
                    .iter()
                    .enumerate()
                    .map(|(index, register)| ArgumentTransfer {
                        source: prepared
                            .registers
                            .location(register.index())
                            .expect("sealed caller operand"),
                        target: registers
                            .location(registers.register_count + index)
                            .expect("sealed callee parameter"),
                    })
                    .collect();
                let destination = dst.map(|register| {
                    prepared
                        .registers
                        .location(register.index())
                        .expect("sealed return operand")
                });
                prepared.calls.insert(
                    pc,
                    PreparedScriptCall {
                        module,
                        function,
                        registers,
                        arguments,
                        destination,
                    },
                );
            }
        }
    }
}
