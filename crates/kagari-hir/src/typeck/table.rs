use std::collections::HashMap;

use super::ScalarValue;
use crate::builtin::{BuiltinFunction, surface::StandardIntrinsic};
use crate::hir::{ExprId, FieldId, FunctionId, LocalId, PatternId, PlaceId, StructId};
use crate::types::TypeId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConstraintTarget {
    Standard(crate::builtin::surface::StandardTypeConstraint),
    Trait(crate::hir::TraitId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeTarget {
    Struct(crate::hir::StructId),
    Enum(crate::hir::EnumId),
    Trait(crate::hir::TraitId),
    Generic(crate::hir::GenericParamId),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedTypeRef {
    pub ty: TypeId,
    pub target: Option<TypeTarget>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallTarget {
    SourceFunction(crate::imports::SourceFunctionId),
    HostFunction(crate::host::HostFunctionId),
    Function(FunctionId),
    StandardIntrinsic(StandardIntrinsic),
    RuntimeHelper(BuiltinFunction),
    TraitMethod(FunctionId),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedCall {
    pub target: CallTarget,
    /// Evaluated before explicit arguments, exactly once.
    pub receiver: Option<ExprId>,
    /// Declaration parameter order; arguments may refer to an enclosing binder.
    pub type_arguments: Vec<TypeId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedStructInit {
    pub structure: StructId,
    /// Source order, including holes for unknown initializer fields.
    pub fields: Vec<Option<FieldId>>,
}

#[derive(Debug, Clone, Default)]
pub struct TypeTable {
    implementations: HashMap<(crate::hir::TraitId, TypeId), HashMap<FunctionId, FunctionId>>,
    constraints: HashMap<crate::hir::TypeRefId, Option<ConstraintTarget>>,
    type_refs: HashMap<crate::hir::TypeRefId, ResolvedTypeRef>,
    field_types: HashMap<FieldId, TypeId>,
    expr_fields: HashMap<ExprId, FieldId>,
    place_fields: HashMap<PlaceId, FieldId>,
    struct_inits: HashMap<ExprId, ResolvedStructInit>,
    exprs: HashMap<ExprId, TypeId>,
    locals: HashMap<LocalId, TypeId>,
    places: HashMap<PlaceId, TypeId>,
    calls: HashMap<ExprId, ResolvedCall>,
    scalars: HashMap<ExprId, ScalarValue>,
    pattern_scalars: HashMap<PatternId, ScalarValue>,
}

impl TypeTable {
    pub(crate) fn insert_implementation(
        &mut self,
        trait_id: crate::hir::TraitId,
        ty: TypeId,
        methods: HashMap<FunctionId, FunctionId>,
    ) {
        self.implementations
            .entry((trait_id, ty))
            .or_insert(methods);
    }
    pub fn implements(&self, trait_id: crate::hir::TraitId, ty: &TypeId) -> bool {
        self.implementations.contains_key(&(trait_id, ty.clone()))
    }
    pub fn implementation_method(&self, method: FunctionId, ty: &TypeId) -> Option<FunctionId> {
        self.implementations
            .iter()
            .filter(|((_, target), _)| target == ty)
            .find_map(|(_, methods)| methods.get(&method).copied())
    }
    pub(crate) fn insert_constraint(
        &mut self,
        id: crate::hir::TypeRefId,
        target: Option<ConstraintTarget>,
    ) {
        self.constraints.insert(id, target);
    }
    pub fn constraint(&self, id: crate::hir::TypeRefId) -> Option<ConstraintTarget> {
        self.constraints.get(&id).copied().flatten()
    }
    pub(crate) fn has_constraint(&self, id: crate::hir::TypeRefId) -> bool {
        self.constraints.contains_key(&id)
    }
    pub(crate) fn insert_type_ref(&mut self, id: crate::hir::TypeRefId, resolved: ResolvedTypeRef) {
        self.type_refs.insert(id, resolved);
    }
    pub fn type_ref(&self, id: crate::hir::TypeRefId) -> Option<&ResolvedTypeRef> {
        self.type_refs.get(&id)
    }
    pub(crate) fn insert_field_type(&mut self, field: FieldId, ty: TypeId) {
        self.field_types.insert(field, ty);
    }
    pub(crate) fn insert_expr_field(&mut self, expr: ExprId, field: FieldId) {
        self.expr_fields.insert(expr, field);
    }
    pub(crate) fn insert_place_field(&mut self, place: PlaceId, field: FieldId) {
        self.place_fields.insert(place, field);
    }
    pub(crate) fn insert_struct_init(&mut self, expr: ExprId, target: ResolvedStructInit) {
        self.struct_inits.insert(expr, target);
    }
    pub fn field_type(&self, field: FieldId) -> Option<TypeId> {
        self.field_types.get(&field).cloned()
    }
    pub fn expr_field(&self, expr: ExprId) -> Option<FieldId> {
        self.expr_fields.get(&expr).copied()
    }
    pub fn place_field(&self, place: PlaceId) -> Option<FieldId> {
        self.place_fields.get(&place).copied()
    }
    pub fn struct_init(&self, expr: ExprId) -> Option<&ResolvedStructInit> {
        self.struct_inits.get(&expr)
    }
    pub(crate) fn restore_function(
        &mut self,
        old: &Self,
        old_map: &crate::source_map::SourceMap,
        new_map: &crate::source_map::SourceMap,
        old_span: kagari_common::Span,
        new_span: kagari_common::Span,
    ) -> bool {
        fn remap(
            old: &[kagari_common::Span],
            new: &[kagari_common::Span],
            old_span: kagari_common::Span,
            new_span: kagari_common::Span,
        ) -> Option<Vec<(usize, usize)>> {
            let relative = |spans: &[kagari_common::Span], owner: kagari_common::Span| {
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
            .map(|(a, b)| (ExprId::new(*a), ExprId::new(*b)))
            .collect::<HashMap<_, _>>();
        let mut calls = Vec::new();
        for (old_id, new_id) in &expr_ids {
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
                        target: call.target,
                        receiver,
                        type_arguments: call.type_arguments.clone(),
                    },
                ));
            }
        }
        self.calls.extend(calls);
        for (a, b) in types {
            if let Some(ty) = old.type_refs.get(&crate::hir::TypeRefId::new(a)) {
                self.type_refs
                    .insert(crate::hir::TypeRefId::new(b), ty.clone());
            }
        }
        for (a, b) in patterns {
            if let Some(value) = old.pattern_scalars.get(&PatternId::new(a)) {
                self.pattern_scalars
                    .insert(PatternId::new(b), value.clone());
            }
        }
        for (a, b) in exprs {
            if let Some(field) = old.expr_fields.get(&ExprId::new(a)) {
                self.expr_fields.insert(ExprId::new(b), *field);
            }
            if let Some(target) = old.struct_inits.get(&ExprId::new(a)) {
                self.struct_inits.insert(ExprId::new(b), target.clone());
            }
            if let Some(value) = old.scalars.get(&ExprId::new(a)) {
                self.scalars.insert(ExprId::new(b), value.clone());
            }
            if let Some(ty) = old.exprs.get(&ExprId::new(a)) {
                self.exprs.insert(ExprId::new(b), ty.clone());
            }
        }
        for (a, b) in locals {
            if let Some(ty) = old.locals.get(&LocalId::new(a)) {
                self.locals.insert(LocalId::new(b), ty.clone());
            }
        }
        for (a, b) in places {
            if let Some(field) = old.place_fields.get(&PlaceId::new(a)) {
                self.place_fields.insert(PlaceId::new(b), *field);
            }
            if let Some(ty) = old.places.get(&PlaceId::new(a)) {
                self.places.insert(PlaceId::new(b), ty.clone());
            }
        }
        true
    }
    pub(crate) fn insert_expr(&mut self, id: ExprId, ty: TypeId) {
        self.exprs.insert(id, ty);
    }

    pub(crate) fn insert_scalar(&mut self, id: ExprId, value: ScalarValue) {
        self.scalars.insert(id, value);
    }
    pub(crate) fn insert_pattern_scalar(&mut self, id: PatternId, value: ScalarValue) {
        self.pattern_scalars.insert(id, value);
    }
    pub fn scalar_value(&self, id: ExprId) -> Option<&ScalarValue> {
        self.scalars.get(&id)
    }
    pub fn pattern_scalar_value(&self, id: PatternId) -> Option<&ScalarValue> {
        self.pattern_scalars.get(&id)
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
            },
        );
    }

    pub(crate) fn insert_type_arguments(&mut self, id: ExprId, arguments: Vec<TypeId>) {
        self.calls
            .get_mut(&id)
            .expect("resolved generic call")
            .type_arguments = arguments;
    }

    pub fn expr_type(&self, id: ExprId) -> Option<TypeId> {
        self.exprs.get(&id).cloned()
    }

    pub fn local_type(&self, id: LocalId) -> Option<TypeId> {
        self.locals.get(&id).cloned()
    }

    pub fn place_type(&self, id: PlaceId) -> Option<TypeId> {
        self.places.get(&id).cloned()
    }

    pub fn call_resolution(&self, id: ExprId) -> Option<ResolvedCall> {
        self.calls.get(&id).cloned()
    }
}
