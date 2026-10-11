use crate::{
    debug::MirFunctionDebugMetadata,
    ids::{BlockId, InstanceId, LocalId, ModuleSlotId, TempId},
    instruction::{CallTarget, Instruction, InstructionBuffer, Terminator},
};
use kagari_abi::representation::ValueType;
use kagari_common::{identity::ModuleIdentity, span::Span};
use kagari_contract::{
    callable::witness::OperationWitness,
    effects::EffectSet,
    layout::{EnumLayout, StructLayout},
    native_import::{NativeImport, callables::NativeCallableApplication},
    slots::SemanticSlots,
    types::{ConcreteFunctionIdentity, ModuleContract},
};
use kagari_types::{host_interface::type_declaration::HostTypeDeclaration, ty::NominalTy};
use serde::{Deserialize, Serialize};
use std::{borrow::Cow, iter};

/// Editable MIR facts whose references belong to an enclosing definition scope.
///
/// Use scoped metadata for adoption and the MIR codec for portable bytes. This
/// record contains no executable verification evidence or identity representation choice.
#[derive(Debug, Clone)]
pub struct MirModule {
    /// Concrete native entry contracts invoked without a script body, including
    /// interface slots and dependencies selected by another native entry.
    pub native_targets: Vec<NativeImport>,
    /// Concrete interface demands, including inherited views that need no
    /// source allocation instruction of their own.
    pub interface_instances: Vec<ConcreteFunctionIdentity>,
    pub host_types: Vec<HostTypeDeclaration>,
    pub dependencies: Vec<ModuleIdentity>,
    pub structures: Vec<StructLayout>,
    pub enumerations: Vec<EnumLayout>,
    pub identity: ModuleIdentity,
    pub source_name: String,
    pub module_slots: ModuleSlotBuffer,
    pub abi: ModuleContract,
    pub functions: FunctionBuffer,
}

#[derive(Debug, Clone)]
pub struct MirFunction {
    pub semantic: SemanticSlots,
    pub id: InstanceId,
    pub instance: ConcreteFunctionIdentity,
    pub name: String,
    pub params: ParameterBuffer,
    pub return_type: ValueType,
    pub locals: LocalBuffer,
    pub temps: TempBuffer,
    pub blocks: BlockBuffer,
    pub entry: BlockId,
    pub effects: EffectSet,
    pub debug: MirFunctionDebugMetadata,
}

impl MirFunction {
    /// Canonical logical point order: entry block first, then block ID order.
    /// Requires the valid entry guaranteed by MIR verification.
    pub fn emission_order(&self) -> impl Iterator<Item = (usize, &BasicBlock)> {
        iter::once((self.entry.index(), &self.blocks[self.entry.index()])).chain(
            self.blocks
                .iter()
                .enumerate()
                .filter(|(index, _)| *index != self.entry.index()),
        )
    }

    /// Conservative GC roots for the entire function lifetime. Slot liveness
    /// can narrow these sets later without changing the value representation.
    pub fn root_slots(&self) -> (Vec<LocalId>, Vec<TempId>) {
        let locals = self
            .locals
            .iter()
            .enumerate()
            .filter(|(_, local)| local.ty.may_contain_gc_reference())
            .map(|(index, _)| LocalId::new(index))
            .collect();
        let temps = self
            .temps
            .iter()
            .enumerate()
            .filter(|(_, temp)| temp.ty.may_contain_gc_reference())
            .map(|(index, _)| TempId::new(index))
            .collect();
        (locals, temps)
    }
}

impl MirModule {
    pub fn selected_callables(&self) -> impl Iterator<Item = &NativeCallableApplication> {
        self.native_applications()
            .flat_map(|import| &import.callables)
            .filter_map(|operation| match operation {
                OperationWitness::Selected(selected) => Some(selected.as_ref()),
                OperationWitness::Forward(_) | OperationWitness::SharedMethod(_) => None,
            })
            .chain(
                self.functions
                    .iter()
                    .flat_map(|function| &function.blocks)
                    .flat_map(|block| &block.instructions)
                    .filter_map(|instruction| match instruction {
                        Instruction::Call {
                            callee: CallTarget::Shared(contract),
                            ..
                        } => Some(&contract.operations),
                        Instruction::Call {
                            callee: CallTarget::InterfaceMethod(contract),
                            ..
                        } => Some(&contract.operations),
                        _ => None,
                    })
                    .flatten()
                    .filter_map(|operation| match operation {
                        OperationWitness::Selected(selected) => Some(selected.as_ref()),
                        OperationWitness::Forward(_) | OperationWitness::SharedMethod(_) => None,
                    }),
            )
    }

    pub fn native_applications(&self) -> impl Iterator<Item = &NativeImport> {
        self.native_targets.iter().chain(
            self.functions
                .iter()
                .flat_map(|function| &function.blocks)
                .flat_map(|block| &block.instructions)
                .filter_map(|instruction| match instruction {
                    Instruction::Call {
                        callee: CallTarget::Native(import),
                        ..
                    } => Some(import.as_ref()),
                    _ => None,
                }),
        )
    }
}

impl MirModule {
    pub fn structure(&self, instance: &NominalTy) -> Option<Cow<'_, StructLayout>> {
        self.structures
            .iter()
            .find(|layout| {
                layout.declaration == instance.declaration && layout.accepts(&instance.arguments)
            })
            .and_then(|layout| layout.apply(&instance.arguments, &Default::default()))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MirParameter {
    pub name: String,
    pub ty: ValueType,
    pub local: LocalId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MirLocal {
    pub name: String,
    pub ty: ValueType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MirTemp {
    pub ty: ValueType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MirModuleSlot {
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

pub type FunctionBuffer = Vec<MirFunction>;

pub type ParameterBuffer = Vec<MirParameter>;

pub type LocalBuffer = Vec<MirLocal>;

pub type ModuleSlotBuffer = Vec<MirModuleSlot>;

pub type TempBuffer = Vec<MirTemp>;

pub type BlockBuffer = Vec<BasicBlock>;

pub type SourceSpanBuffer = Vec<Span>;

mod mapping;
