//! Transparent alias normalization and definition validation.

use crate::{
    DiagnosticBuffer,
    aggregates::{AggregateCatalog, implementations::ImplementationSearchError},
    declarations::Declarations,
    hir::ids::{BodyOwner, HirOwner, TypeRefId},
    hir::{
        item::{alias::TypeAlias, behavior::AssociatedType},
        ty::TypeKind,
    },
    imports::types::aliases::{CanonicalAlias, alias_references},
    lower::LoweredModule,
    resolver::resolved::ResolvedName,
    typeck::{
        GenericBounds, ModuleSignatures, applications, associated, check, constraints, interfaces,
        table::{TypeTable, TypeTarget},
        ty::{TypeContext, resolve_type_in},
    },
    types::{TypeId, TypeSubstitution},
};
use kagari_common::{cancellation::CancellationToken, span::Span};
use kagari_source::diagnostic::{Diagnostic, DiagnosticKind};
use std::iter;

pub(crate) fn prepare(
    lowered: &LoweredModule,
    alias: &TypeAlias,
    declarations: &Declarations,
    cancel: &CancellationToken,
) -> CanonicalAlias {
    let identity = declarations
        .definition(ResolvedName::TypeAlias(alias.id))
        .expect("alias declaration");
    let mut table = TypeTable::default();
    let mut diagnostics = DiagnosticBuffer::default();
    constraints::resolve_owner(
        lowered,
        &alias.generic_params,
        &alias.bounds,
        declarations,
        &mut table,
        &mut diagnostics,
        cancel,
    );
    let context = TypeContext {
        declarations,
        generics: &alias.generic_params,
        self_type: None,
        implementation: None,
    };
    let target = alias
        .target
        .map(|target| resolve_type_in(&lowered.module, target, context, &mut table, cancel))
        .unwrap_or(TypeId::Error);
    let mut bounds = constraints::parameter_bounds(&alias.generic_params, declarations, &table);
    for bound in &alias.bounds {
        if let Some(target) = table.type_ref(bound.target_ref) {
            bounds.entry(target.ty.clone()).or_default().extend(
                bound
                    .traits
                    .iter()
                    .filter_map(|reference| table.constraint(reference.ty)),
            );
        }
    }
    let invalid_attribute = lowered.attributes.iter().find(|attribute| {
        attribute.target_span == lowered.source_map.type_alias_span(alias.id)
            && attribute.name != "meta"
            && !attribute.name.starts_with("tool::")
    });
    let error = if let Some(attribute) = invalid_attribute {
        Some(format!(
            "attribute `{}` is not supported on a transparent type alias; aliases cannot declare native storage or language roles",
            attribute.name
        ))
    } else if target.is_unresolved() {
        Some("target does not resolve to a complete type (check alias argument counts and expansion limits)".into())
    } else if !within_expansion_limit(&target, &TypeSubstitution::default(), cancel) {
        Some("type alias expansion exceeds 4096 type nodes or 64 nested types".into())
    } else if !diagnostics.is_empty() {
        Some("invalid generic bounds in type alias declaration".into())
    } else {
        None
    };
    CanonicalAlias {
        parameters: declarations.parameters_of(identity),
        target: if error.is_some() {
            TypeId::Error
        } else {
            target
        },
        bounds,
        error,
    }
}

/// Space for children retained in one normalized composite, not intermediate
/// alias arguments. Exhaustion preserves syntax traversal but suppresses values.
pub(crate) struct ExpansionBudget {
    pub remaining: usize,
    pub exceeded: bool,
}

impl ExpansionBudget {
    pub(crate) fn new(remaining: usize) -> Self {
        Self {
            remaining,
            exceeded: false,
        }
    }

    pub(crate) fn check(
        &mut self,
        ty: &TypeId,
        substitution: &TypeSubstitution,
        cancel: &CancellationToken,
    ) -> Option<usize> {
        if !self.exceeded
            && let Some(size) = expansion_size(ty, substitution, cancel)
            && size <= self.remaining
        {
            return Some(size);
        }
        self.exceeded = true;
        self.remaining = 0;
        None
    }
}

/// Count substituted nodes before constructing an expanded type. Substitutions
/// are simultaneous: an argument's binders are not substituted a second time.
pub(crate) fn within_expansion_limit(
    ty: &TypeId,
    substitution: &TypeSubstitution,
    cancel: &CancellationToken,
) -> bool {
    expansion_size(ty, substitution, cancel).is_some()
}

pub(crate) fn expansion_size(
    ty: &TypeId,
    substitution: &TypeSubstitution,
    cancel: &CancellationToken,
) -> Option<usize> {
    let mut pending = vec![(ty, 0usize, true)];
    let mut count = 0usize;
    while let Some((ty, depth, substitute)) = pending.pop() {
        if cancel.check().is_err() || count >= 4096 || depth > 64 {
            return None;
        }
        let replacement = match ty {
            TypeId::Generic(parameter) => substitution.get(parameter),
            TypeId::SelfType(owner) => substitution.receiver(owner),
            _ => None,
        };
        if substitute && let Some(argument) = replacement {
            pending.push((argument, depth, false));
            continue;
        }
        count += 1;
        let mut push = |child| pending.push((child, depth + 1, substitute));
        match ty {
            TypeId::Tuple(items) => items.iter().for_each(&mut push),
            TypeId::Function { params, result } => {
                params.iter().for_each(&mut push);
                push(result);
            }
            TypeId::Array(item)
            | TypeId::Iter(item)
            | TypeId::Range(item, _)
            | TypeId::Reference { target: item, .. } => push(item),

            TypeId::Struct(nominal)
            | TypeId::Enum(nominal)
            | TypeId::Trait(nominal)
            | TypeId::NativeObject(nominal) => {
                nominal.arguments.iter().for_each(&mut push);
                nominal.associated_types.values().for_each(&mut push);
            }
            TypeId::Projection {
                arguments,
                receiver,
                interface,
                ..
            } => {
                push(receiver);
                arguments.iter().for_each(&mut push);
                interface.arguments.iter().for_each(&mut push);
                interface.associated_types.values().for_each(&mut push);
            }
            TypeId::Generic(_)
            | TypeId::SelfType(_)
            | TypeId::Host(_)
            | TypeId::Builtin(_)
            | TypeId::Error
            | TypeId::Unknown
            | TypeId::Inference(_) => {}
        }
        if pending.len() > 4096 {
            return None;
        }
    }
    Some(count)
}

/// Validate written alias applications even when their parameters disappear from
/// the normalized target. Context belongs to the original syntax owner.
pub(super) struct AliasValidation<'a> {
    pub lowered: &'a LoweredModule,
    pub declarations: &'a Declarations,
    pub table: &'a TypeTable,
    pub catalog: &'a AggregateCatalog,
    pub cancel: &'a CancellationToken,
}

impl AliasValidation<'_> {
    pub(super) fn uses(
        &self,
        select: impl Fn(TypeRefId) -> bool,
        bounds: &GenericBounds,
        diagnostics: &mut DiagnosticBuffer,
    ) {
        for index in 0..self.lowered.module.body.types.len() {
            let reference = self.lowered.source_map.type_id(index);
            if !select(reference) {
                continue;
            }
            self.application(reference, bounds, diagnostics);
        }
    }

    fn application(
        &self,
        reference: TypeRefId,
        bounds: &GenericBounds,
        diagnostics: &mut DiagnosticBuffer,
    ) {
        if self.cancel.check().is_err() {
            return;
        }
        let Some(TypeTarget::Source(id)) = self
            .table
            .type_ref(reference)
            .and_then(|reference| reference.target.as_ref())
        else {
            return;
        };
        let Some(alias) = self.declarations.imported_types().alias(id) else {
            return;
        };
        let args = match &self.lowered.module.type_ref(reference).kind {
            TypeKind::Named(_) => Vec::new(),
            TypeKind::Generic { args, .. } => args
                .iter()
                .map(|arg| {
                    self.table
                        .type_ref(*arg)
                        .map_or(TypeId::Error, |reference| reference.ty.clone())
                })
                .collect(),
            _ => return,
        };
        if args.len() != alias.parameters.len() {
            return;
        }
        let span = self.lowered.source_map.type_span(reference);
        for argument in &args {
            interfaces::validate_type(
                self.declarations.definitions(),
                argument,
                self.catalog,
                span,
                diagnostics,
                self.cancel,
            );
            applications::validate(
                applications::ApplicationContext {
                    definitions: self.declarations.definitions(),
                    catalog: self.catalog,
                    receiver: None,
                },
                argument,
                bounds,
                span,
                diagnostics,
                self.cancel,
            );
        }
        let substitution: TypeSubstitution = alias.parameters.iter().cloned().zip(args).collect();
        if matches!(
            applications::validate_requirements(
                applications::ApplicationContext {
                    definitions: self.declarations.definitions(),
                    catalog: self.catalog,
                    receiver: None
                },
                &alias.bounds,
                &substitution,
                bounds,
                span,
                diagnostics,
                self.cancel,
            ),
            Err(ImplementationSearchError::LimitExceeded)
        ) {
            diagnostics.push(
                Diagnostic::error(DiagnosticKind::CompileLimitExceeded {
                    resource: "type alias bound validation",
                    limit: 4096,
                })
                .with_span(span),
            );
        }
    }

    pub(super) fn signatures(
        &self,
        signatures: &ModuleSignatures,
        diagnostics: &mut DiagnosticBuffer,
    ) {
        for alias in &self.lowered.module.type_aliases {
            let Some(id) = self
                .declarations
                .definition(ResolvedName::TypeAlias(alias.id))
            else {
                continue;
            };
            let Some(template) = self.declarations.imported_types().alias(id) else {
                continue;
            };
            let span = self.lowered.source_map.type_alias_span(alias.id);
            check::validate_reference_position(&template.target, span, diagnostics);
            interfaces::validate_type(
                self.declarations.definitions(),
                &template.target,
                self.catalog,
                span,
                diagnostics,
                self.cancel,
            );
            applications::validate(
                applications::ApplicationContext {
                    definitions: self.declarations.definitions(),
                    catalog: self.catalog,
                    receiver: None,
                },
                &template.target,
                &template.bounds,
                span,
                diagnostics,
                self.cancel,
            );
        }
        for index in 0..self.lowered.module.body.types.len() {
            let reference = self.lowered.source_map.type_id(index);
            if self.cancel.check().is_err() {
                return;
            }
            let bounds = self.context_bounds(reference, signatures);
            self.application(reference, &bounds, diagnostics);
        }
    }

    fn context_bounds(&self, reference: TypeRefId, signatures: &ModuleSignatures) -> GenericBounds {
        if let HirOwner::Body(BodyOwner::Function(id)) = reference.owner() {
            return signatures
                .functions()
                .iter()
                .find(|function| function.id == id)
                .map(|function| function.bounds.clone())
                .unwrap_or_default();
        }
        let span = self.lowered.source_map.type_span(reference);
        let contains = |owner: Span| owner.start <= span.start && span.end <= owner.end;
        for alias in &self.lowered.module.type_aliases {
            if contains(self.lowered.source_map.type_alias_span(alias.id)) {
                return self
                    .declarations
                    .definition(ResolvedName::TypeAlias(alias.id))
                    .and_then(|id| self.declarations.imported_types().alias(id))
                    .map(|alias| alias.bounds.clone())
                    .unwrap_or_default();
            }
        }
        for item in &self.lowered.module.structs {
            if contains(self.lowered.source_map.struct_span(item.id)) {
                return constraints::parameter_bounds(
                    &item.generic_params,
                    self.declarations,
                    self.table,
                );
            }
        }
        for item in &self.lowered.module.enums {
            if contains(self.lowered.source_map.enum_span(item.id)) {
                return constraints::parameter_bounds(
                    &item.generic_params,
                    self.declarations,
                    self.table,
                );
            }
        }
        for item in &self.lowered.module.impls {
            if contains(self.lowered.source_map.impl_span(item.id)) {
                let mut bounds =
                    constraints::implementation_bounds(item, self.declarations, self.table);
                self.member_bounds(&item.associated_types, span, &mut bounds);
                return bounds;
            }
        }
        for item in &self.lowered.module.traits {
            if contains(self.lowered.source_map.trait_span(item.id)) {
                let mut bounds = constraints::parameter_bounds(
                    &item.generic_params,
                    self.declarations,
                    self.table,
                );
                self.member_bounds(&item.associated_types, span, &mut bounds);
                return bounds;
            }
        }
        GenericBounds::default()
    }

    fn member_bounds(&self, members: &[AssociatedType], span: Span, bounds: &mut GenericBounds) {
        for member in members {
            let references = member
                .ty
                .into_iter()
                .chain(member.bounds.iter().map(|bound| bound.ty))
                .chain(
                    member
                        .generic_params
                        .iter()
                        .flat_map(|parameter| &parameter.bounds)
                        .map(|bound| bound.ty),
                )
                .chain(member.parameter_bounds.iter().flat_map(|bound| {
                    iter::once(bound.target_ref).chain(bound.traits.iter().map(|bound| bound.ty))
                }));
            if references.into_iter().any(|reference| {
                let owner = self.lowered.source_map.type_span(reference);
                owner.start <= span.start && span.end <= owner.end
            }) {
                for (subject, requirements) in
                    associated::family_inputs(member, self.declarations, self.table).bounds
                {
                    bounds.entry(subject).or_default().extend(requirements);
                }
            }
        }
    }
}

pub(super) fn check_definitions(
    lowered: &LoweredModule,
    declarations: &Declarations,
    table: &mut TypeTable,
    diagnostics: &mut DiagnosticBuffer,
    cancel: &CancellationToken,
) {
    for alias in &lowered.module.type_aliases {
        if cancel.check().is_err() {
            return;
        }
        let Some(id) = declarations.definition(ResolvedName::TypeAlias(alias.id)) else {
            continue;
        };
        let Some(template) = declarations.imported_types().alias(id) else {
            continue;
        };
        if let Some(reason) = &template.error {
            diagnostics.push(
                Diagnostic::error(DiagnosticKind::InvalidTypeAlias {
                    name: alias.name.clone(),
                    reason: reason.clone(),
                })
                .with_span(lowered.source_map.type_alias_span(alias.id)),
            );
        }
        let context = TypeContext {
            declarations,
            generics: &alias.generic_params,
            self_type: None,
            implementation: None,
        };
        if let Some(target) = alias.target {
            resolve_type_in(&lowered.module, target, context, table, cancel);
        }
        // Bound validation remains at its existing semantic owner; this also
        // publishes navigation facts for declarations that are never used.
        constraints::resolve_owner(
            lowered,
            &alias.generic_params,
            &alias.bounds,
            declarations,
            table,
            diagnostics,
            cancel,
        );
        let Ok(references) = alias_references(&lowered.module, alias, cancel) else {
            return;
        };
        for reference in references {
            if matches!(&lowered.module.type_ref(reference).kind, TypeKind::Named(name) if name == "_")
            {
                diagnostics.push(
                    Diagnostic::error(DiagnosticKind::InvalidTypeAlias {
                        name: alias.name.clone(),
                        reason: "alias targets cannot contain inference placeholders".into(),
                    })
                    .with_span(lowered.source_map.type_span(reference)),
                );
            }
        }
    }
}
