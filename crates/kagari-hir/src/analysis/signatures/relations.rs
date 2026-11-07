//! Reverse implementation navigation over checked signatures, without body analysis.

use crate::{
    aggregates::implementations::{
        ImplementationSearchContext, ImplementationSearchError, ImplementationSearchLimits,
    },
    analysis::signatures::{FileSignatures, SignatureSnapshot},
    declarations::DeclarationId,
    hir::{ids::ImplId, item::behavior::Impl},
    resolver::resolved::ResolvedName,
    typeck::{GenericBounds, constraints, inference},
    types::{GenericParameterType, TypeId, TypeSubstitution},
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionKind,
        mapping::DefinitionMappingError,
        scoped::DefinitionReferences,
        table::{DefinitionId, DefinitionTable},
    },
};
use kagari_source::{
    diagnostic::Severity,
    identity::{FileId, FileSpan},
};
use kagari_syntax::ast::visit::NodeRef;
use std::{collections::HashSet, mem::size_of, sync::Arc};

#[cfg(test)]
mod tests;

/// Explicit storage, output and solver bounds for one retained relation view.
#[derive(Debug, Clone, Copy)]
pub struct RelationLimits {
    /// Maximum implementation declarations retained in this view.
    pub implementations: usize,
    /// Maximum distinct navigation destinations returned by one query.
    pub results: usize,
    /// Maximum allocated bytes in the compact reverse rows; source facts are shared.
    pub bytes: usize,
    /// Bound checks and recursive depth for each implementation candidate.
    pub search: ImplementationSearchLimits,
}

impl Default for RelationLimits {
    fn default() -> Self {
        Self {
            implementations: 65_536,
            results: 16_384,
            bytes: 16 * 1024 * 1024,
            search: ImplementationSearchLimits {
                max_checks: 4096,
                max_depth: 64,
            },
        }
    }
}

/// Failure is distinct from a complete query that has no implementations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum RelationError {
    /// The caller cancelled construction or matching.
    #[error("implementation search cancelled")]
    Cancelled,
    /// A storage, output, candidate or recursive search limit was reached.
    #[error("implementation search limit exceeded")]
    LimitExceeded,
    /// Declaration/signature errors leave unknown semantic relations.
    #[error("implementation coverage is incomplete because signatures are unresolved")]
    Incomplete,
    /// A target does not belong to this retained semantic scope.
    #[error("implementation target belongs to another definition scope")]
    ForeignTarget,
}

impl From<ImplementationSearchError> for RelationError {
    fn from(error: ImplementationSearchError) -> Self {
        match error {
            ImplementationSearchError::Cancelled => Self::Cancelled,
            ImplementationSearchError::LimitExceeded => Self::LimitExceeded,
        }
    }
}

/// Semantic target selected by the position query in this exact definition scope.
#[derive(Debug, Clone)]
pub struct ImplementationTarget {
    /// Named declaration; locals use their known receiver type instead.
    pub declaration: Option<DefinitionId>,
    /// Applied receiver when known; absent for an unapplied trait/type declaration.
    pub receiver: Option<TypeId>,
    /// Generic assumptions available at the selected source position.
    pub assumptions: GenericBounds,
}

/// An authoritative physical selection and enclosing implementation/member range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImplementationLocation {
    /// Name or receiver spelling to select at the destination.
    pub selection: FileSpan,
    /// Whole implementation block or member declaration.
    pub extent: FileSpan,
}

#[derive(Debug, Clone, Copy)]
struct ImplementationRow {
    id: DefinitionId,
    file: FileId,
    local: ImplId,
    location: ImplementationLocation,
}

/// Immutable checked relation view retaining the exact signature/source owners.
#[derive(Debug, Clone)]
pub struct ImplementationRelations {
    signatures: SignatureSnapshot,
    rows: Arc<[ImplementationRow]>,
    limits: RelationLimits,
}

impl SignatureSnapshot {
    /// Builds source implementation rows without checking a function body.
    /// Recoverable body syntax errors do not invalidate checked headers. Unresolved
    /// declaration or signature facts fail explicitly rather than proving no matches.
    pub fn implementation_relations(
        &self,
        limits: RelationLimits,
        cancel: &CancellationToken,
    ) -> Result<ImplementationRelations, RelationError> {
        check(cancel)?;
        let mut rows = Vec::new();
        for (file_id, file) in self.files.iter() {
            check(cancel)?;
            let mut body_ranges = Vec::new();
            for node in file.declaration.syntax().descendants() {
                check(cancel)?;
                let body = match node {
                    NodeRef::FnDef(function) => function.body(),
                    NodeRef::MethodDef(method) => method.body(),
                    _ => None,
                };
                if let Some(body) = body.filter(|body| body.data().close.as_ref().is_some()) {
                    body_ranges.push(body.body_span());
                }
            }
            if file.declaration.parsed.diagnostics().any(|diagnostic| {
                diagnostic.severity == Severity::Error
                    && !diagnostic.span.is_some_and(|span| {
                        body_ranges
                            .iter()
                            .any(|body| body.start <= span.start && span.end <= body.end)
                    })
            }) {
                return Err(RelationError::Incomplete);
            }
            if file
                .prepared
                .names
                .diagnostics()
                .iter()
                .chain(file.prepared.signatures.diagnostics())
                .any(|diagnostic| diagnostic.severity == Severity::Error)
            {
                return Err(RelationError::Incomplete);
            }
            let lowered = &file.prepared.lowered;
            for implementation in &lowered.module.impls {
                check(cancel)?;
                let id = *file
                    .declarations()
                    .impl_identity(implementation.id)
                    .ok_or(RelationError::Incomplete)?;
                let receiver = implementation.for_type.ok_or(RelationError::Incomplete)?;
                let ty = file
                    .signatures()
                    .facts()
                    .type_table()
                    .type_ref(receiver)
                    .ok_or(RelationError::Incomplete)?;
                if ty.ty.is_unresolved() {
                    return Err(RelationError::Incomplete);
                }
                if implementation.trait_ref.is_some()
                    && self.aggregates.implementation_signature(&id).is_none()
                {
                    return Err(RelationError::Incomplete);
                }
                let source = file.source();
                let selection = source
                    .span(lowered.source_map.type_span(receiver))
                    .ok_or(RelationError::Incomplete)?;
                let extent = source
                    .span(lowered.source_map.impl_span(implementation.id))
                    .ok_or(RelationError::Incomplete)?;
                if rows.len() >= limits.implementations
                    || (rows.len() + 1).saturating_mul(size_of::<ImplementationRow>())
                        > limits.bytes
                {
                    return Err(RelationError::LimitExceeded);
                }
                rows.push(ImplementationRow {
                    id,
                    file: *file_id,
                    local: implementation.id,
                    location: ImplementationLocation { selection, extent },
                });
            }
        }
        check(cancel)?;
        Ok(ImplementationRelations {
            signatures: self.clone(),
            rows: rows.into(),
            limits,
        })
    }
}

impl ImplementationRelations {
    /// Borrows the checked signatures that own all identities and source anchors.
    pub fn signatures(&self) -> &SignatureSnapshot {
        &self.signatures
    }

    /// Bytes retained by compact rows, excluding shared signature/source owners.
    pub fn retained_bytes(&self) -> usize {
        self.rows.len() * size_of::<ImplementationRow>()
    }

    /// Number of source implementation templates in the complete view.
    pub fn implementation_count(&self) -> usize {
        self.rows.len()
    }

    /// Enumerates checked implementors or the selected concrete member body.
    /// Generic templates occur once; concrete receivers use the normal bound solver.
    pub fn query(
        &self,
        target: &ImplementationTarget,
        cancel: &CancellationToken,
    ) -> Result<Vec<ImplementationLocation>, RelationError> {
        check(cancel)?;
        let definitions = &self.signatures.definitions;
        if let Some(id) = target.declaration {
            definitions
                .resolve(id)
                .map_err(|_| RelationError::ForeignTarget)?;
        }
        if let Some(receiver) = &target.receiver {
            validate(receiver, definitions, cancel)?;
        }
        for (subject, bounds) in &target.assumptions {
            validate(subject, definitions, cancel)?;
            validate(bounds, definitions, cancel)?;
        }
        let aggregates = &self.signatures.aggregates;
        let selected = target.declaration;
        let trait_id = selected
            .filter(|id| aggregates.trait_(id).is_some())
            .or_else(|| {
                let parent = definitions.parent(selected?).ok()??;
                aggregates.trait_(&parent).map(|_| parent)
            })
            .or(match target.receiver.as_ref() {
                Some(TypeId::Trait(interface)) => Some(interface.declaration),
                _ => None,
            });
        let member = selected.filter(|id| Some(*id) != trait_id && trait_id.is_some());
        if let Some(id) = selected.filter(|_| trait_id.is_none())
            && let Some(parent) = definitions.parent(id).ok().flatten()
            && definitions
                .resolve(parent)
                .ok()
                .and_then(|view| view.segments().last().map(|part| part.kind))
                == Some(DefinitionKind::Impl)
        {
            return self
                .member_location(id)
                .map(|location| vec![location])
                .ok_or(RelationError::Incomplete);
        }
        let receiver = target
            .receiver
            .as_ref()
            .filter(|ty| !matches!(ty, TypeId::Trait(_)));
        let nominal = selected.filter(|_| trait_id.is_none());
        let known_interface = match target.receiver.as_ref() {
            Some(TypeId::Trait(interface)) => Some(interface),
            _ => None,
        };
        let applied_interface = if let Some(interface) = known_interface {
            let projected = aggregates
                .interface_closure(
                    definitions,
                    interface,
                    &TypeId::Trait(interface.clone()),
                    cancel,
                )?
                .into_iter()
                .find(|interface| Some(interface.declaration) == trait_id);
            let Some(projected) = projected else {
                return Ok(Vec::new());
            };
            Some(projected)
        } else {
            None
        };
        let mut result = Vec::new();
        let mut seen = HashSet::new();
        for row in self.rows.iter() {
            check(cancel)?;
            let implementation = aggregates.implementation_signature(&row.id);
            if trait_id.is_some_and(|id| {
                implementation
                    .is_none_or(|implementation| implementation.trait_type.declaration != id)
            }) {
                continue;
            }
            if let Some(interface) = &applied_interface
                && !aggregates.implementation_supports_interface(
                    definitions,
                    implementation.ok_or(RelationError::Incomplete)?,
                    interface,
                    known_interface
                        .filter(|required| required.declaration != interface.declaration),
                    ImplementationSearchContext {
                        assumptions: &target.assumptions,
                        limits: self.limits.search,
                        cancel,
                    },
                )?
            {
                continue;
            }
            let file = &self.signatures.files[&row.file];
            let item = &file.prepared.lowered.module.impls[row.local.index()];
            let pattern = &file
                .signatures()
                .facts()
                .type_table()
                .type_ref(item.for_type.ok_or(RelationError::Incomplete)?)
                .ok_or(RelationError::Incomplete)?
                .ty;
            if let Some(receiver) = receiver {
                let applies = if let Some(implementation) = implementation {
                    aggregates.implementation_applies(
                        definitions,
                        implementation,
                        receiver,
                        ImplementationSearchContext {
                            assumptions: &target.assumptions,
                            limits: self.limits.search,
                            cancel,
                        },
                    )?
                } else {
                    self.inherent_applies(
                        file,
                        item,
                        pattern,
                        receiver,
                        &target.assumptions,
                        cancel,
                    )?
                };
                if !applies {
                    continue;
                }
            } else if let Some(nominal) = nominal {
                if nominal_definition(pattern) != Some(nominal) {
                    continue;
                }
            } else if trait_id.is_none() {
                continue;
            }
            let location = match member {
                Some(member) => self.implementation_member(row.id, member)?,
                None => Some(row.location),
            };
            if let Some(location) = location
                && seen.insert((
                    location.selection.file,
                    location.selection.revision,
                    location.selection.range.start,
                    location.selection.range.end,
                ))
            {
                if result.len() >= self.limits.results {
                    return Err(RelationError::LimitExceeded);
                }
                result.push(location);
            }
        }
        check(cancel)?;
        Ok(result)
    }

    fn member_location(&self, id: DefinitionId) -> Option<ImplementationLocation> {
        let declaration = self
            .signatures
            .declarations
            .declaration(&DeclarationId::Definition(id))?;
        let selection = declaration.location;
        let extent = self
            .signatures
            .files
            .values()
            .find_map(|file| {
                let function = file
                    .prepared
                    .lowered
                    .module
                    .functions
                    .iter()
                    .find(|function| {
                        file.declarations()
                            .definition(ResolvedName::Function(function.id))
                            == Some(&id)
                    })?;
                file.source()
                    .span(file.prepared.lowered.source_map.function_span(function.id))
            })
            .unwrap_or(selection);
        Some(ImplementationLocation { selection, extent })
    }

    fn implementation_member(
        &self,
        implementation: DefinitionId,
        member: DefinitionId,
    ) -> Result<Option<ImplementationLocation>, RelationError> {
        let aggregates = &self.signatures.aggregates;
        let implementation = aggregates
            .implementation_signature(&implementation)
            .ok_or(RelationError::Incomplete)?;
        if let Some(method) = aggregates.trait_method(&member) {
            let id = implementation
                .methods
                .get(&member)
                .copied()
                .or_else(|| method.default.as_ref().map(|_| member));
            return Ok(id.and_then(|id| self.member_location(id)));
        }
        if let Some(id) = aggregates.implementation_constant(&implementation.id, &member) {
            return Ok(self.member_location(*id));
        }
        let definitions = &self.signatures.definitions;
        let view = definitions
            .resolve(member)
            .map_err(|_| RelationError::ForeignTarget)?;
        let Some(segment) = view.last() else {
            return Ok(None);
        };
        let id = definitions.lookup_child(
            implementation.id,
            segment.kind,
            segment.name,
            segment.occurrence,
        );
        Ok(id.and_then(|id| self.member_location(id)))
    }

    fn inherent_applies(
        &self,
        file: &FileSignatures,
        item: &Impl,
        pattern: &TypeId,
        receiver: &TypeId,
        assumptions: &GenericBounds,
        cancel: &CancellationToken,
    ) -> Result<bool, RelationError> {
        let parameters = item
            .generic_params
            .iter()
            .filter_map(|parameter| file.declarations().generic_type(parameter.id))
            .collect::<Vec<GenericParameterType>>();
        let mut substitution = TypeSubstitution::default();
        inference::infer(
            pattern,
            receiver,
            &parameters,
            &mut substitution,
            cancel,
            None,
        )
        .map_err(|_| RelationError::Cancelled)?;
        let aggregates = &self.signatures.aggregates;
        let definitions = &self.signatures.definitions;
        if aggregates.normalize_type(definitions, &pattern.instantiate(&substitution))
            != aggregates.normalize_type(definitions, receiver)
        {
            return Ok(false);
        }
        let bounds = constraints::implementation_bounds(
            item,
            file.declarations(),
            file.signatures().facts().type_table(),
        );
        for (parameter, requirements) in bounds {
            let actual =
                aggregates.normalize_type(definitions, &parameter.instantiate(&substitution));
            for required in requirements {
                check(cancel)?;
                let required = required.instantiate(&substitution);
                if aggregates.implementation_count_bounded(
                    definitions,
                    &required,
                    &actual,
                    ImplementationSearchContext {
                        assumptions,
                        limits: self.limits.search,
                        cancel,
                    },
                )? != 1
                {
                    return Ok(false);
                }
            }
        }
        Ok(true)
    }
}

fn nominal_definition(ty: &TypeId) -> Option<DefinitionId> {
    match ty {
        TypeId::Struct(nominal) | TypeId::Enum(nominal) | TypeId::NativeObject(nominal) => {
            Some(nominal.declaration)
        }
        TypeId::Reference { target, .. } => nominal_definition(target),
        _ => None,
    }
}

fn check(cancel: &CancellationToken) -> Result<(), RelationError> {
    cancel.check().map_err(|_| RelationError::Cancelled)
}

fn validate<T: DefinitionReferences>(
    record: &T,
    definitions: &DefinitionTable,
    cancel: &CancellationToken,
) -> Result<(), RelationError> {
    record
        .visit_definition_ids(
            &mut |id| definitions.resolve(*id).map(|_| ()).map_err(Into::into),
            cancel,
        )
        .map_err(|error| match error {
            DefinitionMappingError::Cancelled => RelationError::Cancelled,
            _ => RelationError::ForeignTarget,
        })
}
