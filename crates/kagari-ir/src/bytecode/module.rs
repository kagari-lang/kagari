use crate::{
    bytecode::instruction::{
        BytecodeInstruction, ConstantOperand, FieldId, FunctionRef, JumpTarget, PathId,
    },
    module::{EffectSet, ValueType},
};

#[derive(Debug, Clone, Default)]
pub struct BytecodeModule {
    pub module_init: Option<FunctionRef>,
    pub module_slots: BytecodeModuleSlotBuffer,
    pub constants: ConstantPool,
    pub types: BytecodeTypeTable,
    pub fields: FieldTable,
    pub paths: PathTable,
    pub function_table: FunctionTable,
    pub public_items: PublicItemTable,
    pub functions: BytecodeFunctionBuffer,
}

#[derive(Debug, Clone)]
pub struct BytecodeModuleSlot {
    pub name: String,
    pub ty: ValueType,
    pub mutable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldRecord {
    pub id: FieldId,
    pub owner: String,
    pub name: String,
    pub ty: ValueType,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathRecord {
    pub id: PathId,
    pub root_ty: ValueType,
    pub result_ty: ValueType,
    pub read_only: bool,
    pub debug_name: String,
}

#[derive(Debug, Clone, Default)]
pub struct BytecodeFunction {
    pub id: FunctionRef,
    pub name: String,
    pub parameter_count: u16,
    pub register_count: u16,
    pub local_count: u16,
    pub metadata: FunctionMetadata,
    pub instructions: BytecodeInstructionBuffer,
}

#[derive(Debug, Clone, Default)]
pub struct FunctionMetadata {
    pub params: TypeLayoutBuffer,
    pub return_type: ValueType,
    pub locals: TypeLayoutBuffer,
    pub registers: TypeLayoutBuffer,
    pub control_flow_targets: ControlFlowTargetBuffer,
    pub effects: EffectSet,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionRecord {
    pub id: FunctionRef,
    pub name: String,
    pub params: TypeLayoutBuffer,
    pub return_type: ValueType,
    pub effects: EffectSet,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PublicItemRecord {
    Function { name: String, function: FunctionRef },
}

pub type BytecodeFunctionBuffer = Vec<BytecodeFunction>;
pub type BytecodeInstructionBuffer = Vec<BytecodeInstruction>;
pub type BytecodeModuleSlotBuffer = Vec<BytecodeModuleSlot>;
pub type ConstantPool = Vec<ConstantOperand>;
pub type BytecodeTypeTable = Vec<ValueType>;
pub type FieldTable = Vec<FieldRecord>;
pub type PathTable = Vec<PathRecord>;
pub type FunctionTable = Vec<FunctionRecord>;
pub type PublicItemTable = Vec<PublicItemRecord>;
pub type TypeLayoutBuffer = Vec<ValueType>;
pub type ControlFlowTargetBuffer = Vec<JumpTarget>;
