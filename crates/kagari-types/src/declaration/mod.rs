//! Portable semantic declarations and symbolic method defaults.
pub mod applications;
pub mod conversion;
pub mod fingerprint;
pub mod host;
mod mapping;
pub mod module;
pub mod names;
pub mod native;
pub mod ownership;
pub mod path;
pub mod requirement;
pub mod verify;

#[doc(hidden)]
pub mod wire;
use crate::{
    callable::{CallableImplementation, MethodPolicy},
    declaration::{
        host::{HostCallPolicy, HostFieldPolicy, HostTypePolicy},
        native::{NativeStorageLayout, NativeTypeConstructor},
        requirement::NativeCallableRequirement,
    },
    ty::{Constraint, GenericBound, GenericParam, NominalTy, Ty},
    visibility::Visibility,
};
use kagari_common::identity::{DefinitionKind, table::DefinitionId};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FnDecl {
    /// Checked external-ID access policy; signatures remain ordinary params/result.
    pub host_policy: Option<HostCallPolicy>,
    pub method_policy: MethodPolicy,
    pub name: String,
    pub implementation: CallableImplementation,
    pub generic_params: Vec<GenericParam>,
    pub bounds: Vec<GenericBound>,
    pub params: Vec<Param>,
    pub return_type: Ty,
}

/// Declaration contract for native entrypoints, including inherent and private
/// methods absent from the public ABI. The function uses the same checked model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeDeclaration {
    /// Concrete value produced by Rust before a checked interface-result adapter.
    pub concrete_result: Option<Ty>,
    pub declaration: DefinitionId,
    pub function: FnDecl,
    pub callable_requirements: Vec<NativeCallableRequirement>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Param {
    pub name: String,
    pub ty: Ty,
    pub mutable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConstDef {
    pub name: String,
    pub ty: Ty,
    pub value: String,
}

/// A shared type schema retained with its nominal identity across dependencies.
/// For an external `game::Player`, the ID names that Struct declaration while
/// `definition` owns its fields/access policy. This carries no Rust layout or proof.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeDeclaration {
    /// Nominal identity of the shared type schema, including private members.
    pub declaration: DefinitionId,
    pub definition: TypeDef,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeDef {
    pub name: String,
    pub kind: TypeDefKind,
    pub generic_params: Vec<GenericParam>,
    pub bounds: Vec<GenericBound>,
    pub fields: Vec<FieldDef>,
    pub variants: Vec<VariantDef>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TypeDefKind {
    Struct,
    Enum,
    Native(NativeTypeConstructor),
    NativeStorage(NativeStorageLayout),
    /// Address-free external object identity with declared manual path access.
    Host(HostTypePolicy),
}

impl TypeDefKind {
    /// Nominal identity belongs to the declaration kind, independent of its producer.
    pub fn definition_kind(self) -> DefinitionKind {
        match self {
            Self::Struct | Self::Host(_) => DefinitionKind::Struct,
            Self::Enum => DefinitionKind::Enum,
            Self::Native(constructor) => constructor.declaration_kind(),
            Self::NativeStorage(_) => DefinitionKind::AssociatedType,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldDef {
    /// Specialized external path/read policy, valid only on a Host declaration.
    pub host_policy: Option<HostFieldPolicy>,
    pub visibility: Visibility,
    pub name: String,
    pub ty: Ty,
    pub mutable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VariantDef {
    /// Library-authored failure reporting captures diagnostic origin on construction.
    pub reports_failure: bool,
    pub name: String,
    pub payload: Vec<Ty>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraitDef {
    pub associated_consts: Vec<AssociatedConstDef>,
    pub name: String,
    pub supertraits: Vec<NominalTy>,
    pub generic_params: Vec<GenericParam>,
    pub bounds: Vec<GenericBound>,
    pub methods: Vec<FnDecl>,
    pub associated_types: Vec<AssociatedTypeDef>,
    /// Adapter authority carried from installed native library declarations.
    pub conversion_adapter: Option<conversion::ConversionAdapter>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssociatedConstDef {
    pub declaration: DefinitionId,
    pub ty: Ty,
    pub default_value: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssociatedTypeDef {
    pub generic_params: Vec<GenericParam>,
    pub parameter_bounds: Vec<GenericBound>,
    pub declaration: DefinitionId,
    pub bounds: Vec<Constraint>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssociatedTypeFamily {
    pub declaration: DefinitionId,
    pub generic_params: Vec<GenericParam>,
    pub bounds: Vec<GenericBound>,
    pub value: Ty,
}
