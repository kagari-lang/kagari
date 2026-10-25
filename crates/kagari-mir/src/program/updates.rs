//! Publication evidence is produced explicitly, then checked against immutable MIR.
mod canonical;
mod context;
use crate::program::updates::context::Context;
#[cfg(test)]
pub(crate) mod fixtures;
mod inventory;
#[cfg(test)]
mod tests;

#[cfg(test)]
pub(crate) fn empty() -> UpdateEvidence {
    UpdateEvidence {
        source_manifest: None,
        sources: vec![],
        bodies: vec![],
        initializers: vec![],
        targets: vec![],
        selections: vec![],
    }
}

use crate::function::MirModule;
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::DefinitionMappingError, scoped::DefinitionReferences, table::DefinitionTable,
    },
};
use kagari_contract::updates::wire::UpdateEvidenceRecord;
use kagari_contract::{
    types::{ConcreteFunctionIdentity, proofs::ProofCatalog},
    updates::{
        BodyClassification, BodyRecord, CallRequirement, CanonicalOperand, SourceRecord,
        StaticInitializer, UpdateEvidence,
        admission::{self, Inventory},
        limits::{Budget, Limits, MAX_NESTED, UpdateError},
    },
};
use std::collections::{HashMap, HashSet};

/// Checked source templates and explicit compiler instance classifications.
/// Call before optimization; source-free producers must supply the same contracts.
pub struct EvidenceInputs {
    pub sources: Vec<SourceRecord>,
    pub initializers: Vec<StaticInitializer>,
    pub selections: Vec<CallRequirement>,
    pub classifications: HashMap<ConcreteFunctionIdentity, BodyClassification>,
}

/// Capture executable bodies before simplification. This does not grant a verifier seal.
pub fn build(
    modules: &[MirModule],
    inputs: EvidenceInputs,
    definitions: &DefinitionTable,
    cancel: &CancellationToken,
) -> Result<UpdateEvidence, UpdateError> {
    let modules = modules.iter().collect::<Vec<_>>();
    let mut budget = Budget::new(Limits::default(), cancel)?;
    let mut evidence = UpdateEvidence {
        source_manifest: None,
        sources: inputs.sources,
        bodies: vec![],
        initializers: inputs.initializers,
        targets: vec![],
        selections: inputs.selections,
    };
    let extraction = Context::new(&modules, definitions, cancel, &mut budget)?;
    let mut classifications = inputs.classifications;
    for module in &modules {
        for function in &module.functions {
            budget.visit()?;
            let classification = classifications
                .remove(&function.instance)
                .ok_or(UpdateError::InvalidEvidence)?;
            evidence.bodies.push(canonical::extract(
                &extraction,
                module,
                function,
                classification,
                None,
                &mut budget,
            )?);
        }
    }
    if !classifications.is_empty() {
        return Err(UpdateError::InvalidEvidence);
    }
    // Associate named/static recipes without rescanning every unused template.
    let mut source_indices = HashMap::new();
    let mut source_bodies = Vec::new();
    for (index, source) in evidence.sources.iter().enumerate() {
        budget.visit()?;
        if source.bodies.len() > MAX_NESTED
            || source_indices.insert(source.declaration, index).is_some()
        {
            return Err(UpdateError::InvalidEvidence);
        }
        let mut seen = HashSet::new();
        for body in &source.bodies {
            budget.visit()?;
            seen.insert(body.clone());
        }
        source_bodies.push(seen);
    }
    for body in &evidence.bodies {
        budget.visit()?;
        let source = match &body.classification {
            BodyClassification::Named(id) => Some(extraction.source(*id)),
            BodyClassification::StaticInitializer(id) => Some(*id),
            _ => None,
        };
        if let Some(index) = source.and_then(|id| source_indices.get(&id)).copied() {
            let origin = body.requirement.origin.clone();
            if source_bodies[index].insert(origin.clone()) {
                if evidence.sources[index].bodies.len() >= MAX_NESTED {
                    return Err(UpdateError::LimitExceeded);
                }
                evidence.sources[index].bodies.push(origin);
            }
        }
    }
    let index = inventory::collect(&modules, &evidence.bodies, definitions, cancel, &mut budget)?;
    evidence.targets = index.targets.clone();
    admission::validate_inventory(&evidence, &index.borrowed(), definitions, &mut budget)?;
    Ok(evidence)
}

/// Rebuild emitted MIR while retaining the pre-optimization executable and edges.
pub fn refresh(
    modules: &[MirModule],
    previous: &UpdateEvidence,
    definitions: &DefinitionTable,
    cancel: &CancellationToken,
) -> Result<UpdateEvidence, UpdateError> {
    let modules = modules.iter().collect::<Vec<_>>();
    let mut budget = Budget::new(Limits::default(), cancel)?;
    preflight(previous, definitions, cancel, &mut budget)?;
    let actual = extract(&modules, previous, definitions, cancel, &mut budget)?;
    let mut evidence = previous.clone();
    for (body, actual) in evidence.bodies.iter_mut().zip(actual) {
        body.actual_format = actual.actual_format;
        body.actual_instructions = actual.actual_instructions;
        body.requirement.calls = actual.requirement.calls;
    }
    let index = inventory::collect(&modules, &evidence.bodies, definitions, cancel, &mut budget)?;
    evidence.targets = index.targets.clone();
    admission::validate_inventory(&evidence, &index.borrowed(), definitions, &mut budget)?;
    Ok(evidence)
}

pub(crate) fn verify(
    modules: &[&MirModule],
    evidence: &UpdateEvidence,
    definitions: &DefinitionTable,
    catalog: &ProofCatalog<'_>,
    cancel: &CancellationToken,
) -> Result<(), UpdateError> {
    let mut budget = Budget::new(Limits::default(), cancel)?;
    let index = inventory::collect(modules, &evidence.bodies, definitions, cancel, &mut budget)?;
    admission::validate_inventory(
        evidence,
        &Inventory {
            bodies: &index.bodies,
            ..index.borrowed()
        },
        definitions,
        &mut budget,
    )?;
    let actual = extract(modules, evidence, definitions, cancel, &mut budget)?;
    let sources = evidence
        .sources
        .iter()
        .map(|source| (source.declaration, source))
        .collect::<HashMap<_, _>>();
    let extraction = Context::new(modules, definitions, cancel, &mut budget)?;
    for body in &actual {
        budget.visit()?;
        let source = match body.classification {
            BodyClassification::Named(id) => Some(extraction.source(id)),
            BodyClassification::StaticInitializer(id) => Some(id),
            _ => None,
        };
        if let Some(id) = source {
            let source = sources.get(&id).ok_or(UpdateError::InvalidEvidence)?;
            if !source.bodies.contains(&body.requirement.origin) {
                return Err(UpdateError::InvalidEvidence);
            }
        }
    }
    for (declared, actual) in evidence.bodies.iter().zip(&actual) {
        admission::validate_executable_edges(declared, actual, &mut budget)?;
    }
    admission::validate_semantics(
        evidence,
        modules
            .iter()
            .flat_map(|module| module.native_applications()),
        catalog,
        &mut budget,
    )
}

fn extract(
    modules: &[&MirModule],
    evidence: &UpdateEvidence,
    definitions: &DefinitionTable,
    cancel: &CancellationToken,
    budget: &mut Budget<'_>,
) -> Result<Vec<BodyRecord>, UpdateError> {
    let mut declared = HashMap::new();
    for body in &evidence.bodies {
        budget.visit()?;
        if declared.insert(&body.requirement.origin, body).is_some() {
            return Err(UpdateError::InvalidEvidence);
        }
    }
    let extraction = Context::new(modules, definitions, cancel, budget)?;
    let mut actual = Vec::new();
    for module in modules {
        for function in &module.functions {
            budget.visit()?;
            let previous = declared
                .remove(&function.instance)
                .ok_or_else(|| UpdateError::MissingBody(function.instance.clone()))?;
            actual.push(canonical::extract(
                &extraction,
                module,
                function,
                previous.classification.clone(),
                Some(previous),
                budget,
            )?);
        }
    }
    if !declared.is_empty() {
        return Err(UpdateError::InvalidEvidence);
    }
    // The certificate body order is canonical module/function order, not a lookup
    // hint that could silently associate a recipe with a different executable.
    Ok(actual)
}

pub(crate) fn mapping_error(error: UpdateError) -> DefinitionMappingError {
    match error {
        UpdateError::Cancelled => DefinitionMappingError::Cancelled,
        UpdateError::LimitExceeded => DefinitionMappingError::LimitExceeded,
        UpdateError::Identity(error) => error,
        _ => DefinitionMappingError::InvalidContract,
    }
}

pub(crate) fn preflight(
    evidence: &UpdateEvidence,
    definitions: &DefinitionTable,
    cancel: &CancellationToken,
    budget: &mut Budget<'_>,
) -> Result<(), UpdateError> {
    let mut references = HashSet::new();
    evidence.visit_definition_ids(
        &mut |id| {
            budget.visit().map_err(mapping_error)?;
            references.insert(*id);
            Ok(())
        },
        cancel,
    )?;
    let encoding = definitions
        .encode(references)
        .map_err(DefinitionMappingError::from)?;
    let _ = UpdateEvidenceRecord::encode(evidence, &encoding, budget)?;
    Ok(())
}

/// Stable call indexes preserve references in the pre-optimization instruction list.
fn call(
    calls: &mut Vec<CallRequirement>,
    required: CallRequirement,
    budget: &mut Budget<'_>,
) -> Result<CanonicalOperand, UpdateError> {
    for (index, call) in calls.iter().enumerate() {
        budget.visit()?;
        if call == &required {
            return Ok(CanonicalOperand::Call(index as u32));
        }
    }
    budget.records(1)?;
    if calls.len() >= MAX_NESTED {
        return Err(UpdateError::LimitExceeded);
    }
    let index = calls.len();
    calls.push(required);
    Ok(CanonicalOperand::Call(index as u32))
}
