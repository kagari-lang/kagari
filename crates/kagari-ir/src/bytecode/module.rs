use crate::bytecode::instruction::{BytecodeInstruction, FunctionRef};

#[derive(Debug, Clone, Default)]
pub struct BytecodeModule {
    pub module_init: Option<FunctionRef>,
    pub module_slots: BytecodeModuleSlotBuffer,
    pub functions: BytecodeFunctionBuffer,
}

#[derive(Debug, Clone)]
pub struct BytecodeModuleSlot {
    pub name: String,
    pub mutable: bool,
}

#[derive(Debug, Clone)]
pub struct BytecodeFunction {
    pub id: FunctionRef,
    pub name: String,
    pub parameter_count: u16,
    pub register_count: u16,
    pub local_count: u16,
    pub instructions: BytecodeInstructionBuffer,
}

pub type BytecodeFunctionBuffer = Vec<BytecodeFunction>;
pub type BytecodeInstructionBuffer = Vec<BytecodeInstruction>;
pub type BytecodeModuleSlotBuffer = Vec<BytecodeModuleSlot>;
