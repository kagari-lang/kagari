//! File-local module ABI facts and concrete function identities.
use crate::types::{
    ConcreteFunctionIdentity, ModuleContract, TraitContract, wire::PublicItemRecord,
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{DefinitionMappingError, check_cancel},
        table::wire::{DefinitionDecoding, DefinitionEncoding, PortableDefinitionRef},
    },
};
use kagari_types::{
    declaration::wire::{
        declarations::{TraitDefRecord, TypeDeclarationRecord},
        native::NativeDeclarationRecord,
    },
    ty::wire::WireType,
};
use serde::{Deserialize, Serialize};

/// File-local ModuleContract facts; no verification evidence is serialized.
#[derive(Serialize, Deserialize)]
pub struct ModuleContractRecord {
    #[serde(deserialize_with = "crate::decode_limits::table")]
    native_declarations: Vec<NativeDeclarationRecord>,
    #[serde(deserialize_with = "crate::decode_limits::table")]
    public_items: Vec<PublicItemRecord>,
    #[serde(deserialize_with = "crate::decode_limits::table")]
    private_types: Vec<TypeDeclarationRecord>,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    trait_references: Vec<TraitContractRecord>,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    trait_contracts: Vec<TraitContractRecord>,
}

impl ModuleContractRecord {
    /// Convert using the explicit file table; preserve cancellation and shape errors.
    /// The caller retains the resulting scope and performs semantic verification.
    pub fn encode(
        value: &ModuleContract,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(Self {
            native_declarations: value
                .native_declarations
                .iter()
                .map(|value| NativeDeclarationRecord::encode(value, encoding, cancel))
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
        })
    }

    /// Convert using the explicit file table; preserve cancellation and shape errors.
    /// The caller retains the resulting scope and performs semantic verification.
    pub fn decode(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<ModuleContract, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(ModuleContract {
            native_declarations: self
                .native_declarations
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
        })
    }
}

/// File-local TraitContract facts; no verification evidence is serialized.
#[derive(Serialize, Deserialize)]
pub struct TraitContractRecord {
    declaration: PortableDefinitionRef,
    abi: TraitDefRecord,
}

impl TraitContractRecord {
    /// Convert using the explicit file table; preserve cancellation and shape errors.
    /// The caller retains the resulting scope and performs semantic verification.
    pub fn encode(
        value: &TraitContract,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(Self {
            declaration: encoding.reference(value.declaration)?,
            abi: TraitDefRecord::encode(&value.abi, encoding, cancel)?,
        })
    }

    /// Convert using the explicit file table; preserve cancellation and shape errors.
    /// The caller retains the resulting scope and performs semantic verification.
    pub fn decode(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<TraitContract, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(TraitContract {
            declaration: decoding.resolve(self.declaration)?,
            abi: self.abi.decode(decoding, cancel)?,
        })
    }
}

/// File-local ConcreteFunctionIdentity facts; no verification evidence is serialized.
#[derive(Serialize, Deserialize)]
pub struct ConcreteFunctionIdentityRecord {
    declaration: PortableDefinitionRef,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    arguments: Vec<WireType>,
}

impl ConcreteFunctionIdentityRecord {
    /// Convert using the explicit file table; preserve cancellation and shape errors.
    /// The caller retains the resulting scope and performs semantic verification.
    pub fn encode(
        value: &ConcreteFunctionIdentity,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(Self {
            declaration: encoding.reference(value.declaration)?,
            arguments: value
                .arguments
                .iter()
                .map(|value| {
                    Ok::<_, DefinitionMappingError>(WireType::encode(value, encoding, cancel)?)
                })
                .collect::<Result<_, _>>()?,
        })
    }

    /// Convert using the explicit file table; preserve cancellation and shape errors.
    /// The caller retains the resulting scope and performs semantic verification.
    pub fn decode(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<ConcreteFunctionIdentity, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(ConcreteFunctionIdentity {
            declaration: decoding.resolve(self.declaration)?,
            arguments: self
                .arguments
                .into_iter()
                .map(|value| Ok::<_, DefinitionMappingError>(value.decode(decoding, cancel)?))
                .collect::<Result<_, _>>()?,
        })
    }
}
