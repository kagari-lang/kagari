//! Portable semantic declarations and symbolic method defaults.
pub mod applications;
pub mod conversion;
mod mapping;
pub mod module;
pub mod native;
pub mod ownership;
pub mod requirement;
pub mod verify;
use crate::{
    callable::{CallableImplementation, MethodPolicy},
    collection::CollectionAccess,
    declaration::{
        native::{NativeStorageLayout, NativeTypeConstructor},
        requirement::NativeCallableRequirement,
    },
    ty::{Constraint, GenericBound, GenericParam, NominalTy, Ty},
};
use kagari_common::identity::{DefinitionKind, DefinitionPath, reference::DefinitionReference};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound(
    serialize = "I: DefinitionReference + serde::Serialize",
    deserialize = "I: DefinitionReference + serde::Deserialize<'de>"
))]
pub struct FnDecl<I = DefinitionPath> {
    pub method_policy: MethodPolicy,
    pub name: String,
    pub implementation: CallableImplementation<I>,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub generic_params: Vec<GenericParam<I>>,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub bounds: Vec<GenericBound<I>>,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub params: Vec<Param<I>>,
    pub return_type: Ty<I>,
}

/// Declaration contract for native entrypoints, including inherent and private
/// methods absent from the public ABI. The function uses the same checked model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound(
    serialize = "I: DefinitionReference + serde::Serialize",
    deserialize = "I: DefinitionReference + serde::Deserialize<'de>"
))]
pub struct NativeDeclaration<I = DefinitionPath> {
    /// Concrete value produced by Rust before a checked interface-result adapter.
    pub concrete_result: Option<Ty<I>>,
    pub declaration: I,
    pub function: FnDecl<I>,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub callable_requirements: Vec<NativeCallableRequirement<I>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound(
    serialize = "I: DefinitionReference + serde::Serialize",
    deserialize = "I: DefinitionReference + serde::Deserialize<'de>"
))]
pub struct Param<I = DefinitionPath> {
    pub name: String,
    pub ty: Ty<I>,
    pub mutable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound(
    serialize = "I: DefinitionReference + serde::Serialize",
    deserialize = "I: DefinitionReference + serde::Deserialize<'de>"
))]
pub struct ConstDef<I = DefinitionPath> {
    pub name: String,
    pub ty: Ty<I>,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound(
    serialize = "I: DefinitionReference + serde::Serialize",
    deserialize = "I: DefinitionReference + serde::Deserialize<'de>"
))]
pub struct TypeDef<I = DefinitionPath> {
    pub name: String,
    pub kind: TypeDefKind,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub generic_params: Vec<GenericParam<I>>,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub bounds: Vec<GenericBound<I>>,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub fields: Vec<FieldDef<I>>,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub variants: Vec<VariantDef<I>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TypeDefKind {
    Struct,
    Enum,
    Native(NativeTypeConstructor),
    NativeStorage(NativeStorageLayout),
}

impl TypeDefKind {
    /// Nominal identity belongs to the declaration kind, independent of its producer.
    pub fn definition_kind(self) -> DefinitionKind {
        match self {
            Self::Struct => DefinitionKind::Struct,
            Self::Enum => DefinitionKind::Enum,
            Self::Native(constructor) => constructor.declaration_kind(),
            Self::NativeStorage(_) => DefinitionKind::AssociatedType,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound(
    serialize = "I: DefinitionReference + serde::Serialize",
    deserialize = "I: DefinitionReference + serde::Deserialize<'de>"
))]
pub struct FieldDef<I = DefinitionPath> {
    pub name: String,
    pub ty: Ty<I>,
    pub mutable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound(
    serialize = "I: DefinitionReference + serde::Serialize",
    deserialize = "I: DefinitionReference + serde::Deserialize<'de>"
))]
pub struct VariantDef<I = DefinitionPath> {
    pub name: String,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub payload: Vec<Ty<I>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound(
    serialize = "I: DefinitionReference + serde::Serialize",
    deserialize = "I: DefinitionReference + serde::Deserialize<'de>"
))]
pub struct TraitDef<I = DefinitionPath> {
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub associated_consts: Vec<AssociatedConstDef<I>>,
    pub name: String,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub supertraits: Vec<NominalTy<I>>,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub generic_params: Vec<GenericParam<I>>,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub bounds: Vec<GenericBound<I>>,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub methods: Vec<FnDecl<I>>,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub associated_types: Vec<AssociatedTypeDef<I>>,
    /// Installed native storage capability of this interface. Ordinary source
    /// traits have no storage capability; declarations and loading retain it.
    pub storage_access: Option<CollectionAccess>,
    /// Adapter authority carried from installed native library declarations.
    pub conversion_adapter: Option<conversion::ConversionAdapter<I>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound(
    serialize = "I: DefinitionReference + serde::Serialize",
    deserialize = "I: DefinitionReference + serde::Deserialize<'de>"
))]
pub struct AssociatedConstDef<I = DefinitionPath> {
    pub declaration: I,
    pub ty: Ty<I>,
    pub default_value: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound(
    serialize = "I: DefinitionReference + serde::Serialize",
    deserialize = "I: DefinitionReference + serde::Deserialize<'de>"
))]
pub struct AssociatedTypeDef<I = DefinitionPath> {
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub generic_params: Vec<GenericParam<I>>,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub parameter_bounds: Vec<GenericBound<I>>,
    pub declaration: I,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub bounds: Vec<Constraint<I>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound(
    serialize = "I: DefinitionReference + serde::Serialize",
    deserialize = "I: DefinitionReference + serde::Deserialize<'de>"
))]
pub struct AssociatedTypeFamily<I = DefinitionPath> {
    pub declaration: I,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub generic_params: Vec<GenericParam<I>>,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub bounds: Vec<GenericBound<I>>,
    pub value: Ty<I>,
}
