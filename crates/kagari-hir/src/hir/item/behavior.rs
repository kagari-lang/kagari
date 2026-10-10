//! Trait/implementation surfaces, generic binders and links to method functions.

use crate::hir::ids::{
    ConstId, FunctionId, GenericParamId, ImplId, MethodId, StructId, TraitId, TraitMethodId,
    TypeRefId,
};
use kagari_types::visibility::Visibility;

/// The currently single receiver category, separate from parameter writeability.
///
/// `trait R { fn run(self); }` produces `TraitMethod.receiver = Value` and a
/// function parameter named "self". There are no `&self`/`&mut self` categories.
/// Current lowering writes `Value` even for a trait method without `self`; inspect
/// `Function.params` to establish whether a receiver parameter exists. Ordinary
/// value semantics determine copying versus shared identity, not this tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReceiverKind {
    /// A `self` receiver; static writeability/host access checks remain separate.
    Value,
}

/// Intended owner of a record in the currently unpopulated method registry.
///
/// `Struct(s)` would select `Module.structs[s.index()]`; `Trait(t)` would select
/// `Module.traits[t.index()]`. These are model alternatives, not the output of
/// current method lowering: actual members live in `TraitDef.methods` and
/// `Impl.methods`, linked to function rows. No current source example populates
/// `MethodOwner`; do not infer that `impl S { ... }` creates this record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MethodOwner {
    /// A struct-owned method.
    Struct(StructId),
    /// A trait-owned method.
    Trait(TraitId),
}

/// An unused unified method surface intended for `Module.methods`.
///
/// Hypothetical registry record, not current lowering output:
///
/// ```text
/// Method { id: m, owner: Struct(s), visibility: Public, name: "run",
///          receiver: Value, function: f }
/// m -> Module.methods[m.index()]
/// s -> Module.structs[s.index()]
/// f -> Module.functions[f.index()] -> signature and optional body
/// ```
///
/// `id`/`owner` would be allocated links; visibility/name/receiver would describe
/// the member surface, and `function` would supply its function syntax.
/// Current `impl S { pub fn run(self) {} }` instead creates an `ImplMethod`
/// with `function: f`; visibility resides in that function. Trait members use
/// `TraitMethod`. The registry is a deferred cleanup candidate, not another
/// active semantic implementation.
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

/// A trait's inline member declarations, generic inputs and parent requirements.
///
/// ```text
/// pub trait Reader<T>: Parent {
///     const LIMIT: i32 = 10;
///     type Item: Display;
///     fn read(self) -> Self::Item;
/// }
/// TraitDef {
///     id: r, visibility: Public, name: "Reader",
///     generic_params: [GenericParam { id: g, name: "T", bounds: [] }],
///     supertraits: [TraitRef { ty: parent_type }],
///     associated_consts: [limit], associated_types: [item], methods: [read],
/// }
/// parent_type -> Body.type_ref -> Named("Parent")
/// limit / item -> inline AssociatedConst / AssociatedType, explained below
/// read -> inline TraitMethod { has_default: false, function: f, ... }
/// f -> Module.functions[f.index()] -> Function { kind: TraitMethod, body: None, ... }
/// ```
///
/// This fragment illustrates storage; the parent/output traits must be declared.
/// `r` is the allocated slot in `Module.traits` and its source map, not source
/// syntax. Omitted `pub`/generics/parents select private visibility and empty
/// buffers. Methods/associated items remain in their source order within each
/// collection; there is no single mixed member-order vector here.
///
/// A method body sets its member's `has_default` and function `body`; constants
/// can likewise have defaults. An associated type declaration normally leaves
/// its assigned `ty` absent. Signature/aggregate checking validates parent and
/// member contracts; this record alone does not establish an implementation.
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

/// A trait/impl constant member with a value-type annotation and optional definition.
///
/// ```text
/// trait Limit { const VALUE: i32 = 10; }
/// AssociatedConst { name: "VALUE", name_ref: n, ty: t, initializer: Some(c) }
/// n -> Body.type_ref -> Named("VALUE")  // synthetic NAME site, not the value type
/// t -> Body.type_ref -> Named("i32")
/// c -> Module.constant(c) -> ConstItem { owner: Some(Trait(limit)), initializer: e, ... }
/// e -> Body.expr -> Literal { kind: Number, text: "10" }
/// ```
///
/// The member is inline in `TraitDef.associated_consts` or `Impl.associated_consts`.
/// `name` comes from the identifier; lowering synthesizes `name_ref` at that name's
/// source span for diagnostics. `ty` corresponds to `: i32`. `initializer` links
/// to a complete constant record, not directly to an `ExprId` or evaluated value.
///
/// `const VALUE: i32;` in a trait has `initializer: None` and requires an impl
/// definition. `impl Limit for S { const VALUE: i32 = 20; }` stores `Some(c)` with
/// `ConstOwner::Impl`, overriding the default. Checking requires annotations and
/// the supported const-safe scalar semantics; allocating this HIR is not proof
/// that a default expression is legal.
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

/// A trait output type or type family, stored inline in trait/impl member collections.
///
/// | Field | `trait R { type Item: Display; }` | `impl R for S { type Item = i32; }` |
/// | --- | --- | --- |
/// | `name` | "Item" | "Item" |
/// | `name_ref` | Synthetic type node at the declaration's Item name | Same, at the definition's name |
/// | `ty` | `None`: implementation must supply the type | `Some(t)`, with `Body.type_ref(t) = Named("i32")` |
/// | `bounds` | `[TraitRef { ty: display_type }]` | Empty in this example |
/// | `generic_params` | Empty | Empty |
/// | `parameter_bounds` | Empty | Empty |
///
/// `display_type` points to `Named("Display")` in `Body.types`. `name_ref` points
/// to a synthesized `Named("Item")`, used for declaration association and source
/// diagnostics; it is not the assigned type or a resolved projection.
///
/// Member generics and output constraints are different:
///
/// ```text
/// type Item<T: PartialEq>: PartialEq where T: Display;
/// generic_params = [GenericParam { id: g, name: "T", bounds: [partial_eq_ref] }]
/// parameter_bounds = [TraitBound { target: "T", target_ref: t, traits: [display_ref] }]
/// bounds = [partial_eq_ref_for_output]
/// ty = None; name = "Item"; name_ref = synthetic_item_name
/// ```
///
/// Inline binder bounds and `parameter_bounds` constrain INPUT T; `bounds`
/// constrains OUTPUT `Item<T>`. An impl's `type Item<U> = U where U: PartialEq;`
/// supplies its own member binders, `ty: Some(u_type)` and where requirements.
/// Such fragments require matching declared traits/impl contracts to type-check.
///
/// Lowering can retain assigned type syntax on a trait declaration, but current
/// checking rejects associated type defaults; `ty: Some` is not evidence of
/// default support. Signature/aggregate analysis resolves and substitutes these
/// syntax IDs before executable lowering; there is no runtime type-family lookup.
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
    /// Assigned type syntax; trait defaults can be retained but are currently rejected.
    pub ty: Option<TypeRefId>,
    /// Trait requirements on the associated output.
    pub bounds: TraitRefBuffer,
}

/// An inline trait member linking to a function signature and optional default body.
///
/// ```text
/// trait Reader { fn read(self) -> i32; }
/// TraitMethod { has_default: false, id: m, name: "read", receiver: Value, function: f }
/// m -> SourceMap.trait_method_span(m)
/// f -> Module.functions[f.index()] -> Function { kind: TraitMethod, body: None, ... }
/// ```
///
/// Replacing `;` with `{ 1 }` sets `has_default: true` and `Function.body: Some(b)`.
/// `id` is a source-map-wide trait-method identity, not its ordinal within one
/// trait. `name` is written syntax; `receiver` is currently the constant `Value`
/// category (the function parameters establish receiver presence). Generics,
/// visibility, parameter/return types and body live in the function row.
/// Trait checking and method lookup consume this link, not `Module.methods`.
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

/// An impl header and inline member definitions, stored in `Module.impls`.
///
/// ```text
/// impl<T> Reader for Box<T> where T: Display {
///     const LIMIT: i32 = 20;
///     type Item = T;
///     fn read(self) -> T { self.value }
/// }
/// Impl {
///     id: i, generic_params: [GenericParam { id: g, name: "T", bounds: [] }],
///     trait_ref: Some(TraitRef { ty: reader_type }), for_type: Some(box_type),
///     bounds: [TraitBound { target: "T", target_ref: t, traits: [display_ref] }],
///     associated_consts: [limit], associated_types: [item],
///     methods: [ImplMethod { name: "read", function: f }],
/// }
/// reader_type -> Body.type_ref -> Named("Reader")
/// box_type -> Body.type_ref -> Generic { name: "Box", args: [T_type], ... }
/// f -> Module.functions[f.index()] -> Function { kind: ImplMethod, body: Some(b), ... }
/// ```
///
/// This is a source-shape example; Box/Reader/Display and their members must be
/// declared consistently. `i` is allocated, `for_type` encodes the type after
/// `for`, and `trait_ref` encodes the preceding trait. For `impl Box<i32> { ... }`,
/// the target is still present but `trait_ref: None` denotes an inherent impl.
/// A missing `for_type` is recovery, not another legal impl category. Omitted
/// generics/where clauses and absent member kinds leave empty buffers.
///
/// Method/constant bodies occupy shared function/constant collections; associated
/// types remain inline syntax. Aggregate/type analysis validates implementations
/// and supplies lookup facts. Struct/enum `methods` and `impls` fields are not
/// populated by this path.
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

/// An inline impl member's name and link to its shared function record.
///
/// ```text
/// impl Point { pub fn count(self) -> i32 { self.x } }
/// Impl.methods = [ImplMethod { name: "count", function: f }]
/// f -> Module.functions[f.index()]
///   -> Function { kind: ImplMethod, visibility: Public, name: "count", body: Some(b), ... }
/// ```
///
/// The member has no separate MethodId or body. Signature, visibility, async flag,
/// inherited/method generic binders and receiver parameters all live in Function.
/// Method lookup connects this record to the containing impl's target/trait.
#[derive(Debug, Clone)]
pub struct ImplMethod {
    /// Implementation member name.
    pub name: String,
    /// Slot of the corresponding method function.
    pub function: FunctionId,
}

/// A generic input binder, stored inline in its declaring item/member/function.
///
/// ```text
/// fn consume<T: Display + Copy>(x: T) {}
/// GenericParam { id: g, name: "T", bounds: [display_ref, copy_ref] }
/// display_ref.ty -> Body.type_ref -> Named("Display")
/// copy_ref.ty -> Body.type_ref -> Named("Copy")
/// g -> SourceMap.generic_param_span(g)
/// ```
///
/// The name is source syntax, `id` is a source-map-wide allocated identity, and
/// `bounds` retains the `:` list in order. A bare `<T>` has empty bounds.
/// Use `TraitBound` for trailing `where` predicates instead. Semantic checking
/// identifies a binder by its declaration context; a type named "T" is still
/// unresolved until that stage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenericParam {
    /// Source-map-wide generic-parameter slot.
    pub id: GenericParamId,
    /// Binder name as written.
    pub name: String,
    /// Inline unresolved trait references.
    pub bounds: TraitRefBuffer,
}

/// One where-clause predicate, with target syntax separate from trait requirements.
///
/// ```text
/// where T: Display + Copy
/// TraitBound { target: "T", target_ref: t, traits: [display_ref, copy_ref] }
/// t -> Body.type_ref -> Named("T")
/// ```
///
/// `target` is a diagnostic spelling; `target_ref` is the complete syntax used by
/// resolution (for `where T::Item: Display`, it retains that type path). `traits`
/// holds one `TraitRef` per required trait. These records belong to function,
/// impl, native-type or associated-member where buffers, not a global bound arena.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraitBound {
    /// Target spelling retained for diagnostics.
    pub target: String,
    /// Type-syntax handle used to resolve the bound target.
    pub target_ref: TypeRefId,
    /// Required trait references in source order.
    pub traits: TraitRefBuffer,
}

/// A trait application encoded by one unresolved type-syntax handle.
///
/// ```text
/// Reader<i32, Item = String>
/// TraitRef { ty: t }
/// Body.type_ref(t) -> Generic {
///     name: "Reader", args: [i32_type], bindings: [("Item", string_type)],
///     positional_after_binding: false, callable_syntax: false,
/// }
/// ```
///
/// The single field retains the WHOLE application; it is not a resolved TraitId.
/// A bare `Display` points to `Named("Display")`. Callable-trait notation such
/// as `Fn(i32) -> bool` is encoded as a generic application with one synthetic
/// tuple argument, an Output binding and `callable_syntax: true`; see TypeKind.
/// Trait headers/binders/where predicates/impl headers own these inline wrappers.
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
