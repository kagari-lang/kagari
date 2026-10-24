//! Source-owned update evidence, collected before instance planning.
//!
//! Parent program wiring supplies the complete named inventory. This adapter does
//! not assign identities, select executable targets or prove artifact coverage.

use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{DefinitionMappingError, check_cancel},
        table::{DefinitionId, DefinitionTable},
    },
};
use kagari_contract::{
    types::named::{NamedContract, NamedDeclaration, correspondence::DeclarationCorrespondence},
    updates::{Dependency, SourceRecord, limits::MAX_NESTED},
};
use kagari_hir::{
    CheckedAnalysis,
    update_source::{self, SourceBodyKind, SourceBudget, SourceEvidence},
};
use std::collections::{BTreeMap, HashMap};

/// Collect checked templates and require their authored function/constant owners
/// in the declaration inventory. Statics retain their separate owning contracts.
/// Use one budget across every module in a source publication.
pub fn collect(
    module: &CheckedAnalysis,
    inventory: &[NamedDeclaration],
    budget: &mut SourceBudget<'_>,
) -> Result<SourceEvidence, DefinitionMappingError> {
    let evidence = update_source::collect(module, budget)?;
    for body in &evidence.bodies {
        if body.kind == SourceBodyKind::StaticInitializer {
            continue;
        }
        if !inventory.iter().any(|record| {
            record.declaration == body.declaration
                && matches!(
                    (&record.contract, body.kind),
                    (NamedContract::Constant(_), SourceBodyKind::Constant)
                        | (
                            NamedContract::Function(_),
                            SourceBodyKind::Function
                                | SourceBodyKind::AsyncFunction
                                | SourceBodyKind::Native
                                | SourceBodyKind::Required
                        )
                )
        }) {
            return Err(DefinitionMappingError::InvalidContract);
        }
    }
    Ok(evidence)
}

/// Complete each inherent header's dependencies with every actual member of its
/// semantic owner, including members authored in equivalent split impl blocks.
///
/// Publication retains one record per original declaration for independent artifact
/// coverage. After correspondence, equivalent header records are identical and can
/// be coalesced by the protected-evidence importer. Member bodies are never merged.
/// Header disagreement or a repeated semantic member is an error, not last-wins.
/// This does not change any executable/declaration identity in the publication.
pub(super) fn complete_inherent_owners(
    sources: &mut [SourceRecord],
    inventory: &[NamedDeclaration],
    definitions: &DefinitionTable,
    cancel: &CancellationToken,
) -> Result<(), DefinitionMappingError> {
    let mut correspondence = DeclarationCorrespondence::new()?;
    let mapped = correspondence.import(
        &sources.to_vec(),
        inventory.iter().cloned(),
        definitions,
        cancel,
    )?;
    let indices: HashMap<_, _> = sources
        .iter()
        .enumerate()
        .map(|(index, record)| (record.declaration, index))
        .collect();
    if indices.len() != sources.len() {
        return Err(DefinitionMappingError::InvalidContract);
    }
    let mut owners = HashMap::new();
    let mut groups: HashMap<DefinitionId, usize> = HashMap::new();
    for declaration in inventory {
        check_cancel(cancel)?;
        if !matches!(&declaration.contract, NamedContract::Implementation(header)
            if header.trait_type.is_none() && header.bridge.is_none())
        {
            continue;
        }
        let index = *indices
            .get(&declaration.declaration)
            .ok_or(DefinitionMappingError::InvalidContract)?;
        let record = &mapped.records()[index];
        if let Some(previous) = groups.insert(record.declaration, index)
            && mapped.records()[previous] != *record
        {
            // Only equal header-only records can share the completed member set.
            // Never erase conflicting tokens, checked facts or dependency evidence.
            return Err(DefinitionMappingError::InvalidContract);
        }
        owners.insert(declaration.declaration, record.declaration);
    }
    let mut members = HashMap::new();
    for (source, mapped_source) in sources.iter().zip(mapped.records()) {
        check_cancel(cancel)?;
        let Some(owner) = definitions
            .parent(source.declaration)?
            .and_then(|parent| owners.get(&parent))
        else {
            continue;
        };
        let group = members.entry(*owner).or_insert_with(BTreeMap::new);
        let path = mapped
            .definitions()
            .resolve(mapped_source.declaration)?
            .to_path();
        if group.insert(path, source.declaration).is_some() {
            return Err(DefinitionMappingError::InvalidContract);
        }
        if group.len() > MAX_NESTED {
            return Err(DefinitionMappingError::LimitExceeded);
        }
    }
    for source in sources {
        check_cancel(cancel)?;
        let Some(group) = owners
            .get(&source.declaration)
            .and_then(|owner| members.get(owner))
        else {
            continue;
        };
        for member in group.values() {
            check_cancel(cancel)?;
            let dependency = Dependency::Source(*member);
            if !source.dependencies.contains(&dependency) {
                if source.dependencies.len() >= MAX_NESTED {
                    return Err(DefinitionMappingError::LimitExceeded);
                }
                source.dependencies.push(dependency);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use kagari_common::{cancellation::CancellationToken, identity::metadata::DefinitionMetadata};
    use kagari_contract::types::named::correspondence::DeclarationCorrespondence;
    use kagari_hir::analyze_source;
    use kagari_source::source::SourceFile;
    use kagari_stdlib::catalog;
    use kagari_types::{
        callable::CallableImplementation,
        declaration::{FnDecl, Param},
        visibility::Visibility,
    };

    // Publication keeps candidate-scoped identities. Comparison imports them using
    // the same semantic impl correspondence as named contracts, not source order.
    fn canonicalize(
        evidence: &SourceEvidence,
        module: &CheckedAnalysis,
        inventory: &[NamedDeclaration],
        correspondence: &mut DeclarationCorrespondence,
        cancel: &CancellationToken,
    ) -> Result<DefinitionMetadata<SourceEvidence>, DefinitionMappingError> {
        let mapped = correspondence.import(
            evidence,
            inventory.iter().cloned(),
            module.definitions(),
            cancel,
        )?;
        let (definitions, mut records) = mapped.into_parts();
        records.canonical_order(&definitions, cancel)?;
        DefinitionMetadata::checked(definitions, records, cancel)
    }

    #[test]
    fn source_adapter_checks_inventory_and_uses_correspondence() {
        let module = analyze_source(
            &SourceFile::new("adapter.kgr", "static val N: i32 = ((1 + 2));"),
            catalog::shared().unwrap(),
        )
        .unwrap()
        .into_codegen()
        .unwrap();
        let cancel = CancellationToken::default();
        let records = collect(&module, &[], &mut SourceBudget::new(&cancel)).unwrap();
        let canonical = canonicalize(
            &records,
            &module,
            &[],
            &mut DeclarationCorrespondence::new().unwrap(),
            &cancel,
        )
        .unwrap();
        assert_eq!(canonical.records().roots.len(), 1);
        let module = analyze_source(
            &SourceFile::new("adapter.kgr", "fn unused<T>(x: T) -> T { x }"),
            catalog::shared().unwrap(),
        )
        .unwrap()
        .into_codegen()
        .unwrap();
        assert_eq!(
            collect(&module, &[], &mut SourceBudget::new(&cancel)),
            Err(DefinitionMappingError::InvalidContract)
        );
        let evidence = update_source::collect(&module, &mut SourceBudget::new(&cancel)).unwrap();
        let body = &evidence.bodies[0];
        let ty = body.parameters[0].as_type();
        let inventory = [NamedDeclaration {
            declaration: body.declaration,
            visibility: Visibility::Private,
            ambiguous_local_body: None,
            contract: NamedContract::Function(FnDecl {
                host_policy: None,
                method_policy: Default::default(),
                name: "unused".into(),
                implementation: CallableImplementation::Script,
                generic_params: body.parameters.clone(),
                bounds: vec![],
                params: vec![Param {
                    name: "x".into(),
                    ty: ty.clone(),
                    mutable: false,
                }],
                return_type: ty,
            }),
        }];
        let records = collect(&module, &inventory, &mut SourceBudget::new(&cancel)).unwrap();
        let canonical = canonicalize(
            &records,
            &module,
            &inventory,
            &mut DeclarationCorrespondence::new().unwrap(),
            &cancel,
        )
        .unwrap();
        assert_eq!(canonical.records().bodies.len(), 1);
        assert_eq!(canonical.records().bodies[0].parameters.len(), 1);
    }
}
