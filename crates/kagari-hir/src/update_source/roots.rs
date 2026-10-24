//! Declaration-driven protocol roots, including generic templates and defaults.

use crate::{
    CheckedAnalysis,
    aggregates::implementations::{
        ImplementationSearchContext, ImplementationSearchError, ImplementationSearchLimits,
    },
    types::{
        TypeId,
        semantic::{lower_nominal_type, lower_type},
    },
    update_source::{
        FactKind, SourceBudget, SourceEvidence, SourceFact, SourceRoot, SourceRootKind, TokenRange,
        UnknownDependency,
    },
};
use kagari_common::identity::{mapping::DefinitionMappingError, table::DefinitionId};
use kagari_types::{language::Protocol, ty::Ty};

fn search_error(error: ImplementationSearchError) -> DefinitionMappingError {
    match error {
        ImplementationSearchError::Cancelled => DefinitionMappingError::Cancelled,
        _ => DefinitionMappingError::LimitExceeded,
    }
}

fn selection(
    module: &CheckedAnalysis,
    owner: DefinitionId,
    member: DefinitionId,
    types: Vec<Ty>,
    result: &mut SourceEvidence,
    budget: &mut SourceBudget<'_>,
) -> Result<(), DefinitionMappingError> {
    let mut targets = vec![owner, member];
    if let Some((_, default)) = module
        .aggregates
        .default_method(module.definitions(), &member)
    {
        targets.push(default.id);
    }
    budget.charge(1 + targets.len() + types.len(), 0)?;
    result.selections.push(SourceFact {
        site: TokenRange::default(),
        kind: FactKind::Selection,
        targets,
        types,
        related: vec![],
    });
    Ok(())
}

pub(super) fn collect(
    module: &CheckedAnalysis,
    result: &mut SourceEvidence,
    budget: &mut SourceBudget<'_>,
) -> Result<(), DefinitionMappingError> {
    let catalog = &module.aggregates;
    let definitions = module.definitions();
    for implementation in catalog.implementations() {
        budget.charge(1, 0)?;
        if definitions.resolve(implementation.id)?.module()
            != module.lowered.source.module_identity()
        {
            continue;
        }
        let members = catalog.implementation_methods(definitions, implementation);
        let receiver = lower_type(&implementation.for_type);
        for member in &members {
            selection(
                module,
                implementation.id,
                *member,
                vec![
                    receiver.clone(),
                    Ty::Trait(lower_nominal_type(&implementation.trait_type)),
                ],
                result,
                budget,
            )?;
        }
        let closure = catalog
            .trait_closure(
                &implementation.trait_type,
                &implementation.for_type,
                budget.cancel,
            )
            .map_err(search_error)?;
        for interface in &closure {
            let kind = match Protocol::from_definition(&interface.declaration, definitions) {
                Some(Protocol::Hash) => SourceRootKind::Hash,
                Some(Protocol::Eq) => SourceRootKind::Equality,
                _ => continue,
            };
            budget.charge(1, 0)?;
            let mut protected = Vec::new();
            // Reuse source selection under this implementation's already-checked
            // assumptions. No backend parsing, inference or custom trait solver.
            for inherited in catalog
                .trait_closure(interface, &implementation.for_type, budget.cancel)
                .map_err(search_error)?
            {
                let contract = catalog
                    .trait_(&inherited.declaration)
                    .ok_or(DefinitionMappingError::InvalidContract)?;
                for method in &contract.methods {
                    budget.charge(1, 0)?;
                    let direct = (inherited.declaration == implementation.trait_type.declaration)
                        .then(|| implementation.methods.get(&method.id).copied())
                        .flatten();
                    let selected = if let Some(member) = direct {
                        Some((
                            member,
                            implementation
                                .generic_params
                                .iter()
                                .cloned()
                                .map(TypeId::Generic)
                                .collect(),
                        ))
                    } else {
                        catalog
                            .implementation_method(
                                definitions,
                                &method.id,
                                &inherited,
                                &implementation.for_type,
                                ImplementationSearchContext {
                                    assumptions: &implementation.bounds,
                                    limits: ImplementationSearchLimits {
                                        max_checks: budget.records.min(100_000),
                                        max_depth: 64,
                                    },
                                    cancel: budget.cancel,
                                },
                            )
                            .map_err(search_error)?
                    };
                    if let Some((member, arguments)) = selected {
                        if !protected.contains(&member) {
                            protected.push(member);
                        }
                        selection(
                            module,
                            implementation.id,
                            member,
                            [receiver.clone(), Ty::Trait(lower_nominal_type(&inherited))]
                                .into_iter()
                                .chain(arguments.iter().map(lower_type))
                                .collect(),
                            result,
                            budget,
                        )?;
                    } else {
                        budget.charge(1, 0)?;
                        result.selections.push(SourceFact {
                            site: TokenRange::default(),
                            kind: FactKind::Unknown(UnknownDependency::InterfaceDispatch),
                            targets: vec![implementation.id, method.id],
                            types: vec![
                                receiver.clone(),
                                Ty::Trait(lower_nominal_type(&inherited)),
                            ],
                            related: vec![],
                        });
                    }
                }
            }
            result.roots.push(SourceRoot {
                declaration: implementation.id,
                kind,
                receiver: Some(receiver.clone()),
                interface: Some(lower_nominal_type(interface)),
                members: protected,
            });
        }
    }
    Ok(())
}
