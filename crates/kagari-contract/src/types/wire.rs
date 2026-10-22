//! Public ABI file records shared by artifact codecs and ABI fingerprints.
use crate::{
    decode_limits,
    statics::wire::StaticContractRecord,
    types::{InterfaceBridge, InterfaceTable, PublicItem, inherent::InherentTable},
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
        declarations::{AssociatedTypeFamilyRecord, ConstDefRecord, TraitDefRecord, TypeDefRecord},
        signature::{FnDeclRecord, GenericBoundRecord, GenericParamRecord},
    },
    ty::wire::WireType,
};
use serde::{Deserialize, Serialize};

/// Opaque serialized ABI facts. File references never enter the semantic model.
/// Decode resolves the supplied table's scope, without validating declaration ABI
/// or granting executable evidence. The artifact owner retains and verifies both.
#[derive(Serialize, Deserialize)]
#[serde(transparent)]
pub struct PublicItemRecord {
    record: Record,
}

#[derive(Serialize, Deserialize)]
enum Record {
    Function(FnDeclRecord),
    Const(ConstDefRecord),
    Static(StaticContractRecord),
    Type(TypeDefRecord),
    Trait(TraitDefRecord),
    InterfaceTable(Box<InterfaceTableRecord>),
    InherentTable(Box<InherentTableRecord>),
}

impl PublicItemRecord {
    /// Encode scoped ABI facts using the canonical table prepared by the owner.
    /// Returns invalid-reference, bounded type-shape and cancellation errors.
    pub fn encode(
        value: &PublicItem,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self, DefinitionMappingError> {
        check_cancel(cancel)?;
        let record = match value {
            PublicItem::Function(value) => {
                Record::Function(FnDeclRecord::encode(value, encoding, cancel)?)
            }
            PublicItem::Static(value) => {
                Record::Static(StaticContractRecord::encode(value, encoding, cancel)?)
            }
            PublicItem::Const(value) => {
                Record::Const(ConstDefRecord::encode(value, encoding, cancel)?)
            }
            PublicItem::Type(value) => {
                Record::Type(TypeDefRecord::encode(value, encoding, cancel)?)
            }
            PublicItem::Trait(value) => {
                Record::Trait(TraitDefRecord::encode(value, encoding, cancel)?)
            }
            PublicItem::InterfaceTable(value) => Record::InterfaceTable(Box::new(
                InterfaceTableRecord::encode(value, encoding, cancel)?,
            )),
            PublicItem::InherentTable(value) => Record::InherentTable(Box::new(
                InherentTableRecord::encode(value, encoding, cancel)?,
            )),
        };
        Ok(Self { record })
    }

    /// Resolve bounded wire facts into IDs in the supplied decoded table.
    /// Returns invalid-reference, bounded type-shape and cancellation errors.
    pub fn decode(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<PublicItem, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(match self.record {
            Record::Function(value) => PublicItem::Function(value.decode(decoding, cancel)?),
            Record::Const(value) => PublicItem::Const(value.decode(decoding, cancel)?),
            Record::Static(value) => PublicItem::Static(value.decode(decoding, cancel)?),
            Record::Type(value) => PublicItem::Type(value.decode(decoding, cancel)?),
            Record::Trait(value) => PublicItem::Trait(value.decode(decoding, cancel)?),
            Record::InterfaceTable(value) => {
                PublicItem::InterfaceTable(Box::new(value.decode(decoding, cancel)?))
            }
            Record::InherentTable(value) => {
                PublicItem::InherentTable(Box::new(value.decode(decoding, cancel)?))
            }
        })
    }
}

/// Codec record for the scoped InterfaceTable declaration; fields contain only file references.
#[derive(Serialize, Deserialize)]
struct InterfaceTableRecord {
    #[serde(deserialize_with = "decode_limits::nested")]
    associated_type_families: Vec<AssociatedTypeFamilyRecord>,
    #[serde(deserialize_with = "decode_limits::nested")]
    associated_consts: Vec<ConstDefRecord>,
    bridge: Option<InterfaceBridge>,
    declaration: PortableDefinitionRef,
    name: String,
    #[serde(deserialize_with = "decode_limits::nested")]
    generic_params: Vec<GenericParamRecord>,
    #[serde(deserialize_with = "decode_limits::nested")]
    bounds: Vec<GenericBoundRecord>,
    trait_type: WireType,
    for_type: WireType,
    #[serde(deserialize_with = "decode_limits::nested")]
    methods: Vec<FnDeclRecord>,
}

impl InterfaceTableRecord {
    /// Encode using the enclosing artifact's canonical identity table.
    /// Returns reference, cancellation or bounded type-shape errors.
    pub fn encode(
        value: &InterfaceTable,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(Self {
            associated_type_families: value
                .associated_type_families
                .iter()
                .map(|value| AssociatedTypeFamilyRecord::encode(value, encoding, cancel))
                .collect::<Result<_, _>>()?,
            associated_consts: value
                .associated_consts
                .iter()
                .map(|value| ConstDefRecord::encode(value, encoding, cancel))
                .collect::<Result<_, _>>()?,
            bridge: value.bridge,
            declaration: encoding.reference(value.declaration)?,
            name: value.name.clone(),
            generic_params: value
                .generic_params
                .iter()
                .map(|value| GenericParamRecord::encode(value, encoding, cancel))
                .collect::<Result<_, _>>()?,
            bounds: value
                .bounds
                .iter()
                .map(|value| GenericBoundRecord::encode(value, encoding, cancel))
                .collect::<Result<_, _>>()?,
            trait_type: WireType::encode(&value.trait_type, encoding, cancel)?,
            for_type: WireType::encode(&value.for_type, encoding, cancel)?,
            methods: value
                .methods
                .iter()
                .map(|value| FnDeclRecord::encode(value, encoding, cancel))
                .collect::<Result<_, _>>()?,
        })
    }

    /// Resolve into the matching table's scope, without semantic verification evidence.
    /// Returns reference, cancellation or bounded type-shape errors.
    pub fn decode(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<InterfaceTable, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(InterfaceTable {
            associated_type_families: self
                .associated_type_families
                .into_iter()
                .map(|value| value.decode(decoding, cancel))
                .collect::<Result<_, _>>()?,
            associated_consts: self
                .associated_consts
                .into_iter()
                .map(|value| value.decode(decoding, cancel))
                .collect::<Result<_, _>>()?,
            bridge: self.bridge,
            declaration: decoding.resolve(self.declaration)?,
            name: self.name,
            generic_params: self
                .generic_params
                .into_iter()
                .map(|value| value.decode(decoding, cancel))
                .collect::<Result<_, _>>()?,
            bounds: self
                .bounds
                .into_iter()
                .map(|value| value.decode(decoding, cancel))
                .collect::<Result<_, _>>()?,
            trait_type: self.trait_type.decode(decoding, cancel)?,
            for_type: self.for_type.decode(decoding, cancel)?,
            methods: self
                .methods
                .into_iter()
                .map(|value| value.decode(decoding, cancel))
                .collect::<Result<_, _>>()?,
        })
    }
}

/// Codec record for the scoped InherentTable declaration; fields contain only file references.
#[derive(Serialize, Deserialize)]
struct InherentTableRecord {
    declaration: PortableDefinitionRef,
    name: String,
    #[serde(deserialize_with = "decode_limits::nested")]
    generic_params: Vec<GenericParamRecord>,
    #[serde(deserialize_with = "decode_limits::nested")]
    bounds: Vec<GenericBoundRecord>,
    for_type: WireType,
    #[serde(deserialize_with = "decode_limits::nested")]
    methods: Vec<FnDeclRecord>,
}

impl InherentTableRecord {
    /// Encode using the enclosing artifact's canonical identity table.
    /// Returns reference, cancellation or bounded type-shape errors.
    pub fn encode(
        value: &InherentTable,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(Self {
            declaration: encoding.reference(value.declaration)?,
            name: value.name.clone(),
            generic_params: value
                .generic_params
                .iter()
                .map(|value| GenericParamRecord::encode(value, encoding, cancel))
                .collect::<Result<_, _>>()?,
            bounds: value
                .bounds
                .iter()
                .map(|value| GenericBoundRecord::encode(value, encoding, cancel))
                .collect::<Result<_, _>>()?,
            for_type: WireType::encode(&value.for_type, encoding, cancel)?,
            methods: value
                .methods
                .iter()
                .map(|value| FnDeclRecord::encode(value, encoding, cancel))
                .collect::<Result<_, _>>()?,
        })
    }

    /// Resolve into the matching table's scope, without semantic verification evidence.
    /// Returns reference, cancellation or bounded type-shape errors.
    pub fn decode(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<InherentTable, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(InherentTable {
            declaration: decoding.resolve(self.declaration)?,
            name: self.name,
            generic_params: self
                .generic_params
                .into_iter()
                .map(|value| value.decode(decoding, cancel))
                .collect::<Result<_, _>>()?,
            bounds: self
                .bounds
                .into_iter()
                .map(|value| value.decode(decoding, cancel))
                .collect::<Result<_, _>>()?,
            for_type: self.for_type.decode(decoding, cancel)?,
            methods: self
                .methods
                .into_iter()
                .map(|value| value.decode(decoding, cancel))
                .collect::<Result<_, _>>()?,
        })
    }
}

#[doc(hidden)]
pub mod module;
