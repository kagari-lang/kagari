//! Trait/implementation surfaces, generic binders and links to method functions.

use crate::hir::ids::{
    ConstId, FunctionId, GenericParamId, ImplId, MethodId, StructId, TraitId, TraitMethodId,
    TypeRefId,
};
use kagari_types::visibility::Visibility;

/// Receiver form retained by method lowering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReceiverKind {
    /// A `self` receiver; static writeability/host access checks remain separate.
    Value,
}

/// Declaration owning a method record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MethodOwner {
    /// A struct-owned method.
    Struct(StructId),
    /// A trait-owned method.
    Trait(TraitId),
}

/// A named method surface referring to a function stored in `Module.functions`.
#[derive(Debug, Clone)]
pub struct Method {
    /// Slot in `Module.methods`.
    pub id: MethodId,
    /// Enclosing struct or trait.
    pub owner: MethodOwner,
    /// Declared method visibility.
    pub visibility: Visibility,
    /// Method name within its owner.
    pub name: String,
    /// Receiver syntax category.
    pub receiver: ReceiverKind,
    /// Function slot containing signature/body syntax.
    pub function: FunctionId,
}

/// A trait declaration containing method, associated-item and parent syntax.
///
/// ```text
/// trait Reader { fn read(self) -> i32; }
/// Module.traits[t.index()] -> TraitDef { methods: [m], ... }
/// m: TraitMethod { function: f, has_default: false, ... }
/// Module.functions[f.index()] -> Function { kind: TraitMethod, body: None, ... }
/// ```
///
/// Methods have independent function slots. A default implementation has a body;
/// absence for a requirement is legal and is not an empty block.
#[derive(Debug, Clone)]
pub struct TraitDef {
    /// Associated constant signatures and optional defaults.
    pub associated_consts: Vec<AssociatedConst>,
    /// Slot in `Module.traits`.
    pub id: TraitId,
    /// Declared visibility.
    pub visibility: Visibility,
    /// Trait name.
    pub name: String,
    /// Declared generic binders.
    pub generic_params: GenericParamBuffer,
    /// Unresolved parent trait requirements.
    pub supertraits: TraitRefBuffer,
    /// Inline method surfaces pointing to function slots.
    pub methods: TraitMethodBuffer,
    /// Associated type/family declarations.
    pub associated_types: Vec<AssociatedType>,
}

/// An associated constant signature with an optional separately stored initializer.
#[derive(Debug, Clone)]
pub struct AssociatedConst {
    /// Associated constant name.
    pub name: String,
    /// Synthetic type-syntax handle preserving the member-name site.
    pub name_ref: TypeRefId,
    /// Declared value type syntax.
    pub ty: TypeRefId,
    /// Constant slot for an initializer/default, or absent for a requirement.
    pub initializer: Option<ConstId>,
}

/// An associated type/family declaration before substitution and member resolution.
#[derive(Debug, Clone)]
pub struct AssociatedType {
    /// Member-level generic binders.
    pub generic_params: GenericParamBuffer,
    /// Where constraints on member binders.
    pub parameter_bounds: TraitBoundBuffer,
    /// Associated member name.
    pub name: String,
    /// Synthetic type-syntax handle for the member-name site.
    pub name_ref: TypeRefId,
    /// Assigned/default type, when present.
    pub ty: Option<TypeRefId>,
    /// Trait requirements on the associated output.
    pub bounds: TraitRefBuffer,
}

/// A trait method surface linked to its function signature and optional default body.
#[derive(Debug, Clone)]
pub struct TraitMethod {
    /// Whether syntax supplied a default method body.
    pub has_default: bool,
    /// Source-map-wide trait-method identity, not the method's local ordinal.
    pub id: TraitMethodId,
    /// Method name within the trait.
    pub name: String,
    /// Receiver syntax category.
    pub receiver: ReceiverKind,
    /// Slot in `Module.functions` containing this method's signature/body.
    pub function: FunctionId,
}

/// An implementation block retaining its target, trait and member syntax.
///
/// ```text
/// impl Reader for Point { fn read(self) -> i32 { self.x } }
/// Module.impls[i.index()] -> Impl
/// +-- trait_ref: Some(TraitRef { ty: reader_type })
/// +-- for_type: Some(point_type)
/// `-- methods: [ImplMethod { name: "read", function: f }]
/// Module.functions[f.index()] -> Function { kind: ImplMethod, body: Some(b), ... }
/// ```
///
/// Trait matching, bounds and associated values are checked by aggregate/type
/// analysis. An inherent impl has no `trait_ref`; recovery may omit its target.
#[derive(Debug, Clone)]
pub struct Impl {
    /// Associated constant definitions retained in this impl.
    pub associated_consts: Vec<AssociatedConst>,
    /// Slot in `Module.impls`.
    pub id: ImplId,
    /// Implementation-level generic binders.
    pub generic_params: GenericParamBuffer,
    /// Implemented trait, absent for an inherent implementation.
    pub trait_ref: Option<TraitRef>,
    /// Target type syntax, possibly absent after recovery.
    pub for_type: Option<TypeRefId>,
    /// Implementation where-clause constraints.
    pub bounds: TraitBoundBuffer,
    /// Inline names referring to method function slots.
    pub methods: ImplMethodBuffer,
    /// Associated type/family definitions.
    pub associated_types: Vec<AssociatedType>,
}

/// An implementation member referring to a shared function slot.
#[derive(Debug, Clone)]
pub struct ImplMethod {
    /// Implementation member name.
    pub name: String,
    /// Slot of the corresponding method function.
    pub function: FunctionId,
}

/// A generic binder with unresolved trait bounds and a source-map identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenericParam {
    /// Source-map-wide generic-parameter slot.
    pub id: GenericParamId,
    /// Binder name as written.
    pub name: String,
    /// Inline unresolved trait references.
    pub bounds: TraitRefBuffer,
}

/// A where-clause target and its unresolved trait requirements.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraitBound {
    /// Target spelling retained for diagnostics.
    pub target: String,
    /// Type-syntax handle used to resolve the bound target.
    pub target_ref: TypeRefId,
    /// Required trait references in source order.
    pub traits: TraitRefBuffer,
}

/// A trait requirement represented by a type-syntax ID, not a resolved trait identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraitRef {
    /// Type-syntax handle encoding the trait application and associated bindings.
    pub ty: TypeRefId,
}

/// Ordered `Vec<Method>` storage for the owning declaration records.
pub type MethodBuffer = Vec<Method>;
/// Ordered `Vec<TraitDef>` storage for the owning declaration records.
pub type TraitBuffer = Vec<TraitDef>;
/// Ordered `Vec<TraitMethod>` storage for the owning declaration records.
pub type TraitMethodBuffer = Vec<TraitMethod>;
/// Ordered `Vec<Impl>` storage for the owning declaration records.
pub type ImplBuffer = Vec<Impl>;
/// Ordered `Vec<ImplMethod>` storage for the owning declaration records.
pub type ImplMethodBuffer = Vec<ImplMethod>;
/// Ordered `Vec<GenericParam>` storage for the owning declaration records.
pub type GenericParamBuffer = Vec<GenericParam>;
/// Ordered `Vec<TraitBound>` storage for the owning declaration records.
pub type TraitBoundBuffer = Vec<TraitBound>;
/// Ordered `Vec<TraitRef>` storage for the owning declaration records.
pub type TraitRefBuffer = Vec<TraitRef>;
