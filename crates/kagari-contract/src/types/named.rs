//! Authored named contracts retained independently of executable reachability.

pub mod correspondence;
mod mapping;
pub mod origins;
pub mod verify;
#[doc(hidden)]
pub mod wire;

use crate::types::InterfaceBridge;
use kagari_common::identity::table::DefinitionId;
use kagari_types::{
    declaration::{ConstDef, FnDecl},
    ty::{GenericBound, GenericParam, Ty},
    visibility::Visibility,
};

/// A source declaration, not an executable placement or public export capability.
/// For `fn outer() { fn local<T>(x: T) -> T { x } }`, `local` keeps its authored
/// identity under `outer`, even when a default body is placed in another module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamedDeclaration {
    pub declaration: DefinitionId,
    pub visibility: Visibility,
    /// Exact enclosing declaration text for ambiguous same-name locals only.
    /// Reload also requires unchanged indexed executable evidence for that scope;
    /// text alone never proves executable correspondence in a source-free artifact.
    pub ambiguous_local_body: Option<String>,
    pub contract: NamedContract,
}

/// An imported named local's executable placement, separate from its authored ID.
/// Both IDs are explicit portable references. The linked verifier checks the
/// authored local declaration and the placed body's instantiated signature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionOrigin {
    pub placement: DefinitionId,
    pub authored: DefinitionId,
}

/// The contract of a function, constant, transparent alias or implementation.
/// Nominal definitions and traits retain their existing owning record families.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NamedContract {
    Function(FnDecl),
    Constant(ConstDef),
    Alias(AliasContract),
    Implementation(ImplementationContract),
}

/// A normalized transparent alias, including bounds on unused parameters.
/// `type Erased<T: Tag> = i32` retains one binder and its `Tag` bound, although
/// neither appears in `target`. This record creates no nominal type identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AliasContract {
    pub name: String,
    pub generic_params: Vec<GenericParam>,
    pub bounds: Vec<GenericBound>,
    pub target: Ty,
}

/// A checked impl header; member identities descend from its declaration ID.
/// Correspondence compares receiver/trait/bounds after replacing this owner's
/// binder identity, never the source-order occurrence assigned during analysis.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImplementationContract {
    /// Synthesized checked bridges use a distinct correspondence domain. Authored
    /// inventory entries must use None; bridge records come from verified tables.
    pub bridge: Option<InterfaceBridge>,
    pub generic_params: Vec<GenericParam>,
    pub bounds: Vec<GenericBound>,
    pub receiver: Ty,
    pub trait_type: Option<Ty>,
}
