use kagari_common::Span;

use crate::module::{
    ModuleAbi,
    ids::{BlockId, InstanceId, LocalId, ModuleSlotId, TempId},
    instruction::{EffectSet, InstructionBuffer, Terminator},
    types::ValueType,
};

#[derive(Debug, Clone)]
pub struct IrModule {
    /// Concrete interface demands, including inherited views that need no
    /// source allocation instruction of their own.
    pub interface_instances: Vec<FunctionInstance>,
    pub host_types: Vec<kagari_common::host_interface::HostTypeDeclaration>,
    pub dependencies: Vec<kagari_common::identity::ModuleIdentity>,
    pub structures: Vec<super::StructLayout>,
    pub enumerations: Vec<super::EnumLayout>,
    pub identity: kagari_common::identity::ModuleIdentity,
    pub source_name: String,
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

impl IrFunction {
    /// Conservative GC roots for the entire function lifetime. Slot liveness
    /// can narrow these sets later without changing the value representation.
    pub fn root_slots(&self) -> (Vec<LocalId>, Vec<TempId>) {
        let locals = self
            .locals
            .iter()
            .enumerate()
            .filter(|(_, local)| local.ty == ValueType::HeapObject)
            .map(|(index, _)| LocalId::new(index))
            .collect();
        let temps = self
            .temps
            .iter()
            .enumerate()
            .filter(|(_, temp)| temp.ty == ValueType::HeapObject)
            .map(|(index, _)| TempId::new(index))
            .collect();
        (locals, temps)
    }
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
    pub instruction_scopes: Vec<usize>,
    pub terminator: Option<Terminator>,
    pub terminator_span: Option<Span>,
    pub terminator_scope: Option<usize>,
}

#[derive(Debug, Clone, Default)]
pub struct IrFunctionDebugMetadata {
    pub source: Option<std::sync::Arc<kagari_common::SourceFile>>,
    pub source_module: Option<kagari_common::identity::ModuleIdentity>,
    pub source_span: Span,
    pub locals: IrLocalDebugBuffer,
    pub captured_bindings: CapturedBindingDebugBuffer,
    pub lexical_scopes: Vec<IrLexicalScope>,
}

#[derive(Debug, Clone)]
pub struct IrLexicalScope {
    pub parent: Option<usize>,
    pub local: Option<LocalId>,
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
