//! Function declarations, inline parameters and method origin tags.

use crate::hir::{
    ids::{BlockId, FunctionId, GenericParamId, ParamId, TypeRefId},
    item::behavior::TraitBound,
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{DefinitionMapper, DefinitionMappingError},
        scoped::{DefinitionReferences, RemapDefinitions},
        table::DefinitionId,
    },
};
use kagari_types::{
    declaration::{host::HostCallPolicy, requirement::NativeCallableRequirement},
    reference::ReferenceAccess,
    ty::Ty,
    visibility::Visibility,
};

/// A function declaration's signature and implementation, stored in `Module.functions`.
///
/// ```text
/// pub fn echo<T: PartialEq>(x: T) -> T where T: Eq { x }
/// Function {
///     id: f, kind: User, is_async: false, visibility: Public, name: "echo",
///     generic_params: [g],
///     bounds: [TraitBound { target: "T", target_ref: t_where, traits: [eq_ref] }],
///     params: [Param { id: p, name: "x", ty: t_param }],
///     return_type: Some(t_result), implementation: Script(b),
/// }
/// t_where, t_param, t_result -> HirNodes.type_ref -> TypeKind::Named("T")
/// partial_eq_ref / eq_ref -> TraitRef { ty: ... } -> trait type syntax
/// b -> HirNodes.block -> BlockData { statements: [], tail_expr: Some(x_expr) }
/// x_expr -> HirNodes.expr -> Name { name: "x", explicit_type: None }
/// ```
///
/// The traits require matching declarations from the installed foundation.
/// Symbolic IDs are allocator outputs, not written syntax or promised indices.
/// `f.index()` selects the function row and matching source-map slot; the stored
/// `id` repeats that identity for consumers holding only `&Function`.
///
/// Omitting `pub` selects `Private`; omitting generics/`where` leaves those buffers
/// empty. Omitting `-> T` sets `return_type` to `None` (checked as unit), whereas
/// `-> ()` stores `Some(unit_type)`. `async fn` sets `is_async`, while its written
/// result type still describes the completed body. Trait requirements use `Required`;
/// installed native entries use `Native`. An empty `{}` is `Script(empty_block)`.
/// Missing bodies in recovered syntax also use `Required`, without making them legal.
///
/// Method signatures/bodies use this same collection: their `kind` is
/// `TraitMethod` or `ImplMethod`, with an external trait/impl member linking to `f`.
/// Impl binders can be prepended to method binders, and implicit `self` type syntax
/// is supplied by lowering. Ordinary function parameters use `ParamId`; closure
/// parameters use `LocalId` in `ExprKind::Closure`. Named local `fn` declarations
/// use `Local` and retain their declaring block without capturing its values.
///
/// Lowering assigns `HirOwner::Body(BodyOwner::Function(f))` to signature/body
/// nodes. Type annotations enter `HirNodes.types`, not the semantic type table;
/// resolution, checking and compiler lowering consume these source records later.
#[derive(Debug, Clone)]
pub struct Function {
    /// Slot in the enclosing module's function collection.
    pub id: FunctionId,
    /// Module, local, trait method or implementation method origin.
    pub kind: FunctionKind,
    /// Explicit async factory; the written return type describes its completed body.
    pub is_async: bool,
    /// Declared source visibility, enforced by later lookup.
    pub visibility: Visibility,
    /// Declared name; may be empty in recovered syntax.
    pub name: String,
    /// Generic binders; method lowering can prepend inherited binders.
    pub generic_params: Vec<GenericParamId>,
    /// Where-clause requirements before trait/type resolution.
    pub bounds: Vec<TraitBound>,
    /// Inline parameter records in call order.
    pub params: Vec<Param>,
    /// Explicit result type syntax, absent when omitted.
    pub return_type: Option<TypeRefId>,
    /// Required member, authored script block or native declaration contract.
    pub implementation: FunctionImplementation,
}

/// Structural implementation owned by a function declaration, not a runtime binding.
///
/// A native declaration alone grants no installation authority. Real forwarding
/// defaults remain ordinary Script bodies and their calls are checked normally.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FunctionImplementation {
    /// A trait member awaiting an implementation, or a missing body during recovery.
    Required,
    /// A script block in the enclosing module's body table.
    Script(BlockId),
    /// A native declaration with its static operation/result requirements.
    Native(NativeContract),
}

impl FunctionImplementation {
    /// Returns the authored block only for a script implementation.
    pub fn script_body(&self) -> Option<BlockId> {
        match self {
            Self::Script(body) => Some(*body),
            Self::Required | Self::Native(_) => None,
        }
    }
}

/// Static requirements supplied by a native declaration, without binding authority.
///
/// These reuse the existing scoped declaration vocabulary: definition identities
/// and generic binders belong to the enclosing analysis's retained definition
/// table. External sidecars import that scope before attachment, without acquiring
/// registration authority. Requirements are ordered slots; repeated operations remain distinct.
/// Parameter/result syntax stays on `Function`; receiver access is caller-visible.
/// Rust adapters and internal payload loans remain outside this declaration.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NativeContract {
    pub host_policy: Option<HostCallPolicy>,
    /// Caller-visible scoped receiver access required by this operation.
    pub receiver_access: Option<ReferenceAccess>,
    /// Operations required by the native algorithm, including applied trait/member arguments.
    pub callable_requirements: Vec<NativeCallableRequirement>,
    /// Concrete result before the existing checked interface-result adaptation.
    pub concrete_result: Option<Ty>,
}

impl DefinitionReferences for NativeContract {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        self.callable_requirements
            .visit_definition_ids(visit, cancel)?;
        if let Some(result) = &self.concrete_result {
            result.visit_definition_ids(visit, cancel)?;
        }
        Ok(())
    }
}

impl RemapDefinitions for NativeContract {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        Ok(Self {
            host_policy: self.host_policy.clone(),
            receiver_access: self.receiver_access,
            callable_requirements: self.callable_requirements.remap_definitions(mapper)?,
            concrete_result: self
                .concrete_result
                .as_ref()
                .map(|result| result.remap_definitions(mapper))
                .transpose()?,
        })
    }
}

/// Declaration origin used by namespace collection and method-specific checking.
///
/// | Source location | Tag | Owning link |
/// | --- | --- | --- |
/// | `fn run() {}` at module level | `User` | `Item::Function(f)` |
/// | `trait R { fn run(self); }` or a default body | `TraitMethod` | `TraitMethod.function = f` |
/// | `impl R for S { fn run(self) {} }` | `ImplMethod` | `ImplMethod.function = f` |
///
/// | `fn run() {}` inside a block | `Local { block }` | `StmtKind::Function(f)` |
///
/// All origins use `Module.functions`. This tag neither identifies the particular
/// trait/impl nor chooses the runtime calling convention. `User` also covers
/// declaration-only installed functions; a closure remains an expression.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FunctionKind {
    /// A named block-local function; the block carries the enclosing body owner.
    Local { block: BlockId },
    /// A free `fn` declaration, including a declaration-only installed function.
    User,
    /// A trait requirement or default method.
    TraitMethod,
    /// A method declared inside an implementation block.
    ImplMethod,
}

/// A function parameter stored inline in `Function.params`, in source order.
///
/// ```text
/// fn add(x: i32) -> i32 { x + 1 }
/// Param { id: p, name: "x", ty: t }
/// t -> HirNodes.type_ref(t) -> TypeKind::Named("i32")
/// p -> SourceMap.param_span(p); resolved uses of x refer to Param(p)
/// ```
///
/// `id` is allocated, not derived from the spelling. Its index is the position in
/// the owning function's retained `params` list; `Module.parameter(id)` checks the
/// lowering, function owner and stored identity. Parameters cannot be rebound;
/// shared objects can still be mutated through their permitted fields/operations.
/// For a method's `self`, `name` is "self" and lowering supplies its type syntax.
/// Missing required annotations recover with a placeholder type rather than an
/// inference-enabled `None`.
#[derive(Debug, Clone)]
pub struct Param {
    /// Arena/function-qualified identity whose index is its position in `Function.params`.
    pub id: ParamId,
    /// Source name, including `self` where applicable.
    pub name: String,
    /// Parameter type syntax; missing required types use a placeholder.
    pub ty: TypeRefId,
}
