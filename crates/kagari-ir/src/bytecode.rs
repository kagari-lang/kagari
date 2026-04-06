#[derive(Debug, Clone)]
pub struct BytecodeModule {
    pub functions: FunctionBuffer,
}

#[derive(Debug, Clone)]
pub struct BytecodeFunction {
    pub name: String,
    pub instructions: InstructionBuffer,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BytecodeInstruction {
    Return,
    LoadLocal(u16),
    CallHost { symbol: String, argc: u8 },
}

pub type FunctionBuffer = Vec<BytecodeFunction>;
pub type InstructionBuffer = Vec<BytecodeInstruction>;
