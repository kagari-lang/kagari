//! Publication-only admission and metadata-only installation demand history.
pub(crate) mod coverage;
mod proofs;

use crate::{
    module::{ExecutableModule, ModuleKey, VerifiedProgram},
    reload::{ReloadValidationError, contracts, updates::coverage::CoverageFacts},
};
use kagari_bytecode::{module::BytecodeModule, program::verified::VerifiedBytecodeProgram};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionKind,
        mapping::{DefinitionMapper, DefinitionMappingError, check_cancel, map_sequence},
        metadata::DefinitionMetadata,
        scoped::{DefinitionReferences, RemapDefinitions},
        table::{DefinitionId, DefinitionTable},
    },
};
use kagari_contract::{
    types::{
        ConcreteFunctionIdentity, PublicItem,
        named::{NamedContract, NamedDeclaration, correspondence::DeclarationCorrespondence},
    },
    updates::{
        DemandEnvelope, UpdateEvidence,
        correspondence::import_verified_evidence,
        demands::{Coverage, check_coverage, publication_demands, union_demands},
        initializers::compare_initializers,
        limits::{Budget, MAX_RECORDS, UpdateError},
        wire::UpdateEvidenceRecord,
    },
};
use std::{collections::HashSet, sync::Arc};

/// Admission is tied to one immutable baseline and its already checked candidate.
/// It travels with `PreparedReload`, never through a serialized candidate record.
#[derive(Debug)]
pub(crate) struct ReloadAdmission {
    pub(crate) baseline: ModuleKey,
    pub(crate) demands: Arc<RetainedDemands>,
}

/// Immutable installation history without code, frames, values or executable leases.
/// Each published descriptor owns its cumulative snapshot. A staged candidate owns
/// a separate prospective snapshot; abandoning it cannot change the current one.
#[derive(Debug, Clone)]
pub struct RetainedDemands {
    metadata: DefinitionMetadata<DemandEnvelope>,
    headers: Vec<NamedDeclaration>,
}

impl RetainedDemands {
    /// All historically published body requirements in an explicit comparison scope.
    /// Source compilation must import these with `implementation_headers()` through
    /// checked declaration correspondence; comparison IDs are not source placements.
    pub fn metadata(&self) -> &DefinitionMetadata<DemandEnvelope> {
        &self.metadata
    }

    /// Semantic impl owners needed to import the retained scoped requirements.
    /// Nominal and ordinary callable paths retain their existing portable identity.
    pub fn implementation_headers(&self) -> &[NamedDeclaration] {
        &self.headers
    }

    pub(crate) fn initial(program: &VerifiedProgram) -> Result<Arc<Self>, ReloadValidationError> {
        let cancel = CancellationToken::default();
        let mut correspondence = DeclarationCorrespondence::new().map_err(identity_error)?;
        let headers = headers(program);
        let evidence = correspondence
            .import(
                program.update_evidence(),
                headers.iter().cloned(),
                program.definitions(),
                &cancel,
            )
            .map_err(identity_error)?;
        let imported_headers = correspondence
            .import(
                &headers,
                headers.iter().cloned(),
                program.definitions(),
                &cancel,
            )
            .map_err(identity_error)?;
        let definitions = imported_headers.definitions();
        let result = (|| {
            let mut budget = Budget::new(Default::default(), &cancel)?;
            charge(evidence.records(), definitions, &cancel, &mut budget)?;
            let demands = publication_demands(evidence.records(), &mut budget)?;
            let demands = union_demands(
                &DemandEnvelope::default(),
                &demands,
                definitions,
                &mut budget,
            )?;
            finish(demands, imported_headers.records(), definitions, &cancel)
        })();
        result.map_err(|error| admission_error(error, definitions))
    }
}

/// Prepare the next snapshot using the installation's history, never candidate history.
/// The caller must first validate old declaration/public contracts and candidate code.
pub(crate) fn prepare(
    previous: &ExecutableModule,
    candidate: &VerifiedProgram,
) -> Result<Arc<RetainedDemands>, ReloadValidationError> {
    prepare_records(
        previous,
        Candidate {
            evidence: candidate.update_evidence(),
            coverage: candidate.update_coverage(),
            definitions: candidate.definitions(),
            headers: headers(candidate),
            proof_names: proof_names(
                candidate.modules().iter().map(Arc::as_ref),
                candidate.definitions(),
            )
            .map_err(identity_error)?,
        },
    )
}

pub(super) fn validate_bytecode(
    previous: &ExecutableModule,
    candidate: &VerifiedBytecodeProgram,
) -> Result<(), ReloadValidationError> {
    let coverage = CoverageFacts::from_checked(
        candidate.update_coverage(),
        &candidate.program().modules,
        candidate.definitions(),
    );
    prepare_records(
        previous,
        Candidate {
            evidence: &candidate.program().update_evidence,
            coverage: &coverage,
            definitions: candidate.definitions(),
            headers: contracts::inventory(&candidate.program().modules)
                .filter(|record| matches!(record.contract, NamedContract::Implementation(_)))
                .collect(),
            proof_names: proof_names(&candidate.program().modules, candidate.definitions())
                .map_err(identity_error)?,
        },
    )
    .map(|_| ())
}

struct Candidate<'a> {
    evidence: &'a UpdateEvidence,
    coverage: &'a CoverageFacts,
    definitions: &'a DefinitionTable,
    headers: Vec<NamedDeclaration>,
    proof_names: ProofNames,
}

struct ProofNames(Vec<DefinitionId>);

impl DefinitionReferences for ProofNames {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        for id in &self.0 {
            check_cancel(cancel)?;
            visit(id)?;
        }
        Ok(())
    }
}

impl RemapDefinitions for ProofNames {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        Ok(Self(map_sequence(&self.0, |id| mapper.reference(id))?))
    }
}

/// Preserve implicit method references from checked proof contracts. Generic
/// signatures may reference their method owner explicitly; nongeneric signatures
/// need the same identity even when no executable currently calls that method.
fn proof_names<'a>(
    modules: impl IntoIterator<Item = &'a BytecodeModule>,
    definitions: &DefinitionTable,
) -> Result<ProofNames, DefinitionMappingError> {
    let mut names = HashSet::new();
    let mut method = |owner, name: &str| {
        let id = definitions
            .lookup_child(owner, DefinitionKind::Method, name, 0)
            .ok_or(DefinitionMappingError::InvalidContract)?;
        names.insert(id);
        Ok::<_, DefinitionMappingError>(())
    };
    for module in modules {
        for item in &module.public_items {
            let (owner, methods) = match item {
                PublicItem::InterfaceTable(table) => (table.declaration, &table.methods),
                PublicItem::Trait(contract) => {
                    let root = definitions
                        .lookup_root(&module.identity)
                        .ok_or(DefinitionMappingError::InvalidContract)?;
                    let owner = definitions
                        .lookup_child(root, DefinitionKind::Trait, &contract.name, 0)
                        .ok_or(DefinitionMappingError::InvalidContract)?;
                    (owner, &contract.methods)
                }
                _ => continue,
            };
            for signature in methods {
                method(owner, &signature.name)?;
            }
        }
        for contract in &module.trait_contracts {
            for signature in &contract.abi.methods {
                method(contract.declaration, &signature.name)?;
            }
        }
    }
    Ok(ProofNames(names.into_iter().collect()))
}

fn prepare_records(
    previous: &ExecutableModule,
    candidate: Candidate<'_>,
) -> Result<Arc<RetainedDemands>, ReloadValidationError> {
    let cancel = CancellationToken::default();
    let mut correspondence = DeclarationCorrespondence::new().map_err(identity_error)?;
    let old_program = previous.verified_program();
    let old_headers = headers(old_program);
    let new_headers = candidate.headers;
    let mut budget = Budget::new(Default::default(), &cancel)
        .map_err(|error| ReloadValidationError::UpdateEvidence(error.to_string()))?;
    let previous_evidence = import_verified_evidence(
        &mut correspondence,
        old_program.update_evidence(),
        &old_headers,
        old_program.definitions(),
        &mut budget,
    )
    .map_err(|error| ReloadValidationError::UpdateEvidence(error.to_string()))?;
    let retained = previous.retained_demands();
    let history = correspondence
        .import(
            retained.metadata.records(),
            retained.headers.iter().cloned(),
            retained.metadata.definitions(),
            &cancel,
        )
        .map_err(identity_error)?;
    let evidence = import_verified_evidence(
        &mut correspondence,
        candidate.evidence,
        &new_headers,
        candidate.definitions,
        &mut budget,
    )
    .map_err(|error| ReloadValidationError::UpdateEvidence(error.to_string()))?;
    let coverage = correspondence
        .import(
            candidate.coverage,
            new_headers.iter().cloned(),
            candidate.definitions,
            &cancel,
        )
        .map_err(identity_error)?;
    correspondence
        .import(
            &candidate.proof_names,
            new_headers.iter().cloned(),
            candidate.definitions,
            &cancel,
        )
        .map_err(identity_error)?;
    let imported_headers = correspondence
        .import(
            &new_headers,
            new_headers.iter().cloned(),
            candidate.definitions,
            &cancel,
        )
        .map_err(identity_error)?;
    let definitions = imported_headers.definitions();
    let result = (|| {
        compare_initializers(previous_evidence.records(), evidence.records(), &mut budget)?;
        let published = publication_demands(evidence.records(), &mut budget)?;
        let demands = union_demands(history.records(), &published, definitions, &mut budget)?;
        let facts = coverage.records();
        let natives = facts.natives();
        let catalog = facts
            .proofs
            .catalog(definitions, &cancel)
            .map_err(UpdateError::from)?;
        check_coverage(
            &demands,
            &Coverage {
                evidence: evidence.records(),
                methods: &facts.methods,
                constructions: &facts.constructions,
                layouts: &facts.layouts,
                natives: &natives,
            },
            &catalog,
            &mut budget,
        )?;
        finish(demands, imported_headers.records(), definitions, &cancel)
    })();
    result.map_err(|error| admission_error(error, definitions))
}

pub(crate) fn headers(program: &VerifiedProgram) -> Vec<NamedDeclaration> {
    contracts::inventory(program.modules().iter().map(Arc::as_ref))
        .filter(|record| matches!(record.contract, NamedContract::Implementation(_)))
        .collect()
}

fn finish(
    demands: DemandEnvelope,
    headers: &[NamedDeclaration],
    definitions: &DefinitionTable,
    cancel: &CancellationToken,
) -> Result<Arc<RetainedDemands>, UpdateError> {
    let mut seen = HashSet::new();
    let headers = headers
        .iter()
        .filter(|record| seen.insert(record.declaration))
        .cloned()
        .collect();
    Ok(Arc::new(RetainedDemands {
        metadata: DefinitionMetadata::checked(definitions.clone(), demands, cancel)?,
        headers,
    }))
}

fn charge(
    evidence: &UpdateEvidence,
    definitions: &DefinitionTable,
    cancel: &CancellationToken,
    budget: &mut Budget<'_>,
) -> Result<(), UpdateError> {
    let mut references = HashSet::new();
    let mut remaining = MAX_RECORDS;
    evidence.visit_definition_ids(
        &mut |id| {
            remaining = remaining
                .checked_sub(1)
                .ok_or(DefinitionMappingError::LimitExceeded)?;
            references.insert(*id);
            Ok(())
        },
        cancel,
    )?;
    let encoding = definitions
        .encode(references)
        .map_err(DefinitionMappingError::from)?;
    UpdateEvidenceRecord::encode(evidence, &encoding, budget)?;
    Ok(())
}

fn identity_error(error: DefinitionMappingError) -> ReloadValidationError {
    ReloadValidationError::UpdateEvidence(error.to_string())
}

fn admission_error(error: UpdateError, definitions: &DefinitionTable) -> ReloadValidationError {
    let message = match error {
        UpdateError::InitializerChange { declaration } => format!(
            "static initializer {} changed",
            display(
                &ConcreteFunctionIdentity {
                    declaration,
                    arguments: vec![],
                },
                definitions,
            ),
        ),
        UpdateError::MissingTarget(target) => format!(
            "candidate is missing current target {}",
            display(&target, definitions)
        ),
        UpdateError::MissingBody(body) => format!(
            "candidate is missing required body {}",
            display(&body, definitions)
        ),
        UpdateError::MissingNative(application) => format!(
            "candidate is missing required native application {}",
            display(&application, definitions)
        ),
        UpdateError::UnresolvedSymbolicRoute(application) => format!(
            "candidate has no checked shared route for {}",
            display(&application, definitions)
        ),
        error => error.to_string(),
    };
    ReloadValidationError::UpdateEvidence(message)
}

fn display(identity: &ConcreteFunctionIdentity, definitions: &DefinitionTable) -> String {
    let name = definitions
        .resolve(identity.declaration)
        .map(|view| {
            let path = view.to_path();
            format!(
                "{}::{}",
                path.module,
                path.path
                    .iter()
                    .map(|part| part.name.as_str())
                    .collect::<Vec<_>>()
                    .join("::")
            )
        })
        .unwrap_or_else(|_| "invalid declaration".into());
    if identity.arguments.is_empty() {
        name
    } else {
        format!("{name}<{:?}>", identity.arguments)
    }
}
