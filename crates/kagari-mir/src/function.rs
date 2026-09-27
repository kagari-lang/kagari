use kagari_abi::layout::EnumLayout;
use kagari_abi::layout::StructLayout;
use kagari_abi::slots::SemanticSlots;
use kagari_abi::types::ConcreteFunctionIdentity;
use kagari_abi::types::NominalAbiType;
use kagari_common::Span;
use kagari_common::host_interface::HostTypeDeclaration;
use kagari_common::identity::ModuleIdentity;
use serde::{Deserialize, Serialize};

use crate::debug::MirFunctionDebugMetadata;
use crate::ids::BlockId;
use crate::ids::InstanceId;
use crate::ids::LocalId;
use crate::ids::ModuleSlotId;
use crate::ids::TempId;
use crate::instruction::InstructionBuffer;
use crate::instruction::Terminator;
use kagari_abi::effects::EffectSet;
use kagari_abi::representation::ValueType;
use kagari_abi::types::ModuleAbi;
use std::iter;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MirModule {
    /// Concrete interface demands, including inherited views that need no
    /// source allocation instruction of their own.
    #[serde(deserialize_with = "kagari_abi::decode_limits::nested")]
    pub interface_instances: Vec<ConcreteFunctionIdentity>,
    #[serde(deserialize_with = "kagari_abi::decode_limits::table")]
    pub host_types: Vec<HostTypeDeclaration>,
    #[serde(deserialize_with = "kagari_abi::decode_limits::nested")]
    pub dependencies: Vec<ModuleIdentity>,
    #[serde(deserialize_with = "kagari_abi::decode_limits::table")]
    pub structures: Vec<StructLayout>,
    #[serde(deserialize_with = "kagari_abi::decode_limits::table")]
    pub enumerations: Vec<EnumLayout>,
    pub identity: ModuleIdentity,
    pub source_name: String,
    #[serde(deserialize_with = "kagari_abi::decode_limits::table")]
    pub module_slots: ModuleSlotBuffer,
    pub abi: ModuleAbi,
    #[serde(deserialize_with = "kagari_abi::decode_limits::functions")]
    pub functions: FunctionBuffer,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MirFunction {
    pub semantic: SemanticSlots,
    pub id: InstanceId,
    pub instance: ConcreteFunctionIdentity,
    pub name: String,
    #[serde(deserialize_with = "kagari_abi::decode_limits::nested")]
    pub params: ParameterBuffer,
    pub return_type: ValueType,
    #[serde(deserialize_with = "kagari_abi::decode_limits::table")]
    pub locals: LocalBuffer,
    #[serde(deserialize_with = "kagari_abi::decode_limits::table")]
    pub temps: TempBuffer,
    #[serde(deserialize_with = "kagari_abi::decode_limits::table")]
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

impl MirModule {
    pub fn structure(&self, instance: &NominalAbiType) -> Option<&StructLayout> {
        self.structures.iter().find(|layout| {
            layout.declaration == instance.declaration && layout.arguments == instance.arguments
        })
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
pub struct BasicBlock {
    #[serde(deserialize_with = "kagari_abi::decode_limits::instructions")]
    pub instructions: InstructionBuffer,
    #[serde(deserialize_with = "kagari_abi::decode_limits::instructions")]
    pub instruction_spans: SourceSpanBuffer,
    #[serde(deserialize_with = "kagari_abi::decode_limits::instructions")]
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
