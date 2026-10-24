//! Program and module records shared by KBC serialization and fingerprints.
use crate::{
    artifact::wire::{
        function::{BytecodeFunctionRecord, FunctionRecordWire},
        interface::InterfaceTableRecordWire,
    },
    module::{BytecodeModule, BytecodeTypeTable, ConstantPool, PathTable},
    program::{BytecodeProgram, ModuleRef, updates::mapping_error},
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        ModuleIdentity,
        mapping::{DefinitionMappingError, check_cancel},
        table::wire::{DefinitionDecoding, DefinitionEncoding},
    },
};
use kagari_contract::{
    exports::wire::ProgramExportsRecord,
    ids::FunctionRef,
    layout::wire::{EnumLayoutRecord, StructLayoutRecord},
    native_import::wire::NativeImportRecord,
    statics::wire::StaticDescriptorRecord,
    types::{
        named::wire::{FunctionOriginRecord, NamedDeclarationRecord},
        wire::{PublicItemRecord, module::TraitContractRecord},
    },
    updates::{
        limits::{Budget, Limits},
        wire::UpdateEvidenceRecord,
    },
};

use kagari_types::declaration::wire::{
    declarations::TypeDeclarationRecord, native::NativeDeclarationRecord,
    path::PathDeclarationRecord,
};
use serde::{Deserialize, Serialize};

/// File-local BytecodeProgram facts; no verification evidence is serialized.
#[derive(Serialize, Deserialize)]
pub(in crate::artifact) struct BytecodeProgramRecord {
    root: ModuleRef,
    #[serde(deserialize_with = "kagari_contract::decode_limits::modules")]
    modules: Vec<BytecodeModuleRecord>,
    exports: ProgramExportsRecord,
    update_evidence: UpdateEvidenceRecord,
}

impl BytecodeProgramRecord {
    /// Convert using the explicit file table; preserve cancellation and shape errors.
    /// The caller retains the resulting scope and performs semantic verification.
    pub(in crate::artifact) fn encode(
        value: &BytecodeProgram,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(Self {
            root: value.root,
            exports: ProgramExportsRecord::encode(&value.exports, encoding, cancel)?,
            update_evidence: UpdateEvidenceRecord::encode(
                &value.update_evidence,
                encoding,
                &mut Budget::new(Limits::default(), cancel).map_err(mapping_error)?,
            )
            .map_err(mapping_error)?,
            modules: value
                .modules
                .iter()
                .map(|value| BytecodeModuleRecord::encode(value, encoding, cancel))
                .collect::<Result<_, _>>()?,
        })
    }

    /// Convert using the explicit file table; preserve cancellation and shape errors.
    /// The caller retains the resulting scope and performs semantic verification.
    pub(in crate::artifact) fn decode(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<BytecodeProgram, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(BytecodeProgram {
            root: self.root,
            exports: self.exports.decode(decoding, cancel)?,
            update_evidence: self
                .update_evidence
                .decode(
                    decoding,
                    &mut Budget::new(Limits::default(), cancel).map_err(mapping_error)?,
                )
                .map_err(mapping_error)?,
            modules: self
                .modules
                .into_iter()
                .map(|value| value.decode(decoding, cancel))
                .collect::<Result<_, _>>()?,
        })
    }
}

/// File-local BytecodeModule facts; no verification evidence is serialized.
#[derive(Serialize, Deserialize)]
pub(in crate::artifact) struct BytecodeModuleRecord {
    #[serde(deserialize_with = "kagari_contract::decode_limits::table")]
    dependencies: Vec<ModuleRef>,
    #[serde(deserialize_with = "kagari_contract::decode_limits::table")]
    external_types: Vec<TypeDeclarationRecord>,
    #[serde(deserialize_with = "kagari_contract::decode_limits::table")]
    path_declarations: Vec<PathDeclarationRecord>,
    #[serde(deserialize_with = "kagari_contract::decode_limits::table")]
    native_imports: Vec<NativeImportRecord>,
    #[serde(deserialize_with = "kagari_contract::decode_limits::table")]
    native_declarations: Vec<NativeDeclarationRecord>,
    identity: ModuleIdentity,
    source_name: String,
    #[serde(deserialize_with = "kagari_contract::decode_limits::table")]
    module_slots: Vec<StaticDescriptorRecord<FunctionRef>>,
    #[serde(deserialize_with = "kagari_contract::decode_limits::table")]
    constants: ConstantPool,
    #[serde(deserialize_with = "kagari_contract::decode_limits::table")]
    types: BytecodeTypeTable,
    #[serde(deserialize_with = "kagari_contract::decode_limits::table")]
    structures: Vec<StructLayoutRecord>,
    #[serde(deserialize_with = "kagari_contract::decode_limits::table")]
    enumerations: Vec<EnumLayoutRecord>,
    #[serde(deserialize_with = "kagari_contract::decode_limits::table")]
    interface_tables: Vec<InterfaceTableRecordWire>,
    #[serde(deserialize_with = "kagari_contract::decode_limits::table")]
    paths: PathTable,
    #[serde(deserialize_with = "kagari_contract::decode_limits::table")]
    function_table: Vec<FunctionRecordWire>,
    #[serde(deserialize_with = "kagari_contract::decode_limits::table")]
    public_items: Vec<PublicItemRecord>,
    #[serde(deserialize_with = "kagari_contract::decode_limits::table")]
    private_types: Vec<TypeDeclarationRecord>,
    #[serde(deserialize_with = "kagari_contract::decode_limits::table")]
    named_declarations: Vec<NamedDeclarationRecord>,
    #[serde(deserialize_with = "kagari_contract::decode_limits::table")]
    function_origins: Vec<FunctionOriginRecord>,
    #[serde(deserialize_with = "kagari_contract::decode_limits::nested")]
    trait_references: Vec<TraitContractRecord>,
    #[serde(deserialize_with = "kagari_contract::decode_limits::nested")]
    trait_contracts: Vec<TraitContractRecord>,
    #[serde(deserialize_with = "kagari_contract::decode_limits::functions")]
    functions: Vec<BytecodeFunctionRecord>,
}

impl BytecodeModuleRecord {
    /// Convert using the explicit file table; preserve cancellation and shape errors.
    /// The caller retains the resulting scope and performs semantic verification.
    pub(in crate::artifact) fn encode(
        value: &BytecodeModule,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(Self {
            dependencies: value.dependencies.clone(),
            external_types: value
                .external_types
                .iter()
                .map(|value| TypeDeclarationRecord::encode(value, encoding, cancel))
                .collect::<Result<_, _>>()?,
            path_declarations: value
                .path_declarations
                .iter()
                .map(|path| PathDeclarationRecord::encode(path, encoding, cancel))
                .collect::<Result<_, _>>()?,
            native_imports: value
                .native_imports
                .iter()
                .map(|value| NativeImportRecord::encode(value, encoding, cancel))
                .collect::<Result<_, _>>()?,
            native_declarations: value
                .native_declarations
                .iter()
                .map(|value| NativeDeclarationRecord::encode(value, encoding, cancel))
                .collect::<Result<_, _>>()?,
            identity: value.identity.clone(),
            source_name: value.source_name.clone(),
            module_slots: value
                .module_slots
                .iter()
                .map(|slot| StaticDescriptorRecord::encode(slot, encoding, cancel))
                .collect::<Result<_, _>>()?,
            constants: value.constants.clone(),
            types: value.types.clone(),
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
            interface_tables: value
                .interface_tables
                .iter()
                .map(|value| InterfaceTableRecordWire::encode(value, encoding, cancel))
                .collect::<Result<_, _>>()?,
            paths: value.paths.clone(),
            function_table: value
                .function_table
                .iter()
                .map(|value| FunctionRecordWire::encode(value, encoding, cancel))
                .collect::<Result<_, _>>()?,
            public_items: value
                .public_items
                .iter()
                .map(|value| PublicItemRecord::encode(value, encoding, cancel))
                .collect::<Result<_, _>>()?,
            private_types: value
                .private_types
                .iter()
                .map(|value| TypeDeclarationRecord::encode(value, encoding, cancel))
                .collect::<Result<_, _>>()?,
            named_declarations: value
                .named_declarations
                .iter()
                .map(|value| NamedDeclarationRecord::encode(value, encoding, cancel))
                .collect::<Result<_, _>>()?,
            function_origins: value
                .function_origins
                .iter()
                .map(|value| FunctionOriginRecord::encode(value, encoding, cancel))
                .collect::<Result<_, _>>()?,
            trait_references: value
                .trait_references
                .iter()
                .map(|value| TraitContractRecord::encode(value, encoding, cancel))
                .collect::<Result<_, _>>()?,
            trait_contracts: value
                .trait_contracts
                .iter()
                .map(|value| TraitContractRecord::encode(value, encoding, cancel))
                .collect::<Result<_, _>>()?,
            functions: value
                .functions
                .iter()
                .map(|value| BytecodeFunctionRecord::encode(value, encoding, cancel))
                .collect::<Result<_, _>>()?,
        })
    }

    /// Convert using the explicit file table; preserve cancellation and shape errors.
    /// The caller retains the resulting scope and performs semantic verification.
    pub(in crate::artifact) fn decode(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<BytecodeModule, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(BytecodeModule {
            dependencies: self.dependencies,
            external_types: self
                .external_types
                .into_iter()
                .map(|value| value.decode(decoding, cancel))
                .collect::<Result<_, _>>()?,
            path_declarations: self
                .path_declarations
                .into_iter()
                .map(|path| path.decode(decoding, cancel))
                .collect::<Result<_, _>>()?,
            native_imports: self
                .native_imports
                .into_iter()
                .map(|value| value.decode(decoding, cancel))
                .collect::<Result<_, _>>()?,
            native_declarations: self
                .native_declarations
                .into_iter()
                .map(|value| value.decode(decoding, cancel))
                .collect::<Result<_, _>>()?,
            identity: self.identity,
            source_name: self.source_name,
            module_slots: self
                .module_slots
                .into_iter()
                .map(|slot| slot.decode(decoding, cancel))
                .collect::<Result<_, _>>()?,
            constants: self.constants,
            types: self.types,
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
            interface_tables: self
                .interface_tables
                .into_iter()
                .map(|value| value.decode(decoding, cancel))
                .collect::<Result<_, _>>()?,
            paths: self.paths,
            function_table: self
                .function_table
                .into_iter()
                .map(|value| value.decode(decoding, cancel))
                .collect::<Result<_, _>>()?,
            public_items: self
                .public_items
                .into_iter()
                .map(|value| value.decode(decoding, cancel))
                .collect::<Result<_, _>>()?,
            private_types: self
                .private_types
                .into_iter()
                .map(|value| value.decode(decoding, cancel))
                .collect::<Result<_, _>>()?,
            named_declarations: self
                .named_declarations
                .into_iter()
                .map(|value| value.decode(decoding, cancel))
                .collect::<Result<_, _>>()?,
            function_origins: self
                .function_origins
                .into_iter()
                .map(|value| value.decode(decoding, cancel))
                .collect::<Result<_, _>>()?,
            trait_references: self
                .trait_references
                .into_iter()
                .map(|value| value.decode(decoding, cancel))
                .collect::<Result<_, _>>()?,
            trait_contracts: self
                .trait_contracts
                .into_iter()
                .map(|value| value.decode(decoding, cancel))
                .collect::<Result<_, _>>()?,
            functions: self
                .functions
                .into_iter()
                .map(|value| value.decode(decoding, cancel))
                .collect::<Result<_, _>>()?,
        })
    }
}
