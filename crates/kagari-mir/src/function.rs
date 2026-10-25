use crate::{
    debug::MirFunctionDebugMetadata,
    ids::{BlockId, InstanceId, LocalId, TempId},
    instruction::{CallTarget, ConstructionTarget, Instruction, InstructionBuffer, Terminator},
};
use kagari_abi::representation::ValueType;
use kagari_common::{identity::ModuleIdentity, span::Span};

use kagari_contract::{
    callable::{evidence::EvidenceRef, witness::OperationWitness},
    effects::EffectSet,
    layout::{EnumLayout, StructLayout},
    native_import::{NativeImport, callables::NativeCallableApplication},
    slots::SemanticSlots,
    statics::StaticDescriptor,
    types::{ConcreteFunctionIdentity, ModuleContract},
};
use kagari_types::{declaration::TypeDeclaration, ty::NominalTy};
use serde::{Deserialize, Serialize};
use std::{borrow::Cow, iter};

pub mod uses;

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
    pub external_types: Vec<TypeDeclaration>,
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
        self.operation_witnesses()
            .filter_map(|operation| match operation {
                OperationWitness::Selected(selected) => Some(selected.as_ref()),
                _ => None,
            })
    }

    /// Include nested structural operations in target verification and reachability.
    pub fn operation_witnesses(&self) -> impl Iterator<Item = &OperationWitness> {
        self.native_applications()
            .flat_map(|import| import.callables.iter().map(EvidenceRef::Operation))
            .flat_map(EvidenceRef::operations)
            .chain(
                self.functions
                    .iter()
                    .flat_map(|function| &function.blocks)
                    .flat_map(|block| &block.instructions)
                    .filter(|instruction| {
                        !matches!(
                            instruction,
                            Instruction::Call {
                                callee: CallTarget::Native(_),
                                ..
                            }
                        )
                    })
                    .flat_map(Instruction::operation_witnesses),
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

impl Instruction {
    /// Walk every selected operation, including nested construction evidence.
    /// This is shared by whole-module verification and per-body use discovery.
    pub fn operation_witnesses(&self) -> impl Iterator<Item = &OperationWitness> {
        let (operations, constructions) = match self {
            Self::Call {
                callee: CallTarget::Native(import),
                ..
            } => (import.callables.as_slice(), [].as_slice()),
            Self::Call {
                callee: CallTarget::Shared(call),
                ..
            } => (call.operations.as_slice(), call.constructions.as_slice()),
            Self::Call {
                callee: CallTarget::InterfaceMethod(call),
                ..
            } => (call.operations.as_slice(), call.constructions.as_slice()),
            Self::MakeInterface {
                target:
                    ConstructionTarget::Declared {
                        operations,
                        constructions,
                        ..
                    },
                ..
            } => (operations.as_slice(), constructions.as_slice()),
            _ => ([].as_slice(), [].as_slice()),
        };
        operations
            .iter()
            .map(EvidenceRef::Operation)
            .chain(constructions.iter().map(EvidenceRef::Construction))
            .flat_map(EvidenceRef::operations)
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

pub type ModuleSlotBuffer = Vec<StaticDescriptor<InstanceId>>;

pub type TempBuffer = Vec<MirTemp>;

pub type BlockBuffer = Vec<BasicBlock>;

pub type SourceSpanBuffer = Vec<Span>;

mod mapping;
