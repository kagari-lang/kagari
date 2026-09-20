use std::collections::HashMap;

use super::ScalarValue;
use crate::builtin::{BuiltinFunction, surface::StandardIntrinsic};
use crate::hir::{ExprId, FieldId, FunctionId, LocalId, PatternId, PlaceId};
use crate::types::TypeId;
use kagari_common::identity::DefinitionId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConstraintTarget {
    Standard(crate::builtin::surface::StandardTypeConstraint),
    Trait(DefinitionId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeTarget {
    Host(crate::host::HostTypeId),
    Source(crate::imports::SourceTypeId),
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallTarget {
    SourceFunction(crate::imports::SourceFunctionId),
    HostFunction(crate::host::HostFunctionId),
    Function(FunctionId),
    StandardIntrinsic(StandardIntrinsic),
    RuntimeHelper(BuiltinFunction),
    TraitMethod(DefinitionId),
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
    pub structure: DefinitionId,
    /// Source order, including holes for unknown initializer fields.
    pub fields: Vec<Option<DefinitionId>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedEnumConstructor {
    pub enumeration: DefinitionId,
    /// Missing members keep their known enum owner for error recovery.
    pub variant: Option<DefinitionId>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct TypeTable {
    host_paths: HashMap<ExprId, ResolvedHostPath>,
    implementations: HashMap<(DefinitionId, TypeId), HashMap<DefinitionId, FunctionId>>,
    constraints: HashMap<crate::hir::TypeRefId, Option<ConstraintTarget>>,
    type_refs: HashMap<crate::hir::TypeRefId, ResolvedTypeRef>,
    field_types: HashMap<FieldId, TypeId>,
    expr_fields: HashMap<ExprId, DefinitionId>,
    place_fields: HashMap<PlaceId, DefinitionId>,
    struct_inits: HashMap<ExprId, ResolvedStructInit>,
    enum_constructors: HashMap<ExprId, ResolvedEnumConstructor>,
    exprs: HashMap<ExprId, TypeId>,
    locals: HashMap<LocalId, TypeId>,
    places: HashMap<PlaceId, TypeId>,
    calls: HashMap<ExprId, ResolvedCall>,
    scalars: HashMap<ExprId, ScalarValue>,
    pattern_scalars: HashMap<PatternId, ScalarValue>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedHostPath {
    pub root: ExprId,
    pub declaration: kagari_common::host_interface::HostFieldPathDeclaration,
    pub contract: kagari_common::host_interface::HostPathContract,
}

impl TypeTable {
    pub fn host_path(&self, expr: ExprId) -> Option<&ResolvedHostPath> {
        self.host_paths.get(&expr)
    }
    pub(crate) fn insert_host_path(&mut self, expr: ExprId, path: ResolvedHostPath) {
        self.host_paths.insert(expr, path);
    }
    #[cfg(test)]
    pub(crate) fn assert_same_source_facts(
        &self,
        other: &Self,
        arena: crate::hir::HirArenaId,
        other_arena: crate::hir::HirArenaId,
    ) {
        // Fresh analysis must have different local identities. Compare slot facts
        // only after checking every key/receiver belongs to its actual lowering.
        fn normalized(
            table: &TypeTable,
            from: crate::hir::HirArenaId,
            to: crate::hir::HirArenaId,
        ) -> TypeTable {
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
                        (crate::hir::$ty::new(to, id.owner(), id.index()), fact)
                    }).collect();
                )+};
            }
            keys!(host_paths: ExprId, constraints: TypeRefId, type_refs: TypeRefId, expr_fields: ExprId,
                place_fields: PlaceId, struct_inits: ExprId, enum_constructors: ExprId, exprs: ExprId, locals: LocalId,
                places: PlaceId, calls: ExprId, scalars: ExprId, pattern_scalars: PatternId);
            for call in result.calls.values_mut() {
                if let Some(receiver) = call.receiver {
                    assert_eq!(receiver.arena(), from);
                    call.receiver = Some(ExprId::new(to, receiver.owner(), receiver.index()));
                }
            }
            for path in result.host_paths.values_mut() {
                assert_eq!(path.root.arena(), from);
                path.root = ExprId::new(to, path.root.owner(), path.root.index());
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
        ids: &HashMap<crate::hir::TypeRefId, crate::hir::TypeRefId>,
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
        trait_id: DefinitionId,
        ty: TypeId,
        methods: HashMap<DefinitionId, FunctionId>,
    ) {
        self.implementations
            .entry((trait_id, ty))
            .or_insert(methods);
    }
    pub fn implements(&self, trait_id: &DefinitionId, ty: &TypeId) -> bool {
        self.implementations
            .contains_key(&(trait_id.clone(), ty.clone()))
    }
    pub fn implementation_method(&self, method: &DefinitionId, ty: &TypeId) -> Option<FunctionId> {
        self.implementations
            .iter()
            .filter(|((_, target), _)| target == ty)
            .find_map(|(_, methods)| methods.get(method).copied())
    }
    pub(crate) fn insert_constraint(
        &mut self,
        id: crate::hir::TypeRefId,
        target: Option<ConstraintTarget>,
    ) {
        self.constraints.insert(id, target);
    }
    pub fn constraint(&self, id: crate::hir::TypeRefId) -> Option<ConstraintTarget> {
        self.constraints.get(&id).cloned().flatten()
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
    pub(crate) fn insert_expr_field(&mut self, expr: ExprId, field: DefinitionId) {
        self.expr_fields.insert(expr, field);
    }
    pub(crate) fn insert_place_field(&mut self, place: PlaceId, field: DefinitionId) {
        self.place_fields.insert(place, field);
    }
    pub(crate) fn insert_struct_init(&mut self, expr: ExprId, target: ResolvedStructInit) {
        self.struct_inits.insert(expr, target);
    }
    pub fn field_type(&self, field: FieldId) -> Option<TypeId> {
        self.field_types.get(&field).cloned()
    }
    pub fn expr_field(&self, expr: ExprId) -> Option<&DefinitionId> {
        self.expr_fields.get(&expr)
    }
    pub fn place_field(&self, place: PlaceId) -> Option<&DefinitionId> {
        self.place_fields.get(&place)
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
            .map(|(a, b)| (old_map.expr_id(*a), new_map.expr_id(*b)))
            .collect::<HashMap<_, _>>();
        let mut calls = Vec::new();
        let mut host_paths = Vec::new();
        for (old_id, new_id) in &expr_ids {
            if let Some(path) = old.host_paths.get(old_id) {
                let Some(root) = expr_ids.get(&path.root) else {
                    return false;
                };
                host_paths.push((
                    *new_id,
                    ResolvedHostPath {
                        root: *root,
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
                    },
                ));
            }
        }
        self.calls.extend(calls);
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
        }
        for (a, b) in exprs {
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

    pub fn enum_constructor(&self, id: ExprId) -> Option<&ResolvedEnumConstructor> {
        self.enum_constructors.get(&id)
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
