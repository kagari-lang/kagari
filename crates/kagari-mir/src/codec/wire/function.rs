//! MIR module, function and block records with bounded serialized collections.
use crate::{
    codec::wire::instruction::InstructionRecord,
    debug::MirFunctionDebugMetadata,
    function::{
        BasicBlock, LocalBuffer, MirFunction, MirModule, ModuleSlotBuffer, ParameterBuffer,
        SourceSpanBuffer, TempBuffer,
    },
    ids::{BlockId, InstanceId},
    instruction::Terminator,
};
use kagari_abi::representation::ValueType;
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        ModuleIdentity,
        mapping::{DefinitionMappingError, check_cancel},
        table::wire::{DefinitionDecoding, DefinitionEncoding},
    },
    span::Span,
};
use kagari_contract::{
    callable::wire::InterfaceInstanceRecord,
    effects::EffectSet,
    layout::wire::{EnumLayoutRecord, StructLayoutRecord},
    native_import::wire::NativeImportRecord,
    slots::wire::SemanticSlotsRecord,
    types::wire::module::{ConcreteFunctionIdentityRecord, ModuleContractRecord},
};
use kagari_types::host_interface::wire::declarations::WireHostTypeDeclaration;
use serde::{Deserialize, Serialize};

/// File-local MirModule facts; no verification evidence is serialized.
#[derive(Serialize, Deserialize)]
pub(crate) struct MirModuleRecord {
    #[serde(deserialize_with = "kagari_contract::decode_limits::nested")]
    native_targets: Vec<NativeImportRecord>,
    #[serde(deserialize_with = "kagari_contract::decode_limits::nested")]
    interface_instances: Vec<InterfaceInstanceRecord>,
    #[serde(deserialize_with = "kagari_contract::decode_limits::table")]
    host_types: Vec<WireHostTypeDeclaration>,
    #[serde(deserialize_with = "kagari_contract::decode_limits::nested")]
    dependencies: Vec<ModuleIdentity>,
    #[serde(deserialize_with = "kagari_contract::decode_limits::table")]
    structures: Vec<StructLayoutRecord>,
    #[serde(deserialize_with = "kagari_contract::decode_limits::table")]
    enumerations: Vec<EnumLayoutRecord>,
    identity: ModuleIdentity,
    source_name: String,
    #[serde(deserialize_with = "kagari_contract::decode_limits::table")]
    module_slots: ModuleSlotBuffer,
    abi: ModuleContractRecord,
    #[serde(deserialize_with = "kagari_contract::decode_limits::functions")]
    functions: Vec<MirFunctionRecord>,
}

impl MirModuleRecord {
    /// Convert using the explicit file table; preserve cancellation and shape errors.
    /// The caller retains the resulting scope and performs semantic verification.
    pub(crate) fn encode(
        value: &MirModule,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(Self {
            native_targets: value
                .native_targets
                .iter()
                .map(|value| NativeImportRecord::encode(value, encoding, cancel))
                .collect::<Result<_, _>>()?,
            interface_instances: value
                .interface_instances
                .iter()
                .map(|value| InterfaceInstanceRecord::encode(value, encoding, cancel))
                .collect::<Result<_, _>>()?,
            host_types: value
                .host_types
                .iter()
                .map(|value| {
                    Ok::<_, DefinitionMappingError>(WireHostTypeDeclaration::encode(
                        value, encoding, cancel,
                    )?)
                })
                .collect::<Result<_, _>>()?,
            dependencies: value.dependencies.clone(),
            structures: value
                .structures
                .iter()
                .map(|value| StructLayoutRecord::encode(value, encoding, cancel))
                .collect::<Result<_, _>>()?,
            enumerations: value
                .enumerations
                .iter()
                .map(|value| EnumLayoutRecord::encode(value, encoding, cancel))
                .collect::<Result<_, _>>()?,
            identity: value.identity.clone(),
            source_name: value.source_name.clone(),
            module_slots: value.module_slots.clone(),
            abi: ModuleContractRecord::encode(&value.abi, encoding, cancel)?,
            functions: value
                .functions
                .iter()
                .map(|value| MirFunctionRecord::encode(value, encoding, cancel))
                .collect::<Result<_, _>>()?,
        })
    }

    /// Convert using the explicit file table; preserve cancellation and shape errors.
    /// The caller retains the resulting scope and performs semantic verification.
    pub(crate) fn decode(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<MirModule, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(MirModule {
            native_targets: self
                .native_targets
                .into_iter()
                .map(|value| value.decode(decoding, cancel))
                .collect::<Result<_, _>>()?,
            interface_instances: self
                .interface_instances
                .into_iter()
                .map(|value| value.decode(decoding, cancel))
                .collect::<Result<_, _>>()?,
            host_types: self
                .host_types
                .into_iter()
                .map(|value| Ok::<_, DefinitionMappingError>(value.decode(decoding, cancel)?))
                .collect::<Result<_, _>>()?,
            dependencies: self.dependencies,
            structures: self
                .structures
                .into_iter()
                .map(|value| value.decode(decoding, cancel))
                .collect::<Result<_, _>>()?,
            enumerations: self
                .enumerations
                .into_iter()
                .map(|value| value.decode(decoding, cancel))
                .collect::<Result<_, _>>()?,
            identity: self.identity,
            source_name: self.source_name,
            module_slots: self.module_slots,
            abi: self.abi.decode(decoding, cancel)?,
            functions: self
                .functions
                .into_iter()
                .map(|value| value.decode(decoding, cancel))
                .collect::<Result<_, _>>()?,
        })
    }
}

/// File-local MirFunction facts; no verification evidence is serialized.
#[derive(Serialize, Deserialize)]
struct MirFunctionRecord {
    semantic: SemanticSlotsRecord,
    id: InstanceId,
    instance: ConcreteFunctionIdentityRecord,
    name: String,
    #[serde(deserialize_with = "kagari_contract::decode_limits::nested")]
    params: ParameterBuffer,
    return_type: ValueType,
    #[serde(deserialize_with = "kagari_contract::decode_limits::table")]
    locals: LocalBuffer,
    #[serde(deserialize_with = "kagari_contract::decode_limits::table")]
    temps: TempBuffer,
    #[serde(deserialize_with = "kagari_contract::decode_limits::table")]
    blocks: Vec<BasicBlockRecord>,
    entry: BlockId,
    effects: EffectSet,
    debug: MirFunctionDebugMetadata,
}

impl MirFunctionRecord {
    /// Convert using the explicit file table; preserve cancellation and shape errors.
    /// The caller retains the resulting scope and performs semantic verification.
    fn encode(
        value: &MirFunction,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(Self {
            semantic: SemanticSlotsRecord::encode(&value.semantic, encoding, cancel)?,
            id: value.id,
            instance: ConcreteFunctionIdentityRecord::encode(&value.instance, encoding, cancel)?,
            name: value.name.clone(),
            params: value.params.clone(),
            return_type: value.return_type,
            locals: value.locals.clone(),
            temps: value.temps.clone(),
            blocks: value
                .blocks
                .iter()
                .map(|value| BasicBlockRecord::encode(value, encoding, cancel))
                .collect::<Result<_, _>>()?,
            entry: value.entry,
            effects: value.effects,
            debug: value.debug.clone(),
        })
    }

    /// Convert using the explicit file table; preserve cancellation and shape errors.
    /// The caller retains the resulting scope and performs semantic verification.
    fn decode(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<MirFunction, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(MirFunction {
            semantic: self.semantic.decode(decoding, cancel)?,
            id: self.id,
            instance: self.instance.decode(decoding, cancel)?,
            name: self.name,
            params: self.params,
            return_type: self.return_type,
            locals: self.locals,
            temps: self.temps,
            blocks: self
                .blocks
                .into_iter()
                .map(|value| value.decode(decoding, cancel))
                .collect::<Result<_, _>>()?,
            entry: self.entry,
            effects: self.effects,
            debug: self.debug,
        })
    }
}

/// File-local BasicBlock facts; no verification evidence is serialized.
#[derive(Serialize, Deserialize)]
struct BasicBlockRecord {
    #[serde(deserialize_with = "kagari_contract::decode_limits::instructions")]
    instructions: Vec<InstructionRecord>,
    #[serde(deserialize_with = "kagari_contract::decode_limits::instructions")]
    instruction_spans: SourceSpanBuffer,
    #[serde(deserialize_with = "kagari_contract::decode_limits::instructions")]
    instruction_scopes: Vec<usize>,
    terminator: Option<Terminator>,
    terminator_span: Option<Span>,
    terminator_scope: Option<usize>,
}

impl BasicBlockRecord {
    /// Convert using the explicit file table; preserve cancellation and shape errors.
    /// The caller retains the resulting scope and performs semantic verification.
    fn encode(
        value: &BasicBlock,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(Self {
            instructions: value
                .instructions
                .iter()
                .map(|value| InstructionRecord::encode(value, encoding, cancel))
                .collect::<Result<_, _>>()?,
            instruction_spans: value.instruction_spans.clone(),
            instruction_scopes: value.instruction_scopes.clone(),
            terminator: value.terminator.clone(),
            terminator_span: value.terminator_span,
            terminator_scope: value.terminator_scope,
        })
    }

    /// Convert using the explicit file table; preserve cancellation and shape errors.
    /// The caller retains the resulting scope and performs semantic verification.
    fn decode(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<BasicBlock, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(BasicBlock {
            instructions: self
                .instructions
                .into_iter()
                .map(|value| value.decode(decoding, cancel))
                .collect::<Result<_, _>>()?,
            instruction_spans: self.instruction_spans,
            instruction_scopes: self.instruction_scopes,
            terminator: self.terminator,
            terminator_span: self.terminator_span,
            terminator_scope: self.terminator_scope,
        })
    }
}
