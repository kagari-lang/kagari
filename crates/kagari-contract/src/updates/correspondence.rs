//! Normalize verified source-header collisions only after semantic correspondence.
use crate::{
    types::named::{NamedContract, NamedDeclaration, correspondence::DeclarationCorrespondence},
    updates::{
        SourceRecord, UpdateEvidence,
        admission::{charge_evidence, nested, validate_shape},
        limits::{Budget, UpdateError},
    },
};
use kagari_common::identity::{
    metadata::DefinitionMetadata,
    table::{DefinitionId, DefinitionTable},
};
use std::collections::{HashMap, HashSet};

/// Import verified publication evidence and coalesce equal inherent-impl headers.
///
/// Call only after independent source/body/target inventory and semantic admission
/// in the original publication scope. `inventory` is the complete verified named
/// implementation inventory used by `DeclarationCorrespondence::import`; it may
/// also contain other named declarations. Use the same correspondence for previous
/// and candidate evidence, retained demands, and candidate coverage.
///
/// Two authored `impl<T> Box<T>` blocks with distinct members keep separate original
/// source records for backend coverage. Correspondence maps their checked headers
/// and binders to one owner. Only then may their ordinary source provenance be
/// merged. Canonical initializer facts must be empty on these headers; only authored
/// body lists are unioned, without discarding executable applications.
///
/// Original duplicate source IDs, trait/bridge impl collisions, non-impl collisions,
/// or header records carrying initializer facts reject.
/// Header equivalence is established by the existing importer, not by source text,
/// occurrence numbers, or a caller-supplied set of IDs allowed to collide.
///
/// The result is comparison metadata for `compare_initializers`, not a replacement
/// executable certificate. Never use it to bypass original inventory admission.
/// The physical source manifest is validated in the original publication scope and
/// omitted from this projection: distinct authored blocks can share a semantic impl
/// owner, but their source ranges cannot become one source-attachment authority.
/// The original evidence retains its manifest; canonical source records and their
/// authored body provenance remain in the comparison projection.
/// The shared budget charges the full pre-coalescing evidence (including bytes),
/// validation and normalization work; callers need not charge that evidence again.
/// Cancellation, exhaustion and identity errors propagate without changing `evidence`.
/// As with the underlying importer, the correspondence may retain names on failure.
pub fn import_verified_evidence(
    correspondence: &mut DeclarationCorrespondence,
    evidence: &UpdateEvidence,
    inventory: &[NamedDeclaration],
    definitions: &DefinitionTable,
    budget: &mut Budget<'_>,
) -> Result<DefinitionMetadata<UpdateEvidence>, UpdateError> {
    // Charge before importing/cloning and before comparing recursively nested values.
    charge_evidence(evidence, definitions, budget)?;
    validate_shape(evidence, definitions, budget)?;
    let mut inherent = HashMap::new();
    for record in inventory {
        budget.visit()?;
        if let NamedContract::Implementation(header) = &record.contract {
            let allowed = header.trait_type.is_none() && header.bridge.is_none();
            if inherent.insert(record.declaration, allowed).is_some() {
                return Err(UpdateError::InvalidEvidence);
            }
        }
    }
    let imported = correspondence.import(
        evidence,
        inventory.iter().cloned(),
        definitions,
        budget.cancel,
    )?;
    let (definitions, mut imported) = imported.into_parts();
    if imported.sources.len() != evidence.sources.len() {
        return Err(UpdateError::InvalidEvidence);
    }
    let mut sources: Vec<SourceRecord> = Vec::new();
    let mut seen: HashMap<DefinitionId, (usize, bool)> = HashMap::new();
    // Remapping preserves vector positions. Keep each original declaration beside
    // its remapped record so only importer-proven inherent-header collisions qualify.
    for (original, source) in evidence.sources.iter().zip(imported.sources) {
        budget.visit()?;
        let allowed = inherent
            .get(&original.declaration)
            .copied()
            .unwrap_or(false);
        if let Some(&(index, previous_allowed)) = seen.get(&source.declaration) {
            let previous = &mut sources[index];
            if !allowed
                || !previous_allowed
                || previous.generic.is_some()
                || source.generic.is_some()
                || !previous.tokens.is_empty()
                || !source.tokens.is_empty()
                || !previous.inputs.is_empty()
                || !source.inputs.is_empty()
                || !previous.calls.is_empty()
                || !source.calls.is_empty()
            {
                return Err(UpdateError::InvalidEvidence);
            }
            let mut bodies = HashSet::new();
            for body in &previous.bodies {
                budget.visit()?;
                bodies.insert(body.clone());
            }
            for body in source.bodies {
                budget.visit()?;
                if bodies.insert(body.clone()) {
                    nested(previous.bodies.len() + 1)?;
                    previous.bodies.push(body);
                }
            }
        } else {
            seen.insert(source.declaration, (sources.len(), allowed));
            sources.push(source);
        }
    }
    imported.sources = sources;
    // Physical source anchors were validated before semantic owner coalescing.
    // They belong to the original executable, not this comparison-only ID scope.
    imported.source_manifest = None;
    // No other collapsed key is excused; provenance must still name existing bodies.
    validate_shape(&imported, &definitions, budget)?;
    Ok(DefinitionMetadata::checked(
        definitions,
        imported,
        budget.cancel,
    )?)
}
