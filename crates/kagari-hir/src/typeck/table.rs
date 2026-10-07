//! Node-keyed types and selected semantic operations, shared by tooling and compiler lowering.

#[cfg(test)]
use crate::hir::ids::HirArenaId;
use crate::{
    builtin::BuiltinFunction,
    callable::AppliedCallSignature,
    hir::ids::{
        EnumId, ExprId, FieldId, FunctionId, GenericParamId, LocalId, OpaqueTypeId, PatternId,
        PlaceId, StructId, TraitId, TypeRefId,
    },
    host::{HostFunctionId, HostTypeId},
    language::semantics as traits,
    source_map::SourceMap,
    typeck::{constraints, scalar::ScalarValue, table::propagation::ResolvedPropagation},
    types::{
        AssociatedTypeFamily, AssociatedTypeParameters, GenericParameterType, NominalType, TypeId,
        TypeSubstitution,
    },
};
use kagari_common::{
    identity::{DefinitionPath, reference::DefinitionReference},
    span::Span,
};
use kagari_types::{
    host_interface::path::{HostPathContract, HostPathDeclaration},
    language::Protocol,
    surface::StandardTypeConstraint,
};
use std::collections::{HashMap, HashSet};

/// A checked standard capability or applied nominal trait requirement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConstraintTarget<I: DefinitionReference = DefinitionPath> {
    /// An intrinsic standard capability constraint.
    Standard(StandardTypeConstraint),
    /// A nominal trait application, including arguments and associated constraints.
    Trait(NominalType<I>),
}

/// Declaration/binder identity selected for a source type reference, separate from its resulting type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeTarget<I: DefinitionReference = DefinitionPath> {
    /// Local native-backed type slot.
    OpaqueType(OpaqueTypeId),
    /// Installed host type slot.
    Host(HostTypeId),
    /// Canonical source type definition.
    Source(I),
    /// Canonical associated type/family definition.
    AssociatedType(I),
    /// Local struct slot.
    Struct(StructId),
    /// Local enum slot.
    Enum(EnumId),
    /// Local trait slot.
    Trait(TraitId),
    /// Local generic-parameter source slot.
    Generic(GenericParamId),
}

/// A semantic type plus an optional navigable declaration/binder target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedTypeRef<I: DefinitionReference = DefinitionPath> {
    /// Resolved or recovered semantic type.
    pub ty: TypeId<I>,
    /// Navigation target; structural/intrinsic type forms may have no declaration target.
    pub target: Option<TypeTarget<I>>,
}

/// Selected callable category, distinct from the callee expression's name binding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallTarget<I: DefinitionReference = DefinitionPath> {
    /// The recorded receiver is the callee expression. Its evaluation exits
    /// before any callable value or explicit argument can be produced.
    TerminatingCallee,
    /// A canonical semantic function definition, including cross-module calls.
    SourceFunction(I),
    /// An installed host callable identified by its host declaration slot.
    HostFunction(HostFunctionId),
    /// A function slot in the matching lowered module.
    Function(FunctionId),
    /// A callable value whose callee expression must be evaluated.
    Value,
    /// A recognized runtime helper with its special operand contract.
    RuntimeHelper(BuiltinFunction),
    /// A selected trait method through an applied interface.
    TraitMethod {
        /// Canonical method definition identity.
        method: I,
        /// Applied interface used for the method selection.
        interface: NominalType<I>,
    },
}

/// Type-checking result for one call/operator/protocol expression.
///
/// ```text
/// ExprKind::Call { callee: name_id, args: [arg_id], ... }
/// ResolvedNames.expr_resolution(name_id) -> source/function binding
/// TypeTable.call_resolution(call_id) -> ResolvedCall
/// +-- target: selected callable identity/category
/// +-- receiver: optional expression evaluated once before explicit arguments
/// +-- type_arguments: substituted types in declaration order
/// `-- signature: applied parameter/result contract, when available
/// ```
///
/// A method receiver and a callable-value callee are represented explicitly for
/// evaluation order. The applied parameter types need not equal argument expression
/// types after coercions. This record is semantic input to compiler lowering, not
/// a runtime function pointer or executed call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedCall<I: DefinitionReference = DefinitionPath> {
    /// Selected callable category and canonical/local identity.
    pub target: CallTarget<I>,
    /// Evaluated before explicit arguments, exactly once.
    pub receiver: Option<ExprId>,
    /// Declaration parameter order; arguments may refer to an enclosing binder.
    pub type_arguments: Vec<TypeId<I>>,
    /// Absent for incomplete calls, terminating callees and language helpers
    /// whose operands are not an ordinary callable signature.
    pub signature: Option<AppliedCallSignature<I>>,
}

/// Selected nominal constructor and field identities aligned to source initializer order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedStructInit<I: DefinitionReference = DefinitionPath> {
    /// Canonical struct definition identity.
    pub structure: I,
    /// Source order, including holes for unknown initializer fields.
    pub fields: Vec<Option<I>>,
}

/// Known enum owner and optional selected variant for construction/recovery.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedEnumConstructor<I: DefinitionReference = DefinitionPath> {
    /// Canonical enum definition identity.
    pub enumeration: I,
    /// Missing members keep their known enum owner for error recovery.
    pub variant: Option<I>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TraitImplementation<I: DefinitionReference = DefinitionPath> {
    declaration: I,
    parameters: Vec<GenericParameterType<I>>,
    bounds: super::GenericBounds<I>,
    methods: HashMap<I, FunctionId>,
}

/// Applied Iterable/Iterator contracts selected for a `for` iterable expression.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedIteration<I: DefinitionReference = DefinitionPath> {
    /// Interface used to obtain the iterator.
    pub into_interface: NominalType<I>,
    /// Semantic iterator value type.
    pub iterator: TypeId<I>,
    /// Interface used to obtain each next element.
    pub next_interface: NominalType<I>,
    /// Semantic element type bound by the loop pattern.
    pub item: TypeId<I>,
}

/// Semantic side tables keyed by IDs from one matching lowered module.
///
/// Signature checking writes type-reference/field/constraint facts; body checking
/// adds expression/local/place types and selected operations. Compiler lowering and
/// tooling read these facts instead of resolving names or inferring types again.
///
/// | Keys | Facts | Writer / typical reader |
/// | --- | --- | --- |
/// | `TypeRefId`, `FieldId` | resolved types, constraints, field types | signature/type resolution / diagnostics and member checking |
/// | `ExprId`, `LocalId`, `PlaceId` | value/binding/place types | body checker / tooling and compiler |
/// | `ExprId` | calls, fields, constructors, coercions, iteration, propagation, constants | specialized body checks / compiler lowering |
/// | `PlaceId` | selected fields, index interface, declared host path | mutation checks / compiler lowering |
/// | `PatternId` | scalar/range/field/variant facts | pattern checker / match lowering |
/// | definition identity | associated bounds/family inputs/values | trait/signature checking / substitution and selection |
///
/// An absent map entry means no fact was recorded: the construct may be inapplicable,
/// unvisited or erroneous. A recorded `TypeId::Error` is distinct from absence.
/// Constraint storage also distinguishes an attempted unresolved constraint from
/// an unvisited one, although the public getter flattens both to `None`.
///
/// Temporary inference holes and recursion guards are checker state, removed/cleared
/// before publication. Semantic definition mapping does not remap HIR arenas; body
/// and signature reuse use explicit source-map remappers for local IDs.
#[derive(Debug, Clone, PartialEq)]
pub struct TypeTable<I: DefinitionReference = DefinitionPath> {
    /// Temporary body-local placeholders; removed before publishing facts.
    pub(super) inference_holes: HashMap<TypeRefId, TypeId<I>>,
    iterations: HashMap<ExprId, ResolvedIteration<I>>,
    propagations: HashMap<ExprId, ResolvedPropagation<I>>,
    protocol_receivers: HashMap<ExprId, TypeId<I>>,
    associated_consts: HashMap<ExprId, ResolvedAssociatedConst<I>>,
    /// Active type-resolution recursion guard; an ID here is being resolved, not a published type fact.
    pub(super) resolving_types: HashSet<TypeRefId>,
    /// Associated member identities mapped to declared constraint targets.
    pub(crate) associated_bounds: HashMap<I, Vec<ConstraintTarget<I>>>,
    /// Associated family identities mapped to their generic parameter/constraint metadata.
    pub(crate) associated_type_parameters: HashMap<I, AssociatedTypeParameters<I>>,
    /// Checked associated type family definitions keyed by member identity.
    pub(crate) associated_type_families: HashMap<I, AssociatedTypeFamily<I>>,
    host_place_paths: HashMap<PlaceId, ResolvedHostPlacePath<I>>,
    host_paths: HashMap<ExprId, ResolvedHostPath<I>>,
    implementations: HashMap<(NominalType<I>, TypeId<I>), TraitImplementation<I>>,
    constraints: HashMap<TypeRefId, Option<ConstraintTarget<I>>>,
    type_refs: HashMap<TypeRefId, ResolvedTypeRef<I>>,
    field_types: HashMap<FieldId, TypeId<I>>,
    expr_fields: HashMap<ExprId, I>,
    place_fields: HashMap<PlaceId, I>,
    place_indexes: HashMap<PlaceId, NominalType<I>>,
    struct_inits: HashMap<ExprId, ResolvedStructInit<I>>,
    enum_constructors: HashMap<ExprId, ResolvedEnumConstructor<I>>,
    exprs: HashMap<ExprId, TypeId<I>>,
    callable_coercions: HashMap<ExprId, (TypeId<I>, NominalType<I>)>,
    interface_coercions: HashMap<ExprId, ResolvedInterfaceCoercion<I>>,
    locals: HashMap<LocalId, TypeId<I>>,
    places: HashMap<PlaceId, TypeId<I>>,
    calls: HashMap<ExprId, ResolvedCall<I>>,
    /// Generated methods whose checked tail call proves the registered default recipe.
    pub(crate) native_default_calls: HashMap<FunctionId, ExprId>,
    scalars: HashMap<ExprId, ScalarValue>,
    pattern_scalars: HashMap<PatternId, ScalarValue>,
    pattern_ranges: HashMap<PatternId, (ScalarValue, ScalarValue)>,
    pattern_fields: HashMap<PatternId, Vec<I>>,
    pattern_variants: HashMap<PatternId, I>,
}

/// Selected trait-associated constant access before executable lowering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedAssociatedConst<I: DefinitionReference = DefinitionPath> {
    /// Type selecting the implementation.
    pub receiver: TypeId<I>,
    /// Applied trait containing the constant.
    pub interface: NominalType<I>,
    /// Canonical associated constant definition.
    pub member: I,
}

/// Checked concrete/interface conversion and the implementation path it uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedInterfaceCoercion<I: DefinitionReference = DefinitionPath> {
    /// Selected script/host/upcast implementation category.
    pub implementation: ResolvedInterfaceImplementation<I>,
    /// Source value type before interface conversion.
    pub concrete_type: TypeId<I>,
    /// Applied destination interface type.
    pub interface_type: NominalType<I>,
}

/// How an interface value obtains its implementation metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedInterfaceImplementation<I: DefinitionReference = DefinitionPath> {
    /// An interface-to-parent-interface conversion.
    Upcast,
    /// A selected script implementation and its applied arguments.
    Script {
        /// Canonical implementation identity.
        declaration: I,
        /// Implementation arguments in binder order.
        arguments: Vec<TypeId<I>>,
    },
    /// An installed host implementation path.
    Host,
}

/// A checked expression path through declared host access, not an unrestricted Rust reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedHostPath<I: DefinitionReference = DefinitionPath> {
    /// Root expression evaluated before the path's dynamic arguments.
    pub root: ExprId,
    /// Source-order expressions paired with their declared runtime argument slots.
    pub dynamic_arguments: Vec<(u32, ExprId)>,
    /// Installed path declaration selected by checking.
    pub declaration: HostPathDeclaration<I>,
    /// Checked access/value contract consumed by lowering.
    pub contract: HostPathContract<I>,
}

/// A checked assignment path through declared host access.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedHostPlacePath<I: DefinitionReference = DefinitionPath> {
    /// Root place anchoring the host mutation path.
    pub root: PlaceId,
    /// Source-order index expressions paired with declared runtime argument slots.
    pub dynamic_arguments: Vec<(u32, ExprId)>,
    /// Installed path declaration selected by mutation checking.
    pub declaration: HostPathDeclaration<I>,
    /// Checked access/value contract required for mutation.
    pub contract: HostPathContract<I>,
}

impl TypeTable {
    /// Records/replaces the selected iterator interfaces and element type under the matching node ID.
    pub fn insert_iteration(&mut self, id: ExprId, fact: ResolvedIteration) {
        self.iterations.insert(id, fact);
    }

    /// Records/replaces the receiver type retained for a protocol operation under the matching node ID.
    pub fn insert_protocol_receiver(&mut self, id: ExprId, ty: TypeId) {
        self.protocol_receivers.insert(id, ty);
    }

    /// Records/replaces the index-mutation interface under the matching node ID.
    pub(crate) fn insert_place_index(&mut self, id: PlaceId, interface: NominalType) {
        self.place_indexes.insert(id, interface);
    }

    /// Records/replaces the selected associated constant under the matching node ID.
    pub(crate) fn insert_associated_const(&mut self, expr: ExprId, fact: ResolvedAssociatedConst) {
        self.associated_consts.insert(expr, fact);
    }

    /// Records/replaces the declared host mutation path under the matching node ID.
    pub(crate) fn insert_host_place_path(&mut self, place: PlaceId, path: ResolvedHostPlacePath) {
        self.host_place_paths.insert(place, path);
    }

    /// Records/replaces the declared host read path under the matching node ID.
    pub(crate) fn insert_host_path(&mut self, expr: ExprId, path: ResolvedHostPath) {
        self.host_paths.insert(expr, path);
    }

    #[cfg(test)]
    pub(crate) fn assert_same_source_facts(
        &self,
        other: &Self,
        arena: HirArenaId,
        other_arena: HirArenaId,
    ) {
        // Fresh analysis must have different local identities. Compare slot facts
        // only after checking every key/receiver belongs to its actual lowering.
        fn normalized(table: &TypeTable, from: HirArenaId, to: HirArenaId) -> TypeTable {
            let mut result = table.clone();
            result.field_types = result
                .field_types
                .into_iter()
                .map(|(id, fact)| {
                    assert_eq!(id.arena(), from, "foreign field key");
                    (FieldId::new(to, id.owner(), id.slot()), fact)
                })
                .collect();
            macro_rules! keys {
                ($($field:ident : $ty:ident),+ $(,)?) => {$(
                    result.$field = result.$field.into_iter().map(|(id, fact)| {
                        assert_eq!(id.arena(), from, "foreign key in {}", stringify!($field));
                        (crate::hir::ids::$ty::new(to, id.owner(), id.index()), fact)
                    }).collect();
                )+};
            }

            keys!(propagations: ExprId, iterations: ExprId, protocol_receivers: ExprId, host_place_paths: PlaceId, host_paths: ExprId, constraints: TypeRefId, type_refs: TypeRefId, expr_fields: ExprId,
                place_fields: PlaceId, place_indexes: PlaceId, struct_inits: ExprId, enum_constructors: ExprId, exprs: ExprId, locals: LocalId,
                places: PlaceId, calls: ExprId, scalars: ExprId, pattern_scalars: PatternId, pattern_ranges: PatternId, pattern_variants: PatternId,
                callable_coercions: ExprId, interface_coercions: ExprId, associated_consts: ExprId);
            for site in result.native_default_calls.values_mut() {
                assert_eq!(site.arena(), from);
                *site = ExprId::new(to, site.owner(), site.index());
            }
            for call in result.calls.values_mut() {
                if let Some(receiver) = call.receiver {
                    assert_eq!(receiver.arena(), from);
                    call.receiver = Some(ExprId::new(to, receiver.owner(), receiver.index()));
                }
            }
            for path in result.host_paths.values_mut() {
                assert_eq!(path.root.arena(), from);
                path.root = ExprId::new(to, path.root.owner(), path.root.index());
                for (_, argument) in &mut path.dynamic_arguments {
                    assert_eq!(argument.arena(), from);
                    *argument = ExprId::new(to, argument.owner(), argument.index());
                }
            }
            for path in result.host_place_paths.values_mut() {
                assert_eq!(path.root.arena(), from);
                path.root = PlaceId::new(to, path.root.owner(), path.root.index());
                for (_, argument) in &mut path.dynamic_arguments {
                    assert_eq!(argument.arena(), from);
                    *argument = ExprId::new(to, argument.owner(), argument.index());
                }
            }
            result
        }

        assert_eq!(
            normalized(self, arena, other_arena),
            normalized(other, other_arena, other_arena)
        );
    }

    pub(super) fn remap_signature_types(
        &self,
        ids: &HashMap<TypeRefId, TypeRefId>,
        fields: &HashMap<FieldId, FieldId>,
    ) -> Option<Self> {
        let mut result = self.clone();
        result.field_types = self
            .field_types
            .iter()
            .map(|(id, value)| Some((*fields.get(id)?, value.clone())))
            .collect::<Option<_>>()?;
        result.type_refs = self
            .type_refs
            .iter()
            .map(|(id, value)| Some((*ids.get(id)?, value.clone())))
            .collect::<Option<_>>()?;
        result.constraints = self
            .constraints
            .iter()
            .map(|(id, value)| Some((*ids.get(id)?, value.clone())))
            .collect::<Option<_>>()?;
        Some(result)
    }

    pub(crate) fn insert_implementation(
        &mut self,
        declaration: DefinitionPath,
        trait_type: NominalType,
        ty: TypeId,
        parameters: Vec<GenericParameterType>,
        bounds: super::GenericBounds,
        methods: HashMap<DefinitionPath, FunctionId>,
    ) {
        self.implementations
            .entry((trait_type, ty))
            .or_insert(TraitImplementation {
                declaration,
                parameters,
                bounds,
                methods,
            });
    }

    pub(crate) fn implementation_entries(
        &self,
    ) -> impl Iterator<
        Item = (
            &DefinitionPath,
            &NominalType,
            &TypeId,
            &[GenericParameterType],
            &super::GenericBounds,
            &HashMap<DefinitionPath, FunctionId>,
        ),
    > {
        self.implementations
            .iter()
            .map(|((trait_type, for_type), implementation)| {
                (
                    &implementation.declaration,
                    trait_type,
                    for_type,
                    implementation.parameters.as_slice(),
                    &implementation.bounds,
                    &implementation.methods,
                )
            })
    }

    /// Checks intrinsic or matching recorded trait implementations with a cycle guard and bound validation.
    pub fn implements(&self, trait_type: &NominalType, ty: &TypeId) -> bool {
        self.implements_with_guard(trait_type, ty, &mut HashSet::new())
    }

    /// Finds a matching implementation method plus substituted arguments; absent if no method/bounds match.
    pub fn implementation_method(
        &self,
        method: &DefinitionPath,
        trait_type: &NominalType,
        ty: &TypeId,
    ) -> Option<(FunctionId, Vec<TypeId>)> {
        self.implementations
            .iter()
            .find_map(|((implemented_trait, pattern), implementation)| {
                let function = *implementation.methods.get(method)?;
                let matched = match_implementation(
                    implemented_trait,
                    trait_type,
                    pattern,
                    ty,
                    &implementation.parameters,
                )?;
                if !self.implementation_bounds_hold(implementation, &matched, &mut HashSet::new()) {
                    return None;
                }
                let arguments = implementation
                    .parameters
                    .iter()
                    .map(|parameter| matched.get(parameter).cloned())
                    .collect::<Option<Vec<_>>>()?;
                Some((function, arguments))
            })
    }

    fn implements_with_guard(
        &self,
        trait_type: &NominalType,
        ty: &TypeId,
        visiting: &mut HashSet<(NominalType, TypeId)>,
    ) -> bool {
        if trait_type.arguments.is_empty()
            && trait_type.associated_types.is_empty()
            && let Some(kind) = Protocol::from_id(&trait_type.declaration)
        {
            return traits::intrinsic_holds(kind, ty, None, &Default::default());
        }
        let key = (trait_type.clone(), ty.clone());
        if !visiting.insert(key.clone()) {
            return false;
        }
        let found =
            self.implementations
                .iter()
                .any(|((implemented_trait, pattern), implementation)| {
                    match_implementation(
                        implemented_trait,
                        trait_type,
                        pattern,
                        ty,
                        &implementation.parameters,
                    )
                    .is_some_and(|matched| {
                        self.implementation_bounds_hold(implementation, &matched, visiting)
                    })
                });
        visiting.remove(&key);
        found
    }

    fn implementation_bounds_hold(
        &self,
        implementation: &TraitImplementation,
        matched: &TypeSubstitution,
        visiting: &mut HashSet<(NominalType, TypeId)>,
    ) -> bool {
        implementation
            .bounds
            .iter()
            .all(|(parameter, constraints)| {
                let actual = parameter.instantiate(matched);
                constraints.iter().all(|constraint| match constraint {
                    ConstraintTarget::Standard(standard) => {
                        constraints::type_satisfies_standard_constraint(
                            &actual,
                            *standard,
                            &Default::default(),
                            None,
                        )
                    }
                    ConstraintTarget::Trait(trait_type) => self.implements_with_guard(
                        &trait_type.instantiate(matched),
                        &actual,
                        visiting,
                    ),
                })
            })
    }

    /// Records/replaces the resolved constraint target under the matching node ID.
    pub(crate) fn insert_constraint(&mut self, id: TypeRefId, target: Option<ConstraintTarget>) {
        self.constraints.insert(id, target);
    }

    /// Records/replaces the resolved type and navigation target under the matching node ID.
    pub(crate) fn insert_type_ref(&mut self, id: TypeRefId, resolved: ResolvedTypeRef) {
        self.type_refs.insert(id, resolved);
    }

    /// Records/replaces the field semantic type under the matching node ID.
    pub(crate) fn insert_field_type(&mut self, field: FieldId, ty: TypeId) {
        self.field_types.insert(field, ty);
    }

    /// Records/replaces the selected field definition under the matching node ID.
    pub(crate) fn insert_expr_field(&mut self, expr: ExprId, field: DefinitionPath) {
        self.expr_fields.insert(expr, field);
    }

    /// Records/replaces the selected assignment-field definition under the matching node ID.
    pub(crate) fn insert_place_field(&mut self, place: PlaceId, field: DefinitionPath) {
        self.place_fields.insert(place, field);
    }

    /// Records/replaces the constructor field selections under the matching node ID.
    pub(crate) fn insert_struct_init(&mut self, expr: ExprId, target: ResolvedStructInit) {
        self.struct_inits.insert(expr, target);
    }

    pub(crate) fn restore_function(
        &mut self,
        old: &Self,
        old_map: &SourceMap,
        new_map: &SourceMap,
        old_span: Span,
        new_span: Span,
    ) -> bool {
        fn remap(
            old: &[Span],
            new: &[Span],
            old_span: Span,
            new_span: Span,
        ) -> Option<Vec<(usize, usize)>> {
            let relative = |spans: &[Span], owner: Span| {
                spans
                    .iter()
                    .enumerate()
                    .filter(|(_, span)| span.start >= owner.start && span.end <= owner.end)
                    .map(|(id, span)| (id, span.start - owner.start, span.end - owner.start))
                    .collect::<Vec<_>>()
            };
            let old = relative(old, old_span);
            let new = relative(new, new_span);
            if old.len() != new.len() {
                return None;
            }
            old.into_iter()
                .zip(new)
                .map(|((a, start, end), (b, ns, ne))| (start == ns && end == ne).then_some((a, b)))
                .collect()
        }
        let Some(exprs) = remap(
            old_map.expr_spans(),
            new_map.expr_spans(),
            old_span,
            new_span,
        ) else {
            return false;
        };
        let Some(locals) = remap(
            old_map.local_spans(),
            new_map.local_spans(),
            old_span,
            new_span,
        ) else {
            return false;
        };
        let Some(places) = remap(
            old_map.place_spans(),
            new_map.place_spans(),
            old_span,
            new_span,
        ) else {
            return false;
        };
        let Some(patterns) = remap(
            old_map.pattern_spans(),
            new_map.pattern_spans(),
            old_span,
            new_span,
        ) else {
            return false;
        };
        let Some(types) = remap(
            old_map.type_spans(),
            new_map.type_spans(),
            old_span,
            new_span,
        ) else {
            return false;
        };
        let expr_ids = exprs
            .iter()
            .map(|(a, b)| (old_map.expr_id(*a), new_map.expr_id(*b)))
            .collect::<HashMap<_, _>>();
        let mut calls = Vec::new();
        let place_ids = places
            .iter()
            .map(|(a, b)| (old_map.place_id(*a), new_map.place_id(*b)))
            .collect::<HashMap<_, _>>();
        let mut host_place_paths = Vec::new();
        for (old_id, new_id) in &place_ids {
            if let Some(path) = old.host_place_paths.get(old_id) {
                let Some(root) = place_ids.get(&path.root) else {
                    return false;
                };
                let Some(dynamic_arguments) = path
                    .dynamic_arguments
                    .iter()
                    .map(|(slot, argument)| expr_ids.get(argument).copied().map(|id| (*slot, id)))
                    .collect::<Option<Vec<_>>>()
                else {
                    return false;
                };
                host_place_paths.push((
                    *new_id,
                    ResolvedHostPlacePath {
                        root: *root,
                        dynamic_arguments,
                        declaration: path.declaration.clone(),
                        contract: path.contract.clone(),
                    },
                ));
            }
        }
        let mut host_paths = Vec::new();
        for (old_id, new_id) in &expr_ids {
            if let Some(path) = old.host_paths.get(old_id) {
                let Some(root) = expr_ids.get(&path.root) else {
                    return false;
                };
                let Some(dynamic_arguments) = path
                    .dynamic_arguments
                    .iter()
                    .map(|(slot, argument)| expr_ids.get(argument).copied().map(|id| (*slot, id)))
                    .collect::<Option<Vec<_>>>()
                else {
                    return false;
                };
                host_paths.push((
                    *new_id,
                    ResolvedHostPath {
                        root: *root,
                        dynamic_arguments,
                        declaration: path.declaration.clone(),
                        contract: path.contract.clone(),
                    },
                ));
            }
            if let Some(call) = old.calls.get(old_id) {
                let receiver = match call.receiver {
                    Some(id) => match expr_ids.get(&id) {
                        Some(id) => Some(*id),
                        None => return false,
                    },
                    None => None,
                };
                // The non-body declaration environment must match before reuse,
                // so function/method arena IDs remain unchanged.
                calls.push((
                    *new_id,
                    ResolvedCall {
                        target: call.target.clone(),
                        receiver,
                        type_arguments: call.type_arguments.clone(),
                        signature: call.signature.clone(),
                    },
                ));
            }
        }
        for (function, site) in &old.native_default_calls {
            if let Some(mapped) = expr_ids.get(site) {
                self.native_default_calls.insert(*function, *mapped);
            }
        }
        self.calls.extend(calls);
        self.host_place_paths.extend(host_place_paths);
        self.host_paths.extend(host_paths);
        for (a, b) in types {
            if let Some(ty) = old.type_refs.get(&old_map.type_id(a)) {
                self.type_refs.insert(new_map.type_id(b), ty.clone());
            }
        }
        for (a, b) in patterns {
            if let Some(value) = old.pattern_scalars.get(&old_map.pattern_id(a)) {
                self.pattern_scalars
                    .insert(new_map.pattern_id(b), value.clone());
            }
            if let Some(bounds) = old.pattern_ranges.get(&old_map.pattern_id(a)) {
                self.pattern_ranges
                    .insert(new_map.pattern_id(b), bounds.clone());
            }
            if let Some(fields) = old.pattern_fields.get(&old_map.pattern_id(a)) {
                self.pattern_fields
                    .insert(new_map.pattern_id(b), fields.clone());
            }
            if let Some(variant) = old.pattern_variants.get(&old_map.pattern_id(a)) {
                self.pattern_variants
                    .insert(new_map.pattern_id(b), variant.clone());
            }
        }
        for (a, b) in exprs {
            if let Some(fact) = old.propagations.get(&old_map.expr_id(a)) {
                self.propagations.insert(new_map.expr_id(b), fact.clone());
            }
            if let Some(fact) = old.iterations.get(&old_map.expr_id(a)) {
                self.iterations.insert(new_map.expr_id(b), fact.clone());
            }
            if let Some(fact) = old.protocol_receivers.get(&old_map.expr_id(a)) {
                self.protocol_receivers
                    .insert(new_map.expr_id(b), fact.clone());
            }
            if let Some(fact) = old.associated_consts.get(&old_map.expr_id(a)) {
                self.associated_consts
                    .insert(new_map.expr_id(b), fact.clone());
            }
            if let Some(coercion) = old.callable_coercions.get(&old_map.expr_id(a)) {
                self.callable_coercions
                    .insert(new_map.expr_id(b), coercion.clone());
            }
            if let Some(coercion) = old.interface_coercions.get(&old_map.expr_id(a)) {
                self.interface_coercions
                    .insert(new_map.expr_id(b), coercion.clone());
            }
            if let Some(field) = old.expr_fields.get(&old_map.expr_id(a)) {
                self.expr_fields.insert(new_map.expr_id(b), field.clone());
            }
            if let Some(target) = old.struct_inits.get(&old_map.expr_id(a)) {
                self.struct_inits.insert(new_map.expr_id(b), target.clone());
            }
            if let Some(target) = old.enum_constructors.get(&old_map.expr_id(a)) {
                self.enum_constructors
                    .insert(new_map.expr_id(b), target.clone());
            }
            if let Some(value) = old.scalars.get(&old_map.expr_id(a)) {
                self.scalars.insert(new_map.expr_id(b), value.clone());
            }
            if let Some(ty) = old.exprs.get(&old_map.expr_id(a)) {
                self.exprs.insert(new_map.expr_id(b), ty.clone());
            }
        }
        for (a, b) in locals {
            if let Some(ty) = old.locals.get(&old_map.local_id(a)) {
                self.locals.insert(new_map.local_id(b), ty.clone());
            }
        }
        for (a, b) in places {
            if let Some(interface) = old.place_indexes.get(&old_map.place_id(a)) {
                self.place_indexes
                    .insert(new_map.place_id(b), interface.clone());
            }
            if let Some(field) = old.place_fields.get(&old_map.place_id(a)) {
                self.place_fields.insert(new_map.place_id(b), field.clone());
            }
            if let Some(ty) = old.places.get(&old_map.place_id(a)) {
                self.places.insert(new_map.place_id(b), ty.clone());
            }
        }
        true
    }

    pub(crate) fn insert_expr(&mut self, id: ExprId, ty: TypeId) {
        self.exprs.insert(id, ty);
    }

    /// Records/replaces the enum/variant constructor selection under the matching node ID.
    pub(crate) fn insert_enum_constructor(&mut self, id: ExprId, target: ResolvedEnumConstructor) {
        self.enum_constructors.insert(id, target);
    }

    pub(crate) fn insert_scalar(&mut self, id: ExprId, value: ScalarValue) {
        self.scalars.insert(id, value);
    }

    pub(crate) fn insert_pattern_scalar(&mut self, id: PatternId, value: ScalarValue) {
        self.pattern_scalars.insert(id, value);
    }

    /// Records/replaces the checked pattern endpoints under the matching node ID.
    pub(crate) fn insert_pattern_range(
        &mut self,
        id: PatternId,
        start: ScalarValue,
        end: ScalarValue,
    ) {
        self.pattern_ranges.insert(id, (start, end));
    }

    /// Records/replaces the field definitions in pattern field order under the matching node ID.
    pub fn insert_pattern_fields(&mut self, id: PatternId, fields: Vec<DefinitionPath>) {
        self.pattern_fields.insert(id, fields);
    }

    /// Records/replaces the selected enum variant under the matching node ID.
    pub fn insert_pattern_variant(&mut self, id: PatternId, variant: DefinitionPath) {
        self.pattern_variants.insert(id, variant);
    }

    pub(crate) fn insert_local(&mut self, id: LocalId, ty: TypeId) {
        self.locals.insert(id, ty);
    }

    pub(crate) fn insert_place(&mut self, id: PlaceId, ty: TypeId) {
        self.places.insert(id, ty);
    }

    pub(crate) fn insert_call(&mut self, id: ExprId, target: CallTarget, receiver: Option<ExprId>) {
        self.calls.insert(
            id,
            ResolvedCall {
                target,
                receiver,
                type_arguments: Vec::new(),
                signature: None,
            },
        );
    }

    pub(crate) fn insert_type_arguments(&mut self, id: ExprId, arguments: Vec<TypeId>) {
        self.calls
            .get_mut(&id)
            .expect("resolved generic call")
            .type_arguments = arguments;
    }

    pub(crate) fn insert_call_signature(&mut self, id: ExprId, signature: AppliedCallSignature) {
        self.calls
            .get_mut(&id)
            .expect("resolved callable application")
            .signature = Some(signature);
    }

    /// The produced value type includes checked capability and callable conversions.
    pub fn coerced_expr_type(&self, id: ExprId) -> Option<TypeId> {
        self.callable_coercion(id)
            .and_then(|(_, interface)| traits::callable_signature(interface))
            .or_else(|| {
                self.interface_coercion(id)
                    .map(|coercion| TypeId::Trait(coercion.interface_type.clone()))
            })
            .or_else(|| self.expr_type(id))
    }

    /// Records/replaces the callable source type and destination interface under the matching node ID.
    pub(crate) fn insert_callable_coercion(
        &mut self,
        id: ExprId,
        receiver: TypeId,
        interface: NominalType,
    ) {
        self.callable_coercions.insert(id, (receiver, interface));
    }

    /// Records/replaces the checked interface conversion under the matching node ID.
    pub(crate) fn insert_interface_coercion(
        &mut self,
        id: ExprId,
        coercion: ResolvedInterfaceCoercion,
    ) {
        self.interface_coercions.insert(id, coercion);
    }
}

pub(crate) fn match_implementation(
    implemented_trait: &NominalType,
    requested_trait: &NominalType,
    pattern: &TypeId,
    actual: &TypeId,
    parameters: &[GenericParameterType],
) -> Option<TypeSubstitution> {
    if implemented_trait.declaration != requested_trait.declaration
        || implemented_trait.arguments.len() != requested_trait.arguments.len()
    {
        return None;
    }
    let mut bindings = TypeSubstitution::default();
    let mut pending = vec![(pattern, actual)];
    pending.extend(
        implemented_trait
            .arguments
            .iter()
            .zip(&requested_trait.arguments),
    );
    while let Some((pattern, actual)) = pending.pop() {
        match (pattern, actual) {
            (TypeId::Generic(parameter), actual) if parameters.contains(parameter) => {
                if let Some(bound) = bindings.get(parameter) {
                    if bound != actual {
                        return None;
                    }
                } else {
                    bindings.insert(parameter.clone(), actual.clone());
                }
            }
            (TypeId::Struct(left), TypeId::Struct(right))
            | (TypeId::NativeObject(left), TypeId::NativeObject(right))
            | (TypeId::Enum(left), TypeId::Enum(right))
            | (TypeId::Trait(left), TypeId::Trait(right)) => {
                if left.declaration != right.declaration
                    || left.arguments.len() != right.arguments.len()
                    || !left
                        .associated_types
                        .keys()
                        .eq(right.associated_types.keys())
                {
                    return None;
                }
                pending.extend(
                    left.associated_types
                        .values()
                        .zip(right.associated_types.values()),
                );
                pending.extend(left.arguments.iter().zip(&right.arguments));
            }
            (TypeId::Tuple(left), TypeId::Tuple(right)) if left.len() == right.len() => {
                pending.extend(left.iter().zip(right));
            }
            (
                TypeId::Function {
                    params: left,
                    result: left_result,
                },
                TypeId::Function {
                    params: right,
                    result: right_result,
                },
            ) if left.len() == right.len() => {
                pending.push((left_result, right_result));
                pending.extend(left.iter().zip(right));
            }
            (TypeId::Range(left, a), TypeId::Range(right, b)) if a == b => {
                pending.push((left, right))
            }
            (TypeId::Iter(left), TypeId::Iter(right))
            | (TypeId::Array(left, _), TypeId::Array(right, _))
            | (TypeId::Set(left, _), TypeId::Set(right, _)) => pending.push((left, right)),
            (
                TypeId::Map {
                    key: left_key,
                    value: left_value,
                    ..
                },
                TypeId::Map {
                    key: right_key,
                    value: right_value,
                    ..
                },
            ) => {
                pending.push((left_key, right_key));
                pending.push((left_value, right_value));
            }
            (left, right) if left == right => {}
            _ => return None,
        }
    }
    for (member, expected) in &requested_trait.associated_types {
        let actual = implemented_trait
            .associated_types
            .get(member)?
            .instantiate(&bindings);
        if actual != *expected {
            return None;
        }
    }
    Some(bindings)
}

impl<I: DefinitionReference> Default for TypeTable<I> {
    fn default() -> Self {
        Self {
            inference_holes: Default::default(),
            iterations: Default::default(),
            propagations: Default::default(),
            protocol_receivers: Default::default(),
            associated_consts: Default::default(),
            resolving_types: Default::default(),
            associated_bounds: Default::default(),
            associated_type_parameters: Default::default(),
            associated_type_families: Default::default(),
            host_place_paths: Default::default(),
            host_paths: Default::default(),
            implementations: Default::default(),
            constraints: Default::default(),
            type_refs: Default::default(),
            field_types: Default::default(),
            expr_fields: Default::default(),
            place_fields: Default::default(),
            place_indexes: Default::default(),
            struct_inits: Default::default(),
            enum_constructors: Default::default(),
            exprs: Default::default(),
            callable_coercions: Default::default(),
            interface_coercions: Default::default(),
            locals: Default::default(),
            places: Default::default(),
            calls: Default::default(),
            native_default_calls: Default::default(),
            scalars: Default::default(),
            pattern_scalars: Default::default(),
            pattern_ranges: Default::default(),
            pattern_fields: Default::default(),
            pattern_variants: Default::default(),
        }
    }
}

mod mapping;
pub mod propagation;

impl<I: DefinitionReference> TypeTable<I> {
    /// Returns the checked forwarding call for a generated native default method.
    /// Signature-only or unsuccessful body analysis has no entry.
    pub fn native_default_call(&self, function: FunctionId) -> Option<ResolvedCall<I>> {
        self.call_resolution(*self.native_default_calls.get(&function)?)
    }

    /// Returns the recorded selected iterator interfaces and element type, or `None` when no such fact was recorded.
    pub fn iteration(&self, id: ExprId) -> Option<&ResolvedIteration<I>> {
        self.iterations.get(&id)
    }

    /// Returns the recorded receiver type retained for a protocol operation, or `None` when no such fact was recorded.
    pub fn protocol_receiver(&self, id: ExprId) -> Option<&TypeId<I>> {
        self.protocol_receivers.get(&id)
    }

    /// Returns the recorded index-mutation interface, or `None` when no such fact was recorded.
    pub fn place_index(&self, id: PlaceId) -> Option<&NominalType<I>> {
        self.place_indexes.get(&id)
    }

    /// Returns the recorded associated family binders and bounds, or `None` when no such fact was recorded.
    pub fn associated_type_parameters(&self, member: &I) -> Option<&AssociatedTypeParameters<I>> {
        self.associated_type_parameters.get(member)
    }

    /// Returns the recorded associated family output definition, or `None` when no such fact was recorded.
    pub fn associated_type_family(&self, member: &I) -> Option<&AssociatedTypeFamily<I>> {
        self.associated_type_families.get(member)
    }

    /// Returns the recorded selected associated constant, or `None` when no such fact was recorded.
    pub fn associated_const(&self, expr: ExprId) -> Option<&ResolvedAssociatedConst<I>> {
        self.associated_consts.get(&expr)
    }

    /// Returns the recorded declared host mutation path, or `None` when no such fact was recorded.
    pub fn host_place_path(&self, place: PlaceId) -> Option<&ResolvedHostPlacePath<I>> {
        self.host_place_paths.get(&place)
    }

    /// Returns the recorded declared host read path, or `None` when no such fact was recorded.
    pub fn host_path(&self, expr: ExprId) -> Option<&ResolvedHostPath<I>> {
        self.host_paths.get(&expr)
    }

    /// Returns the recorded resolved constraint target, or `None` when no such fact was recorded.
    pub fn constraint(&self, id: TypeRefId) -> Option<ConstraintTarget<I>> {
        self.constraints.get(&id).cloned().flatten()
    }

    pub(crate) fn has_constraint(&self, id: TypeRefId) -> bool {
        self.constraints.contains_key(&id)
    }

    /// Returns the recorded resolved type and navigation target, or `None` when no such fact was recorded.
    pub fn type_ref(&self, id: TypeRefId) -> Option<&ResolvedTypeRef<I>> {
        self.type_refs.get(&id)
    }

    /// Returns the recorded field semantic type, or `None` when no such fact was recorded.
    pub fn field_type(&self, field: FieldId) -> Option<TypeId<I>> {
        self.field_types.get(&field).cloned()
    }

    /// Returns the recorded selected field definition, or `None` when no such fact was recorded.
    pub fn expr_field(&self, expr: ExprId) -> Option<&I> {
        self.expr_fields.get(&expr)
    }

    /// Returns the recorded selected assignment-field definition, or `None` when no such fact was recorded.
    pub fn place_field(&self, place: PlaceId) -> Option<&I> {
        self.place_fields.get(&place)
    }

    /// Returns the recorded constructor field selections, or `None` when no such fact was recorded.
    pub fn struct_init(&self, expr: ExprId) -> Option<&ResolvedStructInit<I>> {
        self.struct_inits.get(&expr)
    }

    /// Returns the recorded enum/variant constructor selection, or `None` when no such fact was recorded.
    pub fn enum_constructor(&self, id: ExprId) -> Option<&ResolvedEnumConstructor<I>> {
        self.enum_constructors.get(&id)
    }

    /// Returns the recorded checked scalar value, or `None` when no such fact was recorded.
    pub fn scalar_value(&self, id: ExprId) -> Option<&ScalarValue> {
        self.scalars.get(&id)
    }

    /// Returns the recorded checked literal-pattern value, or `None` when no such fact was recorded.
    pub fn pattern_scalar_value(&self, id: PatternId) -> Option<&ScalarValue> {
        self.pattern_scalars.get(&id)
    }

    /// Returns the recorded checked pattern endpoints, or `None` when no such fact was recorded.
    pub fn pattern_range(&self, id: PatternId) -> Option<&(ScalarValue, ScalarValue)> {
        self.pattern_ranges.get(&id)
    }

    /// Returns the recorded field definitions in pattern field order, or `None` when no such fact was recorded.
    pub fn pattern_fields(&self, id: PatternId) -> Option<&[I]> {
        self.pattern_fields.get(&id).map(Vec::as_slice)
    }

    /// Returns the recorded selected enum variant, or `None` when no such fact was recorded.
    pub fn pattern_variant(&self, id: PatternId) -> Option<&I> {
        self.pattern_variants.get(&id)
    }

    /// Returns the recorded expression semantic type, or `None` when no such fact was recorded.
    pub fn expr_type(&self, id: ExprId) -> Option<TypeId<I>> {
        self.exprs.get(&id).cloned()
    }

    /// Returns the recorded callable source type and destination interface, or `None` when no such fact was recorded.
    pub fn callable_coercion(&self, id: ExprId) -> Option<&(TypeId<I>, NominalType<I>)> {
        self.callable_coercions.get(&id)
    }

    /// Returns the recorded checked interface conversion, or `None` when no such fact was recorded.
    pub fn interface_coercion(&self, id: ExprId) -> Option<&ResolvedInterfaceCoercion<I>> {
        self.interface_coercions.get(&id)
    }

    /// Returns the recorded local binding type, or `None` when no such fact was recorded.
    pub fn local_type(&self, id: LocalId) -> Option<TypeId<I>> {
        self.locals.get(&id).cloned()
    }

    /// Returns the recorded assignment-place type, or `None` when no such fact was recorded.
    pub fn place_type(&self, id: PlaceId) -> Option<TypeId<I>> {
        self.places.get(&id).cloned()
    }

    /// Returns the recorded selected callable and applied signature, or `None` when no such fact was recorded.
    pub fn call_resolution(&self, id: ExprId) -> Option<ResolvedCall<I>> {
        self.calls.get(&id).cloned()
    }
}

impl<I: DefinitionReference> TypeTable<I> {
    #[cfg(test)]
    pub(crate) fn host_write_places(&self) -> impl Iterator<Item = PlaceId> + '_ {
        self.host_place_paths.keys().copied()
    }
}
