//! Check written constraint applications after the aggregate catalog is complete.
use super::{ApplicationContext, REQUIREMENT_LIMITS, check_requirement, validate};
use crate::{
    DiagnosticBuffer,
    aggregates::{
        AggregateCatalog,
        implementations::{ImplementationSearchContext, ImplementationSearchError},
    },
    declarations::Declarations,
    hir::{
        ids::GenericParamId,
        item::{
            Module,
            behavior::{AssociatedType, TraitBound, TraitRef},
        },
    },
    resolver::resolved::ResolvedName,
    typeck::{GenericBounds, ModuleSignatures, constraints, table::TypeTable},
    types::TypeId,
};
use kagari_common::cancellation::CancellationToken;
use kagari_source::diagnostic::{Diagnostic, DiagnosticKind};

pub(super) fn check(
    module: &Module,
    declarations: &Declarations,
    signatures: &ModuleSignatures,
    catalog: &AggregateCatalog,
    diagnostics: &mut DiagnosticBuffer,
    cancel: &CancellationToken,
) {
    let table = signatures.type_table();
    let mut checker = Checker {
        module,
        table,
        declarations,
        catalog,
        diagnostics,
        cancel,
    };
    for (owner, params, predicates) in module
        .structs
        .iter()
        .map(|s| (ResolvedName::Struct(s.id), &s.generic_params, &s.bounds))
        .chain(
            module
                .enums
                .iter()
                .map(|e| (ResolvedName::Enum(e.id), &e.generic_params, &e.bounds)),
        )
        .chain(
            module
                .native_types
                .iter()
                .map(|n| (ResolvedName::NativeType(n.id), &n.generic_params, &n.bounds)),
        )
        .chain(
            module
                .traits
                .iter()
                .map(|t| (ResolvedName::Trait(t.id), &t.generic_params, &t.bounds)),
        )
    {
        if cancel.check().is_err() {
            return;
        }
        let Some(available) = declarations
            .definition(owner)
            .and_then(|id| signatures.type_bounds(id))
        else {
            continue;
        };
        checker.owner(params, predicates, available);
        if let ResolvedName::Trait(id) = owner {
            checker.members(&module.traits[id.index()].associated_types, available);
        }
    }
    for item in &module.impls {
        if cancel.check().is_err() {
            return;
        }
        let available = constraints::implementation_bounds(module, item, declarations, table);
        checker.owner(&item.generic_params, &item.bounds, &available);
        checker.members(&item.associated_types, &available);
    }
    for alias in &module.type_aliases {
        if cancel.check().is_err() {
            return;
        }
        if let Some(template) = declarations
            .definition(ResolvedName::TypeAlias(alias.id))
            .and_then(|id| declarations.imported_types().alias(id))
        {
            checker.owner(&alias.generic_params, &alias.bounds, &template.bounds);
        }
    }
    for signature in signatures.functions() {
        if cancel.check().is_err() {
            return;
        }
        let function = &module.functions[signature.id.index()];
        let owner = declarations.definition(ResolvedName::Function(function.id));
        // Inherited method binders are assumptions here, but their applications
        // have already been checked at their declaration, without method-local facts.
        let local = function
            .generic_params
            .iter()
            .zip(&signature.generic_params)
            .filter_map(|(id, param)| (Some(&param.owner) == owner).then_some(id));
        checker.owner(local, &function.bounds, &signature.bounds);
    }
}

struct Checker<'a> {
    module: &'a Module,
    table: &'a TypeTable,
    declarations: &'a Declarations,
    catalog: &'a AggregateCatalog,
    diagnostics: &'a mut DiagnosticBuffer,
    cancel: &'a CancellationToken,
}

impl Checker<'_> {
    fn members(&mut self, members: &[AssociatedType], inherited: &GenericBounds) {
        for member in members {
            if self.cancel.check().is_err() {
                return;
            }
            if member.generic_params.is_empty() && member.parameter_bounds.is_empty() {
                for reference in &member.bounds {
                    self.predicate(None, reference, inherited);
                }
                continue;
            }
            let mut available = constraints::owner_bounds(
                self.module,
                &member.generic_params,
                &member.parameter_bounds,
                self.declarations,
                self.table,
            );
            for (ty, requirements) in inherited {
                available
                    .entry(ty.clone())
                    .or_default()
                    .extend(requirements.iter().cloned());
            }
            self.owner(&member.generic_params, &member.parameter_bounds, &available);
            for reference in &member.bounds {
                self.predicate(None, reference, &available);
            }
        }
    }

    fn owner<'a>(
        &mut self,
        params: impl IntoIterator<Item = &'a GenericParamId>,
        predicates: &[TraitBound],
        available: &GenericBounds,
    ) {
        for &param in params {
            if self.cancel.check().is_err() {
                return;
            }
            let references = &self.module.generic_parameter(param).bounds;
            if !references.is_empty()
                && let Some(subject) = self.declarations.generic_type(param).map(TypeId::Generic)
            {
                for reference in references {
                    self.predicate(Some(&subject), reference, available);
                }
            }
        }
        for predicate in predicates {
            if self.cancel.check().is_err() {
                return;
            }
            let subject = self.table.type_ref(predicate.target_ref).map(|r| &r.ty);
            for reference in &predicate.traits {
                self.predicate(subject, reference, available);
            }
        }
    }

    fn predicate(
        &mut self,
        subject: Option<&TypeId>,
        reference: &TraitRef,
        available: &GenericBounds,
    ) {
        if self.cancel.check().is_err() {
            return;
        }
        let Some(required) = self.table.constraint(reference.ty) else {
            return;
        };
        let Some(contract) = self.catalog.trait_(&required.declaration) else {
            return;
        };
        let span = self.module.type_ref(reference.ty).origin.diagnostic_span();
        let context = || ApplicationContext {
            definitions: self.declarations.definitions(),
            catalog: self.catalog,
            receiver: subject,
        };
        // U: Reader<T> assumes membership, not that Reader<T>'s arguments
        // satisfy its declaration. Use the normal application checker, not the
        // rules for constructing an interface *value* (which require object safety).
        if !required.arguments.is_empty()
            || !required.associated_types.is_empty()
            || contract.bounds.values().any(|bounds| !bounds.is_empty())
        {
            validate(
                context(),
                &TypeId::Trait(required.clone()),
                available,
                span,
                self.diagnostics,
                self.cancel,
            );
        }
        // Closed predicates must hold independently; never prove the assertion
        // itself using the owner's assumptions (which include that assertion).
        let Some(subject) = subject.filter(|ty| ty.is_closed()) else {
            return;
        };
        if !required
            .arguments
            .iter()
            .chain(required.associated_types.values())
            .all(TypeId::is_closed)
        {
            return;
        }
        let search = ImplementationSearchContext {
            assumptions: &GenericBounds::default(),
            limits: REQUIREMENT_LIMITS,
            cancel: self.cancel,
        };
        if let Err(ImplementationSearchError::LimitExceeded) = check_requirement(
            &context(),
            subject,
            &required,
            search,
            span,
            self.diagnostics,
        ) {
            self.diagnostics.push(
                Diagnostic::error(DiagnosticKind::CompileLimitExceeded {
                    resource: "trait implementation search",
                    limit: REQUIREMENT_LIMITS.max_checks,
                })
                .with_span(span),
            );
        }
    }
}
