//! Fixed wire data and trait declarations; no semantic operations.
use crate::{
    collection::CollectionAccess,
    declaration::{
        AssociatedConstDef, AssociatedTypeDef, AssociatedTypeFamily, ConstDef, FieldDef, TraitDef,
        TypeDef, TypeDefKind, VariantDef,
        wire::{
            leaf::{ConstraintRecord, ConversionAdapterRecord, NominalTyRecord},
            signature::{FnDeclRecord, GenericBoundRecord, GenericParamRecord},
        },
    },
    decode_limits,
    ty::wire::WireType,
    visibility::Visibility,
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{DefinitionMappingError, check_cancel},
        table::{
            DefinitionId,
            wire::{DefinitionDecoding, DefinitionEncoding, PortableDefinitionRef},
        },
    },
};
use serde::{Deserialize, Serialize};

/// Codec record for the scoped ConstDef declaration; identity fields are file-local references.
#[derive(Serialize, Deserialize)]
pub struct ConstDefRecord {
    name: String,
    ty: WireType,
    value: String,
}

impl ConstDefRecord {
    /// Encode using the enclosing artifact's canonical identity table.
    /// Returns reference, cancellation or bounded type-shape errors.
    pub fn encode(
        value: &ConstDef<DefinitionId>,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(Self {
            name: value.name.clone(),
            ty: WireType::encode(&value.ty, encoding, cancel)?,
            value: value.value.clone(),
        })
    }

    /// Resolve into the matching table's scope, without semantic verification evidence.
    /// Returns reference, cancellation or bounded type-shape errors.
    pub fn decode(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<ConstDef<DefinitionId>, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(ConstDef {
            name: self.name,
            ty: self.ty.decode(decoding, cancel)?,
            value: self.value,
        })
    }
}

/// Codec record for the scoped TypeDef declaration; identity fields are file-local references.
#[derive(Serialize, Deserialize)]
pub struct TypeDefRecord {
    name: String,
    kind: TypeDefKind,
    #[serde(deserialize_with = "decode_limits::nested")]
    generic_params: Vec<GenericParamRecord>,
    #[serde(deserialize_with = "decode_limits::nested")]
    bounds: Vec<GenericBoundRecord>,
    #[serde(deserialize_with = "decode_limits::nested")]
    fields: Vec<FieldDefRecord>,
    #[serde(deserialize_with = "decode_limits::nested")]
    variants: Vec<VariantDefRecord>,
}

impl TypeDefRecord {
    /// Encode using the enclosing artifact's canonical identity table.
    /// Returns reference, cancellation or bounded type-shape errors.
    pub fn encode(
        value: &TypeDef<DefinitionId>,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(Self {
            name: value.name.clone(),
            kind: value.kind,
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
            fields: value
                .fields
                .iter()
                .map(|value| FieldDefRecord::encode(value, encoding, cancel))
                .collect::<Result<_, _>>()?,
            variants: value
                .variants
                .iter()
                .map(|value| VariantDefRecord::encode(value, encoding, cancel))
                .collect::<Result<_, _>>()?,
        })
    }

    /// Resolve into the matching table's scope, without semantic verification evidence.
    /// Returns reference, cancellation or bounded type-shape errors.
    pub fn decode(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<TypeDef<DefinitionId>, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(TypeDef {
            name: self.name,
            kind: self.kind,
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
            fields: self
                .fields
                .into_iter()
                .map(|value| value.decode(decoding, cancel))
                .collect::<Result<_, _>>()?,
            variants: self
                .variants
                .into_iter()
                .map(|value| value.decode(decoding, cancel))
                .collect::<Result<_, _>>()?,
        })
    }
}

/// Codec record for the scoped FieldDef declaration; identity fields are file-local references.
#[derive(Serialize, Deserialize)]
struct FieldDefRecord {
    visibility: Visibility,
    name: String,
    ty: WireType,
    mutable: bool,
}

impl FieldDefRecord {
    /// Encode using the enclosing artifact's canonical identity table.
    /// Returns reference, cancellation or bounded type-shape errors.
    pub fn encode(
        value: &FieldDef<DefinitionId>,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(Self {
            visibility: value.visibility,
            name: value.name.clone(),
            ty: WireType::encode(&value.ty, encoding, cancel)?,
            mutable: value.mutable,
        })
    }

    /// Resolve into the matching table's scope, without semantic verification evidence.
    /// Returns reference, cancellation or bounded type-shape errors.
    pub fn decode(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<FieldDef<DefinitionId>, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(FieldDef {
            visibility: self.visibility,
            name: self.name,
            ty: self.ty.decode(decoding, cancel)?,
            mutable: self.mutable,
        })
    }
}

/// Codec record for the scoped VariantDef declaration; identity fields are file-local references.
#[derive(Serialize, Deserialize)]
struct VariantDefRecord {
    reports_failure: bool,
    name: String,
    #[serde(deserialize_with = "decode_limits::nested")]
    payload: Vec<WireType>,
}

impl VariantDefRecord {
    /// Encode using the enclosing artifact's canonical identity table.
    /// Returns reference, cancellation or bounded type-shape errors.
    pub fn encode(
        value: &VariantDef<DefinitionId>,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(Self {
            reports_failure: value.reports_failure,
            name: value.name.clone(),
            payload: value
                .payload
                .iter()
                .map(|value| WireType::encode(value, encoding, cancel))
                .collect::<Result<_, _>>()?,
        })
    }

    /// Resolve into the matching table's scope, without semantic verification evidence.
    /// Returns reference, cancellation or bounded type-shape errors.
    pub fn decode(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<VariantDef<DefinitionId>, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(VariantDef {
            reports_failure: self.reports_failure,
            name: self.name,
            payload: self
                .payload
                .into_iter()
                .map(|value| value.decode(decoding, cancel))
                .collect::<Result<_, _>>()?,
        })
    }
}

/// Codec record for the scoped TraitDef declaration; identity fields are file-local references.
#[derive(Serialize, Deserialize)]
pub struct TraitDefRecord {
    #[serde(deserialize_with = "decode_limits::nested")]
    associated_consts: Vec<AssociatedConstDefRecord>,
    name: String,
    #[serde(deserialize_with = "decode_limits::nested")]
    supertraits: Vec<NominalTyRecord>,
    #[serde(deserialize_with = "decode_limits::nested")]
    generic_params: Vec<GenericParamRecord>,
    #[serde(deserialize_with = "decode_limits::nested")]
    bounds: Vec<GenericBoundRecord>,
    #[serde(deserialize_with = "decode_limits::nested")]
    methods: Vec<FnDeclRecord>,
    #[serde(deserialize_with = "decode_limits::nested")]
    associated_types: Vec<AssociatedTypeDefRecord>,
    storage_access: Option<CollectionAccess>,
    conversion_adapter: Option<ConversionAdapterRecord>,
}

impl TraitDefRecord {
    /// Encode using the enclosing artifact's canonical identity table.
    /// Returns reference, cancellation or bounded type-shape errors.
    pub fn encode(
        value: &TraitDef<DefinitionId>,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(Self {
            associated_consts: value
                .associated_consts
                .iter()
                .map(|value| AssociatedConstDefRecord::encode(value, encoding, cancel))
                .collect::<Result<_, _>>()?,
            name: value.name.clone(),
            supertraits: value
                .supertraits
                .iter()
                .map(|value| NominalTyRecord::encode(value, encoding, cancel))
                .collect::<Result<_, _>>()?,
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
            methods: value
                .methods
                .iter()
                .map(|value| FnDeclRecord::encode(value, encoding, cancel))
                .collect::<Result<_, _>>()?,
            associated_types: value
                .associated_types
                .iter()
                .map(|value| AssociatedTypeDefRecord::encode(value, encoding, cancel))
                .collect::<Result<_, _>>()?,
            storage_access: value.storage_access,
            conversion_adapter: value
                .conversion_adapter
                .as_ref()
                .map(|value| ConversionAdapterRecord::encode(value, encoding, cancel))
                .transpose()?,
        })
    }

    /// Resolve into the matching table's scope, without semantic verification evidence.
    /// Returns reference, cancellation or bounded type-shape errors.
    pub fn decode(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<TraitDef<DefinitionId>, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(TraitDef {
            associated_consts: self
                .associated_consts
                .into_iter()
                .map(|value| value.decode(decoding, cancel))
                .collect::<Result<_, _>>()?,
            name: self.name,
            supertraits: self
                .supertraits
                .into_iter()
                .map(|value| value.decode(decoding, cancel))
                .collect::<Result<_, _>>()?,
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
            methods: self
                .methods
                .into_iter()
                .map(|value| value.decode(decoding, cancel))
                .collect::<Result<_, _>>()?,
            associated_types: self
                .associated_types
                .into_iter()
                .map(|value| value.decode(decoding, cancel))
                .collect::<Result<_, _>>()?,
            storage_access: self.storage_access,
            conversion_adapter: self
                .conversion_adapter
                .map(|value| value.decode(decoding, cancel))
                .transpose()?,
        })
    }
}

/// Codec record for the scoped AssociatedConstDef declaration; identity fields are file-local references.
#[derive(Serialize, Deserialize)]
struct AssociatedConstDefRecord {
    declaration: PortableDefinitionRef,
    ty: WireType,
    default_value: Option<String>,
}

impl AssociatedConstDefRecord {
    /// Encode using the enclosing artifact's canonical identity table.
    /// Returns reference, cancellation or bounded type-shape errors.
    pub fn encode(
        value: &AssociatedConstDef<DefinitionId>,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(Self {
            declaration: encoding.reference(value.declaration)?,
            ty: WireType::encode(&value.ty, encoding, cancel)?,
            default_value: value.default_value.clone(),
        })
    }

    /// Resolve into the matching table's scope, without semantic verification evidence.
    /// Returns reference, cancellation or bounded type-shape errors.
    pub fn decode(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<AssociatedConstDef<DefinitionId>, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(AssociatedConstDef {
            declaration: decoding.resolve(self.declaration)?,
            ty: self.ty.decode(decoding, cancel)?,
            default_value: self.default_value,
        })
    }
}

/// Codec record for the scoped AssociatedTypeDef declaration; identity fields are file-local references.
#[derive(Serialize, Deserialize)]
struct AssociatedTypeDefRecord {
    #[serde(deserialize_with = "decode_limits::nested")]
    generic_params: Vec<GenericParamRecord>,
    #[serde(deserialize_with = "decode_limits::nested")]
    parameter_bounds: Vec<GenericBoundRecord>,
    declaration: PortableDefinitionRef,
    #[serde(deserialize_with = "decode_limits::nested")]
    bounds: Vec<ConstraintRecord>,
}

impl AssociatedTypeDefRecord {
    /// Encode using the enclosing artifact's canonical identity table.
    /// Returns reference, cancellation or bounded type-shape errors.
    pub fn encode(
        value: &AssociatedTypeDef<DefinitionId>,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(Self {
            generic_params: value
                .generic_params
                .iter()
                .map(|value| GenericParamRecord::encode(value, encoding, cancel))
                .collect::<Result<_, _>>()?,
            parameter_bounds: value
                .parameter_bounds
                .iter()
                .map(|value| GenericBoundRecord::encode(value, encoding, cancel))
                .collect::<Result<_, _>>()?,
            declaration: encoding.reference(value.declaration)?,
            bounds: value
                .bounds
                .iter()
                .map(|value| ConstraintRecord::encode(value, encoding, cancel))
                .collect::<Result<_, _>>()?,
        })
    }

    /// Resolve into the matching table's scope, without semantic verification evidence.
    /// Returns reference, cancellation or bounded type-shape errors.
    pub fn decode(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<AssociatedTypeDef<DefinitionId>, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(AssociatedTypeDef {
            generic_params: self
                .generic_params
                .into_iter()
                .map(|value| value.decode(decoding, cancel))
                .collect::<Result<_, _>>()?,
            parameter_bounds: self
                .parameter_bounds
                .into_iter()
                .map(|value| value.decode(decoding, cancel))
                .collect::<Result<_, _>>()?,
            declaration: decoding.resolve(self.declaration)?,
            bounds: self
                .bounds
                .into_iter()
                .map(|value| value.decode(decoding, cancel))
                .collect::<Result<_, _>>()?,
        })
    }
}

/// Codec record for the scoped AssociatedTypeFamily declaration; identity fields are file-local references.
#[derive(Serialize, Deserialize)]
pub struct AssociatedTypeFamilyRecord {
    declaration: PortableDefinitionRef,
    #[serde(deserialize_with = "decode_limits::nested")]
    generic_params: Vec<GenericParamRecord>,
    #[serde(deserialize_with = "decode_limits::nested")]
    bounds: Vec<GenericBoundRecord>,
    value: WireType,
}

impl AssociatedTypeFamilyRecord {
    /// Encode using the enclosing artifact's canonical identity table.
    /// Returns reference, cancellation or bounded type-shape errors.
    pub fn encode(
        value: &AssociatedTypeFamily<DefinitionId>,
        encoding: &DefinitionEncoding,
        cancel: &CancellationToken,
    ) -> Result<Self, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(Self {
            declaration: encoding.reference(value.declaration)?,
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
            value: WireType::encode(&value.value, encoding, cancel)?,
        })
    }

    /// Resolve into the matching table's scope, without semantic verification evidence.
    /// Returns reference, cancellation or bounded type-shape errors.
    pub fn decode(
        self,
        decoding: &DefinitionDecoding,
        cancel: &CancellationToken,
    ) -> Result<AssociatedTypeFamily<DefinitionId>, DefinitionMappingError> {
        check_cancel(cancel)?;
        Ok(AssociatedTypeFamily {
            declaration: decoding.resolve(self.declaration)?,
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
            value: self.value.decode(decoding, cancel)?,
        })
    }
}
