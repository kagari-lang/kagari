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
    typeck::{constraints, scalar::ScalarValue},
    types::{
        AssociatedTypeFamily, AssociatedTypeParameters, GenericParameterType, NominalType, TypeId,
        TypeSubstitution,
    },
};

use kagari_common::{
    host_interface::path::{HostPathContract, HostPathDeclaration},
    identity::{DefinitionPath, reference::DefinitionReference},
    span::Span,
};
use kagari_contract::{language::Protocol, standard::surface::StandardTypeConstraint};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConstraintTarget<I: DefinitionReference = DefinitionPath> {
    Standard(StandardTypeConstraint),
    Trait(NominalType<I>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeTarget<I: DefinitionReference = DefinitionPath> {
    OpaqueType(OpaqueTypeId),
    Host(HostTypeId),
    Source(I),
    AssociatedType(I),
    Struct(StructId),
    Enum(EnumId),
    Trait(TraitId),
    Generic(GenericParamId),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedTypeRef<I: DefinitionReference = DefinitionPath> {
    pub ty: TypeId<I>,
    pub target: Option<TypeTarget<I>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallTarget<I: DefinitionReference = DefinitionPath> {
    /// The recorded receiver is the callee expression. Its evaluation exits
    /// before any callable value or explicit argument can be produced.
    TerminatingCallee,
    SourceFunction(I),
    HostFunction(HostFunctionId),
    Function(FunctionId),
    Value,
    RuntimeHelper(BuiltinFunction),
    TraitMethod {
        method: I,
        interface: NominalType<I>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedCall<I: DefinitionReference = DefinitionPath> {
    pub target: CallTarget<I>,
    /// Evaluated before explicit arguments, exactly once.
    pub receiver: Option<ExprId>,
    /// Declaration parameter order; arguments may refer to an enclosing binder.
    pub type_arguments: Vec<TypeId<I>>,
    /// Absent for incomplete calls, terminating callees and language helpers
    /// whose operands are not an ordinary callable signature.
    pub signature: Option<AppliedCallSignature<I>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedStructInit<I: DefinitionReference = DefinitionPath> {
    pub structure: I,
    /// Source order, including holes for unknown initializer fields.
    pub fields: Vec<Option<I>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedEnumConstructor<I: DefinitionReference = DefinitionPath> {
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedIteration<I: DefinitionReference = DefinitionPath> {
    pub into_interface: NominalType<I>,
    pub iterator: TypeId<I>,
    pub next_interface: NominalType<I>,
    pub item: TypeId<I>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TypeTable<I: DefinitionReference = DefinitionPath> {
    /// Temporary body-local placeholders; removed before publishing facts.
    pub(super) inference_holes: HashMap<TypeRefId, TypeId<I>>,
    iterations: HashMap<ExprId, ResolvedIteration<I>>,
    protocol_receivers: HashMap<ExprId, TypeId<I>>,
    associated_consts: HashMap<ExprId, ResolvedAssociatedConst<I>>,
    pub(super) resolving_types: HashSet<TypeRefId>,
    pub(crate) associated_bounds: HashMap<I, Vec<ConstraintTarget<I>>>,
    pub(crate) associated_type_parameters: HashMap<I, AssociatedTypeParameters<I>>,
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
    scalars: HashMap<ExprId, ScalarValue>,
    pattern_scalars: HashMap<PatternId, ScalarValue>,
    pattern_ranges: HashMap<PatternId, (ScalarValue, ScalarValue)>,
    pattern_fields: HashMap<PatternId, Vec<I>>,
    pattern_variants: HashMap<PatternId, I>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedAssociatedConst<I: DefinitionReference = DefinitionPath> {
    pub receiver: TypeId<I>,
    pub interface: NominalType<I>,
    pub member: I,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedInterfaceCoercion<I: DefinitionReference = DefinitionPath> {
    pub implementation: ResolvedInterfaceImplementation<I>,
    pub concrete_type: TypeId<I>,
    pub interface_type: NominalType<I>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedInterfaceImplementation<I: DefinitionReference = DefinitionPath> {
    Upcast,
    Script {
        declaration: I,
        arguments: Vec<TypeId<I>>,
    },
    Host,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedHostPath<I: DefinitionReference = DefinitionPath> {
    pub root: ExprId,
    /// Source-order expressions paired with their declared runtime argument slots.
    pub dynamic_arguments: Vec<(u32, ExprId)>,
    pub declaration: HostPathDeclaration<I>,
    pub contract: HostPathContract<I>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedHostPlacePath<I: DefinitionReference = DefinitionPath> {
    pub root: PlaceId,
    pub dynamic_arguments: Vec<(u32, ExprId)>,
    pub declaration: HostPathDeclaration<I>,
    pub contract: HostPathContract<I>,
}

impl TypeTable {
    pub fn insert_iteration(&mut self, id: ExprId, fact: ResolvedIteration) {
        self.iterations.insert(id, fact);
    }

    pub fn insert_protocol_receiver(&mut self, id: ExprId, ty: TypeId) {
        self.protocol_receivers.insert(id, ty);
    }

    pub(crate) fn insert_place_index(&mut self, id: PlaceId, interface: NominalType) {
        self.place_indexes.insert(id, interface);
    }

    pub(crate) fn insert_associated_const(&mut self, expr: ExprId, fact: ResolvedAssociatedConst) {
        self.associated_consts.insert(expr, fact);
    }

    pub(crate) fn insert_host_place_path(&mut self, place: PlaceId, path: ResolvedHostPlacePath) {
        self.host_place_paths.insert(place, path);
    }

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

            keys!(iterations: ExprId, protocol_receivers: ExprId, host_place_paths: PlaceId, host_paths: ExprId, constraints: TypeRefId, type_refs: TypeRefId, expr_fields: ExprId,
                place_fields: PlaceId, place_indexes: PlaceId, struct_inits: ExprId, enum_constructors: ExprId, exprs: ExprId, locals: LocalId,
                places: PlaceId, calls: ExprId, scalars: ExprId, pattern_scalars: PatternId, pattern_ranges: PatternId, pattern_variants: PatternId,
                callable_coercions: ExprId, interface_coercions: ExprId, associated_consts: ExprId);
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

    pub fn implements(&self, trait_type: &NominalType, ty: &TypeId) -> bool {
        self.implements_with_guard(trait_type, ty, &mut HashSet::new())
    }

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

    pub(crate) fn insert_constraint(&mut self, id: TypeRefId, target: Option<ConstraintTarget>) {
        self.constraints.insert(id, target);
    }

    pub(crate) fn insert_type_ref(&mut self, id: TypeRefId, resolved: ResolvedTypeRef) {
        self.type_refs.insert(id, resolved);
    }

    pub(crate) fn insert_field_type(&mut self, field: FieldId, ty: TypeId) {
        self.field_types.insert(field, ty);
    }

    pub(crate) fn insert_expr_field(&mut self, expr: ExprId, field: DefinitionPath) {
        self.expr_fields.insert(expr, field);
    }

    pub(crate) fn insert_place_field(&mut self, place: PlaceId, field: DefinitionPath) {
        self.place_fields.insert(place, field);
    }

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

    pub(crate) fn insert_enum_constructor(&mut self, id: ExprId, target: ResolvedEnumConstructor) {
        self.enum_constructors.insert(id, target);
    }

    pub(crate) fn insert_scalar(&mut self, id: ExprId, value: ScalarValue) {
        self.scalars.insert(id, value);
    }

    pub(crate) fn insert_pattern_scalar(&mut self, id: PatternId, value: ScalarValue) {
        self.pattern_scalars.insert(id, value);
    }

    pub(crate) fn insert_pattern_range(
        &mut self,
        id: PatternId,
        start: ScalarValue,
        end: ScalarValue,
    ) {
        self.pattern_ranges.insert(id, (start, end));
    }

    pub fn insert_pattern_fields(&mut self, id: PatternId, fields: Vec<DefinitionPath>) {
        self.pattern_fields.insert(id, fields);
    }

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

    pub(crate) fn insert_callable_coercion(
        &mut self,
        id: ExprId,
        receiver: TypeId,
        interface: NominalType,
    ) {
        self.callable_coercions.insert(id, (receiver, interface));
    }

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
            (TypeId::Tuple(left), TypeId::Tuple(right))
            | (TypeId::StandardEnum { args: left, .. }, TypeId::StandardEnum { args: right, .. })
                if left.len() == right.len() =>
            {
                if let (
                    TypeId::StandardEnum {
                        kind: left_kind, ..
                    },
                    TypeId::StandardEnum {
                        kind: right_kind, ..
                    },
                ) = (pattern, actual)
                    && left_kind != right_kind
                {
                    return None;
                }
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
            scalars: Default::default(),
            pattern_scalars: Default::default(),
            pattern_ranges: Default::default(),
            pattern_fields: Default::default(),
            pattern_variants: Default::default(),
        }
    }
}

mod mapping;

impl<I: DefinitionReference> TypeTable<I> {
    pub fn iteration(&self, id: ExprId) -> Option<&ResolvedIteration<I>> {
        self.iterations.get(&id)
    }

    pub fn protocol_receiver(&self, id: ExprId) -> Option<&TypeId<I>> {
        self.protocol_receivers.get(&id)
    }

    pub fn place_index(&self, id: PlaceId) -> Option<&NominalType<I>> {
        self.place_indexes.get(&id)
    }

    pub fn associated_type_parameters(&self, member: &I) -> Option<&AssociatedTypeParameters<I>> {
        self.associated_type_parameters.get(member)
    }

    pub fn associated_type_family(&self, member: &I) -> Option<&AssociatedTypeFamily<I>> {
        self.associated_type_families.get(member)
    }

    pub fn associated_const(&self, expr: ExprId) -> Option<&ResolvedAssociatedConst<I>> {
        self.associated_consts.get(&expr)
    }

    pub fn host_place_path(&self, place: PlaceId) -> Option<&ResolvedHostPlacePath<I>> {
        self.host_place_paths.get(&place)
    }

    pub fn host_path(&self, expr: ExprId) -> Option<&ResolvedHostPath<I>> {
        self.host_paths.get(&expr)
    }

    pub fn constraint(&self, id: TypeRefId) -> Option<ConstraintTarget<I>> {
        self.constraints.get(&id).cloned().flatten()
    }

    pub(crate) fn has_constraint(&self, id: TypeRefId) -> bool {
        self.constraints.contains_key(&id)
    }

    pub fn type_ref(&self, id: TypeRefId) -> Option<&ResolvedTypeRef<I>> {
        self.type_refs.get(&id)
    }

    pub fn field_type(&self, field: FieldId) -> Option<TypeId<I>> {
        self.field_types.get(&field).cloned()
    }

    pub fn expr_field(&self, expr: ExprId) -> Option<&I> {
        self.expr_fields.get(&expr)
    }

    pub fn place_field(&self, place: PlaceId) -> Option<&I> {
        self.place_fields.get(&place)
    }

    pub fn struct_init(&self, expr: ExprId) -> Option<&ResolvedStructInit<I>> {
        self.struct_inits.get(&expr)
    }

    pub fn enum_constructor(&self, id: ExprId) -> Option<&ResolvedEnumConstructor<I>> {
        self.enum_constructors.get(&id)
    }

    pub fn scalar_value(&self, id: ExprId) -> Option<&ScalarValue> {
        self.scalars.get(&id)
    }

    pub fn pattern_scalar_value(&self, id: PatternId) -> Option<&ScalarValue> {
        self.pattern_scalars.get(&id)
    }

    pub fn pattern_range(&self, id: PatternId) -> Option<&(ScalarValue, ScalarValue)> {
        self.pattern_ranges.get(&id)
    }

    pub fn pattern_fields(&self, id: PatternId) -> Option<&[I]> {
        self.pattern_fields.get(&id).map(Vec::as_slice)
    }

    pub fn pattern_variant(&self, id: PatternId) -> Option<&I> {
        self.pattern_variants.get(&id)
    }

    pub fn expr_type(&self, id: ExprId) -> Option<TypeId<I>> {
        self.exprs.get(&id).cloned()
    }

    pub fn callable_coercion(&self, id: ExprId) -> Option<&(TypeId<I>, NominalType<I>)> {
        self.callable_coercions.get(&id)
    }

    pub fn interface_coercion(&self, id: ExprId) -> Option<&ResolvedInterfaceCoercion<I>> {
        self.interface_coercions.get(&id)
    }

    pub fn local_type(&self, id: LocalId) -> Option<TypeId<I>> {
        self.locals.get(&id).cloned()
    }

    pub fn place_type(&self, id: PlaceId) -> Option<TypeId<I>> {
        self.places.get(&id).cloned()
    }

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
