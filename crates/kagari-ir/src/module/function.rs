use kagari_common::Span;

use crate::module::{
    ModuleAbi,
    ids::{BlockId, InstanceId, LocalId, ModuleSlotId},
    instruction::{EffectSet, InstructionBuffer, Terminator},
    types::ValueType,
};

#[derive(Debug, Clone)]
pub struct IrModule {
    pub host_types: Vec<kagari_common::host_interface::HostTypeDeclaration>,
    pub dependencies: Vec<kagari_common::identity::ModuleIdentity>,
    pub structures: Vec<super::StructLayout>,
    pub enumerations: Vec<super::EnumLayout>,
    pub identity: kagari_common::identity::ModuleIdentity,
    pub source_name: String,
    pub module_init: Option<InstanceId>,
    pub module_slots: ModuleSlotBuffer,
    pub abi: ModuleAbi,
    pub functions: FunctionBuffer,
}

#[derive(Debug, Clone)]
pub struct IrFunction {
    pub id: InstanceId,
    pub instance: FunctionInstance,
    pub name: String,
    pub params: ParameterBuffer,
    pub return_type: ValueType,
    pub locals: LocalBuffer,
    pub temps: TempBuffer,
    pub blocks: BlockBuffer,
    pub entry: BlockId,
    pub effects: EffectSet,
    pub debug: IrFunctionDebugMetadata,
}

impl IrModule {
    pub fn structure(&self, instance: &super::abi::NominalAbiType) -> Option<&super::StructLayout> {
        self.structures.iter().find(|layout| {
            layout.declaration == instance.declaration && layout.arguments == instance.arguments
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FunctionInstance {
    pub declaration: kagari_common::identity::DefinitionId,
    pub arguments: Vec<kagari_hir::types::TypeId>,
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
    pub instruction_spans: SourceSpanBuffer,
    pub terminator: Option<Terminator>,
    pub terminator_span: Option<Span>,
}

#[derive(Debug, Clone, Default)]
pub struct IrFunctionDebugMetadata {
    pub source_span: Span,
    pub locals: IrLocalDebugBuffer,
    pub captured_bindings: CapturedBindingDebugBuffer,
}

#[derive(Debug, Clone)]
pub struct IrLocalDebugInfo {
    pub local: LocalId,
    pub name: String,
    pub span: Span,
    pub ty: ValueType,
    pub is_parameter: bool,
}

#[derive(Debug, Clone)]
pub struct IrCapturedBindingDebugInfo {
    pub name: String,
    pub span: Span,
    pub ty: ValueType,
}

pub type FunctionBuffer = Vec<IrFunction>;
pub type ParameterBuffer = Vec<IrParameter>;
pub type LocalBuffer = Vec<IrLocal>;
pub type ModuleSlotBuffer = Vec<IrModuleSlot>;
pub type TempBuffer = Vec<IrTemp>;
pub type BlockBuffer = Vec<BasicBlock>;
pub type SourceSpanBuffer = Vec<Span>;
pub type IrLocalDebugBuffer = Vec<IrLocalDebugInfo>;
pub type CapturedBindingDebugBuffer = Vec<IrCapturedBindingDebugInfo>;
