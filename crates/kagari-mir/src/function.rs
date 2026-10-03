use kagari_abi::{
    callable::witness::OperationWitness,
    effects::EffectSet,
    layout::{EnumLayout, StructLayout},
    native_import::{NativeImport, callables::NativeCallableApplication},
    representation::ValueType,
    slots::SemanticSlots,
    types::{ConcreteFunctionIdentity, ModuleAbi, NominalAbiType},
};
use kagari_common::identity::DefinitionPath;
use kagari_common::identity::reference::DefinitionReference;
use kagari_common::{
    host_interface::type_declaration::HostTypeDeclaration, identity::ModuleIdentity, span::Span,
};
use serde::{Deserialize, Serialize};

use crate::{
    debug::MirFunctionDebugMetadata,
    ids::{BlockId, InstanceId, LocalId, ModuleSlotId, TempId},
    instruction::{CallTarget, Instruction, InstructionBuffer, Terminator},
};
use std::{borrow::Cow, iter};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(bound(
    serialize = "I: DefinitionReference + serde::Serialize",
    deserialize = "I: DefinitionReference + serde::Deserialize<'de>"
))]
pub struct MirModule<I = DefinitionPath> {
    /// Concrete native entry contracts invoked without a script body, including
    /// interface slots and dependencies selected by another native entry.
    #[serde(deserialize_with = "kagari_abi::decode_limits::nested")]
    pub native_targets: Vec<NativeImport<I>>,
    /// Concrete interface demands, including inherited views that need no
    /// source allocation instruction of their own.
    #[serde(deserialize_with = "kagari_abi::decode_limits::nested")]
    pub interface_instances: Vec<ConcreteFunctionIdentity<I>>,
    #[serde(deserialize_with = "kagari_abi::decode_limits::table")]
    pub host_types: Vec<HostTypeDeclaration<I>>,
    #[serde(deserialize_with = "kagari_abi::decode_limits::nested")]
    pub dependencies: Vec<ModuleIdentity>,
    #[serde(deserialize_with = "kagari_abi::decode_limits::table")]
    pub structures: Vec<StructLayout<I>>,
    #[serde(deserialize_with = "kagari_abi::decode_limits::table")]
    pub enumerations: Vec<EnumLayout<I>>,
    pub identity: ModuleIdentity,
    pub source_name: String,
    #[serde(deserialize_with = "kagari_abi::decode_limits::table")]
    pub module_slots: ModuleSlotBuffer,
    pub abi: ModuleAbi<I>,
    #[serde(deserialize_with = "kagari_abi::decode_limits::functions")]
    pub functions: FunctionBuffer<I>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(bound(
    serialize = "I: DefinitionReference + serde::Serialize",
    deserialize = "I: DefinitionReference + serde::Deserialize<'de>"
))]
pub struct MirFunction<I = DefinitionPath> {
    pub semantic: SemanticSlots<I>,
    pub id: InstanceId,
    pub instance: ConcreteFunctionIdentity<I>,
    pub name: String,
    #[serde(deserialize_with = "kagari_abi::decode_limits::nested")]
    pub params: ParameterBuffer,
    pub return_type: ValueType,
    #[serde(deserialize_with = "kagari_abi::decode_limits::table")]
    pub locals: LocalBuffer,
    #[serde(deserialize_with = "kagari_abi::decode_limits::table")]
    pub temps: TempBuffer,
    #[serde(deserialize_with = "kagari_abi::decode_limits::table")]
    pub blocks: BlockBuffer<I>,
    pub entry: BlockId,
    pub effects: EffectSet,
    pub debug: MirFunctionDebugMetadata,
}

impl<I: DefinitionReference> MirFunction<I> {
    /// Canonical logical point order: entry block first, then block ID order.
    /// Requires the valid entry guaranteed by MIR verification.
    pub fn emission_order(&self) -> impl Iterator<Item = (usize, &BasicBlock<I>)> {
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

impl<I: DefinitionReference> MirModule<I> {
    pub fn selected_callables(&self) -> impl Iterator<Item = &NativeCallableApplication<I>> {
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

    pub fn native_applications(&self) -> impl Iterator<Item = &NativeImport<I>> {
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

    pub fn structure(&self, instance: &NominalAbiType<I>) -> Option<Cow<'_, StructLayout<I>>> {
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(bound(
    serialize = "I: DefinitionReference + serde::Serialize",
    deserialize = "I: DefinitionReference + serde::Deserialize<'de>"
))]
pub struct BasicBlock<I = DefinitionPath> {
    #[serde(deserialize_with = "kagari_abi::decode_limits::instructions")]
    pub instructions: InstructionBuffer<I>,
    #[serde(deserialize_with = "kagari_abi::decode_limits::instructions")]
    pub instruction_spans: SourceSpanBuffer,
    #[serde(deserialize_with = "kagari_abi::decode_limits::instructions")]
    pub instruction_scopes: Vec<usize>,
    pub terminator: Option<Terminator>,
    pub terminator_span: Option<Span>,
    pub terminator_scope: Option<usize>,
}

pub type FunctionBuffer<I = DefinitionPath> = Vec<MirFunction<I>>;
pub type ParameterBuffer = Vec<MirParameter>;
pub type LocalBuffer = Vec<MirLocal>;
pub type ModuleSlotBuffer = Vec<MirModuleSlot>;
pub type TempBuffer = Vec<MirTemp>;
pub type BlockBuffer<I = DefinitionPath> = Vec<BasicBlock<I>>;
pub type SourceSpanBuffer = Vec<Span>;

mod mapping;
