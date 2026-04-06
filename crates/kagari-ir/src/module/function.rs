use kagari_hir::hir;

use crate::module::{
    ids::{BlockId, LocalId, ModuleSlotId},
    instruction::{InstructionBuffer, Terminator},
    types::ValueType,
};

#[derive(Debug, Clone)]
pub struct IrModule {
    pub module_init: Option<hir::FunctionId>,
    pub module_slots: ModuleSlotBuffer,
    pub functions: FunctionBuffer,
}

#[derive(Debug, Clone)]
pub struct IrFunction {
    pub hir_id: hir::FunctionId,
    pub name: String,
    pub params: ParameterBuffer,
    pub return_type: ValueType,
    pub locals: LocalBuffer,
    pub temps: TempBuffer,
    pub blocks: BlockBuffer,
    pub entry: BlockId,
}

#[derive(Debug, Clone)]
pub struct IrParameter {
    pub name: String,
    pub ty: ValueType,
    pub local: LocalId,
}

#[derive(Debug, Clone)]
pub struct IrLocal {
    pub name: String,
    pub ty: ValueType,
}

#[derive(Debug, Clone)]
pub struct IrTemp {
    pub ty: ValueType,
}

#[derive(Debug, Clone)]
pub struct IrModuleSlot {
    pub id: ModuleSlotId,
    pub name: String,
    pub ty: ValueType,
    pub mutable: bool,
}

#[derive(Debug, Clone)]
pub struct BasicBlock {
    pub instructions: InstructionBuffer,
    pub terminator: Option<Terminator>,
}

pub type FunctionBuffer = Vec<IrFunction>;
pub type ParameterBuffer = Vec<IrParameter>;
pub type LocalBuffer = Vec<IrLocal>;
pub type ModuleSlotBuffer = Vec<IrModuleSlot>;
pub type TempBuffer = Vec<IrTemp>;
pub type BlockBuffer = Vec<BasicBlock>;
