//! Type assignment/read places and validate field, index and writeability requirements.
//! PlaceId facts describe the checked destination independently of expression values;
//! this stage checks access contracts without performing a mutation.

use crate::{
    aggregates::FieldSignature,
    hir::{
        expr::ExprKind,
        ids::{ExprId, PlaceId},
        place::PlaceKind,
    },
    resolver::resolved::ResolvedName,
    typeck::{
        BodyTypeEnv, body::BodyChecker, completion, scalar::ScalarValue, ty::display_type_id,
    },
    types::{
        NominalType, TypeId, TypeSubstitution,
        semantic::{lower_type, raise_type},
    },
};
use kagari_common::identity::{DefinitionKind, ModuleIdentity, PackageId, table::DefinitionId};
use kagari_source::diagnostic::{Diagnostic, DiagnosticKind};
use kagari_types::{
    collection::CollectionAccess,
    declaration::names::NameNamespace,
    language::Protocol,
    reference::{ReferenceAccess, field_read_type},
    scalar::{BuiltinType, IntegerType},
};

impl<'a> BodyChecker<'a> {
    pub(super) fn resolve_assignment_target_type(
        &mut self,
        place_id: PlaceId,
        env: &mut BodyTypeEnv,
    ) -> Option<TypeId> {
        if let Some(ty) = self.infer_host_path_write(place_id, env) {
            self.type_table.insert_place(place_id, ty.clone());
            return Some(ty);
        }
        let ty = match &self.lowered.module.place(place_id).kind {
            PlaceKind::Expr(expr) => {
                self.infer_expr_type(*expr, env);
                None
            }
            PlaceKind::Name(_) => {
                let ty = self.resolve_readable_place_type(place_id, env);
                let writable = self
                    .place_root_resolution(place_id)
                    .is_some_and(|resolved| match resolved {
                        ResolvedName::Local(id) => env
                            .local_writeability
                            .get(&id)
                            .is_some_and(|access| access.is_var()),
                        ResolvedName::Static(id) => {
                            self.lowered.module.static_item(id).writeability.is_var()
                        }
                        ResolvedName::Source(source) => self
                            .top_level_index
                            .imported_statics
                            .get(&source)
                            .is_some_and(|item| item.writeability.is_var()),
                        _ => false,
                    });
                ty.filter(|_| writable)
            }
            PlaceKind::Field { base, name } => {
                let base_ty = self.resolve_readable_place_type(*base, env)?;
                if base_ty.is_never() {
                    self.type_table.insert_place(place_id, base_ty.clone());
                    return Some(base_ty);
                }
                let field = self.resolve_field(&base_ty, name)?;
                let id = field.id;
                let ty = field.ty.clone();
                let writable = field.writeability.is_var()
                    && !matches!(
                        base_ty,
                        TypeId::Reference {
                            access: ReferenceAccess::Shared,
                            ..
                        }
                    );
                self.type_table.insert_place_field(place_id, id);
                self.type_table.insert_place(place_id, ty.clone());
                writable.then_some(ty)
            }
            PlaceKind::Index { base, index } => {
                let base_ty = self
                    .resolve_readable_place_type(*base, env)
                    .unwrap_or(TypeId::Error);
                let context = self
                    .trait_bounds_for(&base_ty, env)
                    .into_iter()
                    .find(|bound| {
                        Protocol::from_definition(
                            &bound.declaration,
                            self.declarations.definitions(),
                        ) == Some(Protocol::Index)
                    })
                    .and_then(|bound| bound.arguments.into_iter().next());
                let index_ty = self.infer_expr_type_expected(*index, env, context.as_ref());
                if base_ty.is_never() {
                    self.type_table.insert_place(place_id, base_ty.clone());
                    return Some(base_ty);
                }
                if !matches!(base_ty, TypeId::Array(_) | TypeId::Tuple(_)) {
                    let mut requested = self.aggregates.language_trait(Protocol::Index)?;
                    requested.arguments.push(index_ty.clone());
                    let (read, item) = self.select_operator(&base_ty, requested, env)?;
                    self.type_table.insert_place_index(place_id, read);
                    self.type_table.insert_place(place_id, item.clone());
                    let (write, method) =
                        self.select_index_write(&base_ty, &index_ty, &item, env)?;
                    self.type_table
                        .insert_place_index_write(place_id, write, method);
                    return Some(item);
                }
                let ty = self.resolve_index_type(*index, &base_ty);
                let fact = ty.clone().or_else(|| match &base_ty {
                    TypeId::Array(element) => Some((**element).clone()),
                    _ => None,
                });
                if let Some(fact) = fact {
                    self.type_table.insert_place(place_id, fact);
                }
                if matches!(base_ty, TypeId::Tuple(_)) {
                    self.resolve_assignment_target_type(*base, env)?;
                }
                ty
            }
        };

        if let Some(ty) = ty.clone() {
            self.type_table.insert_place(place_id, ty);
        }

        ty
    }

    /// Preserve the installed list assignment spelling as a checked method call.
    /// Index reads are general; a coincidentally named `set` on another interface
    /// does not acquire assignment syntax or native storage authority.
    fn select_index_write(
        &self,
        receiver: &TypeId,
        index: &TypeId,
        item: &TypeId,
        env: &BodyTypeEnv,
    ) -> Option<(NominalType, DefinitionId)> {
        let definitions = self.declarations.definitions();
        let module = definitions.lookup_root(&ModuleIdentity {
            package: PackageId("std".into()),
            path: vec!["collections".into()],
        })?;
        let owner = definitions.lookup_child(module, DefinitionKind::Trait, "MutableList", 0)?;
        let member = definitions.lookup_child(owner, DefinitionKind::Method, "set", 0)?;
        let mut candidates = self
            .trait_bounds_for(receiver, env)
            .into_iter()
            .filter(|bound| {
                bound.declaration == owner && bound.arguments.as_slice() == [item.clone()]
            });
        let interface = candidates.next()?;
        if candidates.next().is_some() {
            return None;
        }
        let contract = self.aggregates.trait_(&owner)?;
        let method = self.aggregates.trait_method(&member)?;
        if method.generic_params != contract.generic_params
            || method.bounds.values().any(|bounds| !bounds.is_empty())
        {
            return None;
        }
        let mut substitution: TypeSubstitution = contract
            .generic_params
            .iter()
            .cloned()
            .zip(interface.arguments.iter().cloned())
            .collect();
        substitution.insert_receiver(owner, receiver.clone());
        let apply = |ty: &TypeId| {
            self.aggregates.normalize_type(
                definitions,
                &ty.instantiate(&substitution)
                    .with_associated_types(&interface, receiver),
            )
        };
        let parameters: Vec<_> = method.params.iter().map(|param| apply(&param.ty)).collect();
        (parameters == [receiver.clone(), index.clone(), item.clone()]
            && *index == TypeId::Builtin(BuiltinType::U64)
            && apply(&method.return_type) == TypeId::Builtin(BuiltinType::Unit))
        .then_some((interface, member))
    }

    pub(super) fn resolve_readable_place_type(
        &mut self,
        place_id: PlaceId,
        env: &mut BodyTypeEnv,
    ) -> Option<TypeId> {
        // Host-target probing and assignment diagnostics may revisit a place.
        // Its checked facts already include any index diagnostics and targets.
        if let Some(ty) = self.type_table.place_type(place_id) {
            return Some(ty);
        }
        let ty = match &self.lowered.module.place(place_id).kind {
            PlaceKind::Expr(expr) => Some(self.infer_expr_type(*expr, env)),
            PlaceKind::Name(_) => {
                self.place_root_resolution(place_id)
                    .and_then(|resolved| match resolved {
                        ResolvedName::Param(id) => env.params.get(&id).cloned(),
                        ResolvedName::Local(id) => env.locals.get(&id).cloned(),
                        ResolvedName::Const(id) => self.top_level_index.consts.get(&id).cloned(),
                        ResolvedName::Static(id) => self.top_level_index.statics.get(&id).cloned(),
                        ResolvedName::Source(source) => self
                            .top_level_index
                            .imported_statics
                            .get(&source)
                            .map(|item| {
                                self.aggregates
                                    .normalize_type(self.declarations.definitions(), &item.ty)
                            }),
                        ResolvedName::Function(_)
                        | ResolvedName::Namespace(_)
                        | ResolvedName::Module(_)
                        | ResolvedName::RuntimeHelper(_)
                        | ResolvedName::OpaqueType(_)
                        | ResolvedName::Struct(_)
                        | ResolvedName::Enum(_)
                        | ResolvedName::Trait(_) => None,
                    })
            }
            PlaceKind::Field { base, name } => {
                let base_ty = self.resolve_readable_place_type(*base, env)?;
                if base_ty.is_never() {
                    self.type_table.insert_place(place_id, base_ty.clone());
                    return Some(base_ty);
                }
                let field = self.resolve_field(&base_ty, name)?;
                let (id, ty) = (field.id, self.field_read_type(&base_ty, &field)?);
                self.type_table.insert_place_field(place_id, id);
                Some(ty)
            }
            PlaceKind::Index { base, index } => {
                let base_ty = self
                    .resolve_readable_place_type(*base, env)
                    .unwrap_or(TypeId::Error);
                let context = self
                    .trait_bounds_for(&base_ty, env)
                    .into_iter()
                    .find(|bound| {
                        Protocol::from_definition(
                            &bound.declaration,
                            self.declarations.definitions(),
                        ) == Some(Protocol::Index)
                    })
                    .and_then(|bound| bound.arguments.into_iter().next());
                let index_ty = self.infer_expr_type_expected(*index, env, context.as_ref());
                if !matches!(base_ty, TypeId::Array(_) | TypeId::Tuple(_))
                    && let Some(requested) =
                        self.aggregates
                            .language_trait(Protocol::Index)
                            .map(|mut requested| {
                                requested.arguments.push(index_ty.clone());
                                requested
                            })
                    && let Some((interface, result)) =
                        self.select_operator(&base_ty, requested, env)
                {
                    self.type_table.insert_place_index(place_id, interface);
                    Some(result)
                } else {
                    self.checked_index_type(*index, &base_ty, &index_ty, *index, env)
                }
            }
        };

        if let Some(ty) = ty.clone() {
            self.type_table.insert_place(place_id, ty);
        }

        ty
    }

    pub(super) fn assignment_target_error_reason(
        &self,
        place_id: PlaceId,
        env: &BodyTypeEnv,
    ) -> String {
        match &self.lowered.module.place(place_id).kind {
            PlaceKind::Expr(_) => "temporary value cannot be reassigned".to_owned(),
            PlaceKind::Name(_) => self
                .place_root_resolution(place_id)
                .map(|resolved| match resolved {
                    ResolvedName::Param(_) => {
                        "function parameters are `val` bindings and cannot be reassigned"
                            .to_string()
                    }
                    ResolvedName::Local(id) => match env.local_writeability.get(&id).copied() {
                        Some(writeability) if !writeability.is_var() => {
                            "`val` binding cannot be reassigned".to_string()
                        }
                        Some(_) => "assignment target type could not be resolved".to_string(),
                        None => "unresolved assignment target".to_string(),
                    },
                    ResolvedName::Const(_) => "`const` item cannot be reassigned".to_string(),
                    ResolvedName::Static(id) => {
                        if self.lowered.module.static_item(id).writeability.is_var() {
                            "assignment target type could not be resolved".to_string()
                        } else {
                            "`static val` binding cannot be reassigned".to_string()
                        }
                    }
                    ResolvedName::Source(source)
                        if self.top_level_index.imported_statics.get(&source).is_some() =>
                    {
                        "`static val` binding cannot be reassigned".to_string()
                    }
                    ResolvedName::Function(_) => "function item is not assignable".to_string(),
                    ResolvedName::Source(_)
                    | ResolvedName::Namespace(_)
                    | ResolvedName::Module(_) => "module item is not assignable".to_string(),
                    ResolvedName::RuntimeHelper(_) => {
                        "standard function item is not assignable".to_string()
                    }
                    ResolvedName::OpaqueType(_) => "opaque type is not assignable".to_string(),
                    ResolvedName::Struct(_) => "struct type is not assignable".to_string(),
                    ResolvedName::Enum(_) => "enum type is not assignable".to_string(),
                    ResolvedName::Trait(_) => "trait type is not assignable".to_string(),
                })
                .unwrap_or_else(|| "unresolved assignment target".to_string()),
            PlaceKind::Field { base, name } => {
                let Some(base_ty) = self.type_table.place_type(*base) else {
                    return self.assignment_target_error_reason(*base, env);
                };
                if matches!(
                    base_ty,
                    TypeId::Reference {
                        access: ReferenceAccess::Shared,
                        ..
                    }
                ) {
                    return "shared reference cannot assign a host field".to_string();
                }
                match self.resolve_field(&base_ty, name) {
                    Some(field) if !field.writeability.is_var() => {
                        format!("`val` field `{name}` cannot be assigned")
                    }
                    Some(_) => "assignment target type could not be resolved".to_string(),
                    None => format!("unknown field `{name}`"),
                }
            }
            PlaceKind::Index { base, index } => {
                let Some(base_ty) = self.type_table.place_type(*base) else {
                    return self.assignment_target_error_reason(*base, env);
                };
                if base_ty.collection_access() == Some(CollectionAccess::ReadOnly)
                    || (self.type_table.place_index(place_id).is_some()
                        && self.type_table.place_index_write(place_id).is_none())
                {
                    "indexed assignment requires an applicable write operation".to_string()
                } else if self.resolve_index_type(*index, &base_ty).is_none() {
                    "indexed value is not assignable".to_string()
                } else {
                    "assignment target type could not be resolved".to_string()
                }
            }
        }
    }

    pub(super) fn place_root_resolution(&self, place_id: PlaceId) -> Option<ResolvedName> {
        let root = self.place_root(place_id);
        self.names.place_resolution(root)
    }

    pub(super) fn place_root(&self, place_id: PlaceId) -> PlaceId {
        match &self.lowered.module.place(place_id).kind {
            PlaceKind::Name(_) | PlaceKind::Expr(_) => place_id,
            PlaceKind::Field { base, .. } | PlaceKind::Index { base, .. } => self.place_root(*base),
        }
    }

    pub(super) fn checked_member_type(
        &mut self,
        receiver: &TypeId,
        name: &str,
        site: ExprId,
        write: bool,
    ) -> TypeId {
        if let Some(field) = self.resolve_field(receiver, name) {
            self.type_table.insert_expr_field(site, field.id);
            if write && !field.writeability.is_var() {
                self.diagnostics.push(
                    Diagnostic::error(DiagnosticKind::InvalidAssignmentTarget {
                        reason: format!("field `{name}` is read-only"),
                    })
                    .with_span(self.lowered.source_map.expr_span(site)),
                );
            }
            // Read-only targets still provide RHS context and independent errors.
            if write {
                return field.ty;
            }
            if let Some(ty) = self.field_read_type(receiver, &field) {
                return ty;
            }
            self.diagnostics.push(
                Diagnostic::error(DiagnosticKind::InvalidReference {
                    reason: "field read requires an inline struct or owned scalar/handle reader",
                })
                .with_span(self.lowered.source_map.expr_span(site)),
            );
            return TypeId::Error;
        }
        if !matches!(receiver, TypeId::Unknown | TypeId::Error) {
            self.diagnostics.push(
                Diagnostic::error(DiagnosticKind::UnknownName {
                    name: name.to_owned(),
                })
                .with_span(self.lowered.source_map.expr_span(site)),
            );
        }
        TypeId::Error
    }

    pub(super) fn const_root_name(&self, expr_id: ExprId) -> Option<String> {
        match &self.lowered.module.expr(expr_id).kind {
            ExprKind::Name { .. } => match self.names.expr_resolution(expr_id) {
                Some(ResolvedName::Const(id)) => self
                    .lowered
                    .module
                    .consts
                    .iter()
                    .find(|item| item.id == id)
                    .map(|item| item.name.clone()),
                _ => None,
            },
            ExprKind::Field { receiver, .. } | ExprKind::Index { receiver, .. } => {
                self.const_root_name(*receiver)
            }
            _ => None,
        }
    }

    pub(super) fn resolve_field(
        &self,
        receiver: &TypeId,
        field_name: &str,
    ) -> Option<FieldSignature> {
        let receiver = match receiver {
            TypeId::Reference { target, .. } => target.as_ref(),
            _ => receiver,
        };
        let TypeId::Struct(id) = receiver else {
            return None;
        };
        let structure = self.aggregates.structure(&id.declaration)?;
        if structure.generic_params.len() != id.arguments.len() {
            return None;
        }
        let substitution = structure
            .generic_params
            .iter()
            .cloned()
            .zip(id.arguments.iter().cloned())
            .collect();
        let mut field = structure
            .fields
            .iter()
            .find(|field| {
                field.name == field_name
                    && field.visibility.allows(
                        self.declarations
                            .definitions()
                            .resolve(field.owner)
                            .expect("scoped member owner")
                            .module(),
                        self.lowered.source.module_identity(),
                    )
            })?
            .clone();
        field.ty = self.aggregates.normalize_type(
            self.declarations.definitions(),
            &field.ty.instantiate(&substitution),
        );
        Some(field)
    }

    fn field_read_type(&self, receiver: &TypeId, field: &FieldSignature) -> Option<TypeId> {
        if field.ty.is_unresolved() {
            return Some(field.ty.clone());
        }
        match receiver {
            TypeId::Reference { access, .. } => {
                field_read_type(&lower_type(&field.ty), *access, field.writeability.is_var())
                    .as_ref()
                    .map(raise_type)
            }
            _ => Some(field.ty.clone()),
        }
    }

    pub(super) fn resolve_struct_id(&self, path: &str) -> Option<DefinitionId> {
        let resolved = self.declarations.resolve_name(path, NameNamespace::Type)?;
        if matches!(resolved, ResolvedName::Struct(_)) {
            return self.declarations.definition(resolved).cloned();
        }
        let TypeId::Struct(id) = &self.declarations.imported_types().resolved(resolved)?.ty else {
            return None;
        };
        Some(id.declaration)
    }

    pub(super) fn resolve_enum_id(&self, path: &str) -> Option<DefinitionId> {
        let resolved = self.declarations.resolve_name(path, NameNamespace::Type)?;
        if matches!(resolved, ResolvedName::Enum(_)) {
            return self.declarations.definition(resolved).cloned();
        }
        let TypeId::Enum(id) = &self.declarations.imported_types().resolved(resolved)?.ty else {
            return None;
        };
        Some(id.declaration)
    }

    pub(super) fn checked_index_type(
        &mut self,
        index: ExprId,
        receiver: &TypeId,
        index_ty: &TypeId,
        site: ExprId,
        env: &BodyTypeEnv,
    ) -> Option<TypeId> {
        let result = self.resolve_index_type(index, receiver);
        if result.is_none()
            && !matches!(receiver, TypeId::Unknown | TypeId::Error)
            && !matches!(index_ty, TypeId::Unknown | TypeId::Error)
        {
            self.diagnostics.push(
                Diagnostic::error(DiagnosticKind::InvalidIndexTarget {
                    type_name: display_type_id(self.declarations.definitions(), receiver),
                })
                .with_span(self.lowered.source_map.expr_span(site)),
            );
        }
        // An invalid index does not erase an array's or unique Index result contract.
        // Keep the diagnostic above, but let downstream member queries recover.
        // A tuple still needs a valid constant index to select a member.
        result.or_else(|| match receiver {
            TypeId::Array(element) => Some((**element).clone()),
            _ => {
                let index = Protocol::Index.definition(self.declarations.definitions())?;
                let mut outputs = self
                    .trait_bounds_for(receiver, env)
                    .into_iter()
                    .filter(|interface| interface.declaration == index)
                    .filter_map(|interface| self.select_operator(receiver, interface, env))
                    .map(|(_, output)| output);
                let output = outputs.next()?;
                outputs.next().is_none().then_some(output)
            }
        })
    }

    pub(super) fn resolve_index_type(
        &self,
        index_expr: ExprId,
        receiver: &TypeId,
    ) -> Option<TypeId> {
        if receiver.is_never() {
            return Some(receiver.clone());
        }
        if !completion::expr_can_complete(
            &self.lowered.module,
            self.names,
            self.type_table,
            index_expr,
            self.cancel,
        )
        .ok()?
        {
            return match receiver {
                TypeId::Array(element) => Some((**element).clone()),
                // No index value exists to select a particular Tuple member.
                TypeId::Tuple(_) => Some(TypeId::Unknown),
                _ => None,
            };
        }
        if !self.type_table.expr_type(index_expr)?.is_integer()
            && !(matches!(receiver, TypeId::Tuple(_)) && self.tuple_index(index_expr).is_some())
        {
            return None;
        }
        match receiver {
            TypeId::Array(element) => Some((**element).clone()),
            TypeId::Tuple(elements) => self
                .tuple_index(index_expr)
                .and_then(|index| elements.get(index).cloned()),
            _ => None,
        }
    }

    pub(super) fn tuple_index(&self, index_expr: ExprId) -> Option<usize> {
        let literal;
        let scalar = if let Some(value) = self.type_table.scalar_value(index_expr) {
            value
        } else if let ExprKind::Literal(value) = &self.lowered.module.expr(index_expr).kind {
            // Tuple positions select a type even before unsuffixed numbers default.
            literal = ScalarValue::parse(value).ok()?;
            &literal
        } else {
            return None;
        };
        match scalar {
            ScalarValue::Integer {
                value,
                ty: IntegerType::I32,
            } => usize::try_from(*value).ok(),
            _ => None,
        }
    }

    pub(super) fn check_const_write(&mut self, expr_id: ExprId) {
        if let Some(const_name) = self.const_root_name(expr_id) {
            self.diagnostics.push(
                Diagnostic::error(DiagnosticKind::ConstWriteNotAllowed { const_name })
                    .with_span(self.lowered.source_map.expr_span(expr_id)),
            );
        }
    }
}
