use crate::bytecode::instruction::{BytecodeInstruction, FunctionRef};

#[derive(Debug, Clone, Default)]
pub struct BytecodeModule {
    pub functions: BytecodeFunctionBuffer,
}

#[derive(Debug, Clone)]
pub struct BytecodeFunction {
    pub id: FunctionRef,
    pub name: String,
    pub register_count: u16,
    pub local_count: u16,
    pub instructions: BytecodeInstructionBuffer,
}

pub type BytecodeFunctionBuffer = Vec<BytecodeFunction>;
pub type BytecodeInstructionBuffer = Vec<BytecodeInstruction>;
