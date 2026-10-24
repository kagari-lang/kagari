//! Inventory reconciliation and local evidence validation, shared by extractors.
mod semantic;
use crate::{
    native_import::NativeImport,
    types::{ConcreteFunctionIdentity, proofs::ProofCatalog},
    updates::{
        BodyClassification, BodyRecord, CallRequirement, CanonicalOperand, CurrentTarget,
        Dependency, ProtectedRoot, RootKind, UpdateEvidence,
        limits::{Budget, MAX_APPLICATIONS, MAX_NESTED, UpdateError},
        wire::UpdateEvidenceRecord,
    },
};
use kagari_common::identity::{
    mapping::DefinitionMappingError,
    scoped::DefinitionReferences,
    table::{DefinitionId, DefinitionTable},
};
use std::collections::{HashMap, HashSet};

/// Independent inventories obtained from checked declarations and actual executable
/// placements. Never construct these lists by enumerating the certificate itself.
pub struct Inventory<'a> {
    pub sources: &'a [DefinitionId],
    pub bodies: &'a [ConcreteFunctionIdentity],
    pub targets: &'a [CurrentTarget],
    pub roots: &'a [ProtectedRoot],
    pub selections: &'a [CallRequirement],
}

/// Reject omitted, duplicated and invented records, then charge the complete wire
/// representation. This does not authenticate source text or installed native code.
pub fn validate_inventory(
    evidence: &UpdateEvidence,
    inventory: &Inventory<'_>,
    definitions: &DefinitionTable,
    budget: &mut Budget<'_>,
) -> Result<(), UpdateError> {
    validate_shape(evidence, definitions, budget)?;
    let mut sources = HashSet::new();
    for source in &evidence.sources {
        budget.visit()?;
        if !sources.insert(source.declaration) {
            return Err(UpdateError::InvalidEvidence);
        }
    }
    if sources.len() != inventory.sources.len() {
        return Err(UpdateError::InvalidEvidence);
    }
    for source in inventory.sources {
        budget.visit()?;
        if !sources.remove(source) {
            return Err(UpdateError::InvalidEvidence);
        }
    }
    let mut bodies = HashSet::new();
    for body in &evidence.bodies {
        budget.visit()?;
        if !bodies.insert(&body.requirement.origin) {
            return Err(UpdateError::InvalidEvidence);
        }
    }
    for body in inventory.bodies {
        budget.visit()?;
        if !bodies.remove(body) {
            return Err(UpdateError::MissingBody(body.clone()));
        }
    }
    if !bodies.is_empty()
        || evidence.bodies.len() != inventory.bodies.len()
        || evidence.targets != inventory.targets
        || evidence.roots != inventory.roots
        || evidence.selections != inventory.selections
    {
        return Err(UpdateError::InvalidEvidence);
    }
    charge_evidence(evidence, definitions, budget)
}

/// Validate signature scopes and every executable semantic call/witness using the
/// existing linked proof catalog. Source templates remain the checked collector's
/// responsibility; independent inventory reconciliation is also mandatory.
/// `native_imports` must enumerate the actual executable native-import inventory,
/// including templates used only through shared calls or function values. Every
/// variant is checked against its registered declaration; explicit native edges
/// must retain an exact supplied import. All records share the catalog's ID scope.
pub fn validate_semantics<'a>(
    evidence: &'a UpdateEvidence,
    native_imports: impl IntoIterator<Item = &'a NativeImport>,
    catalog: &ProofCatalog<'_>,
    budget: &mut Budget<'_>,
) -> Result<(), UpdateError> {
    semantic::validate(evidence, native_imports, catalog, budget)
}

/// Compare independently extracted emitted instructions with the declared actual
/// executable, as well as signature/classification and surviving semantic edges.
/// Extra preserved edges are permitted; edge equality alone never proves executable
/// equality. The backend owns opcode/operand vocabulary and local numbering.
pub fn validate_executable_edges(
    declared: &BodyRecord,
    actual: &BodyRecord,
    budget: &mut Budget<'_>,
) -> Result<(), UpdateError> {
    if declared.requirement.origin != actual.requirement.origin
        || declared.requirement.generic != actual.requirement.generic
        || declared.classification != actual.classification
        || declared.signature != actual.signature
        || declared.captures != actual.captures
        || declared.actual_format != actual.actual_format
        || declared.actual_instructions.len() != actual.actual_instructions.len()
    {
        return Err(UpdateError::InvalidEvidence);
    }
    for (expected, extracted) in declared
        .actual_instructions
        .iter()
        .zip(&actual.actual_instructions)
    {
        budget.visit()?;
        nested(expected.operands.len())?;
        nested(extracted.operands.len())?;
        if expected != extracted {
            return Err(UpdateError::InvalidEvidence);
        }
        for operand in &extracted.operands {
            budget.visit()?;
            if let CanonicalOperand::Call(index) = operand {
                let index = *index as usize;
                let expected = declared
                    .requirement
                    .calls
                    .get(index)
                    .ok_or(UpdateError::InvalidEvidence)?;
                if actual.requirement.calls.get(index) != Some(expected) {
                    return Err(UpdateError::InvalidEvidence);
                }
            }
        }
    }
    for call in &actual.requirement.calls {
        let mut found = false;
        for expected in &declared.requirement.calls {
            budget.visit()?;
            if call == expected {
                found = true;
                break;
            }
        }
        if !found {
            return Err(UpdateError::InvalidEvidence);
        }
    }
    Ok(())
}

pub(crate) fn charge_evidence(
    evidence: &UpdateEvidence,
    definitions: &DefinitionTable,
    budget: &mut Budget<'_>,
) -> Result<(), UpdateError> {
    let mut references = HashSet::new();
    let cancel = budget.cancel;
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

fn mapping_error(error: UpdateError) -> DefinitionMappingError {
    match error {
        UpdateError::Cancelled => DefinitionMappingError::Cancelled,
        UpdateError::LimitExceeded => DefinitionMappingError::LimitExceeded,
        _ => DefinitionMappingError::InvalidContract,
    }
}

pub(crate) fn nested(length: usize) -> Result<(), UpdateError> {
    if length > MAX_NESTED {
        Err(UpdateError::LimitExceeded)
    } else {
        Ok(())
    }
}

pub(crate) fn validate_shape(
    evidence: &UpdateEvidence,
    definitions: &DefinitionTable,
    budget: &mut Budget<'_>,
) -> Result<(), UpdateError> {
    budget.check()?;
    if let Some(manifest) = &evidence.source_manifest {
        manifest.validate(definitions, budget.cancel)?;
        let declarations = manifest
            .declarations
            .iter()
            .map(|record| record.declaration)
            .collect::<HashSet<_>>();
        for source in &evidence.sources {
            budget.visit()?;
            if !declarations.contains(&source.declaration) {
                return Err(UpdateError::InvalidEvidence);
            }
        }
    }
    if evidence.bodies.len() > MAX_APPLICATIONS || evidence.targets.len() > MAX_APPLICATIONS {
        return Err(UpdateError::LimitExceeded);
    }
    for source in &evidence.sources {
        budget.visit()?;
        nested(source.inputs.len())?;
        nested(source.calls.len())?;
        nested(source.dependencies.len())?;
        for operand in &source.inputs {
            operand_valid(operand, source.calls.len())?;
        }
    }
    let mut body_ids = HashMap::new();
    for body in &evidence.bodies {
        budget.visit()?;
        if body_ids.insert(&body.requirement.origin, body).is_some() {
            return Err(UpdateError::InvalidEvidence);
        }
        nested(body.captures.len())?;
        nested(body.requirement.calls.len())?;
        nested(body.dependencies.len())?;
        nested(body.signature.params.len())?;
        if let Some(generic) = &body.requirement.generic {
            nested(generic.parameters.len())?;
            nested(generic.bounds.len())?;
            if !generic.valid(&body.requirement.origin, budget.cancel, definitions) {
                return Err(UpdateError::InvalidEvidence);
            }
        }
        for instruction in body.instructions.iter().chain(&body.actual_instructions) {
            budget.visit()?;
            nested(instruction.operands.len())?;
            for operand in &instruction.operands {
                operand_valid(operand, body.requirement.calls.len())?;
            }
        }
    }
    let mut targets = HashSet::new();
    let mut slots = HashSet::new();
    for target in &evidence.targets {
        budget.visit()?;
        if !targets.insert(&target.application) || !slots.insert((target.module, target.function)) {
            return Err(UpdateError::InvalidEvidence);
        }
        let body = body_ids
            .get(&target.body)
            .ok_or_else(|| UpdateError::MissingBody(target.body.clone()))?;
        if !matches!(body.classification, BodyClassification::Named(id) if id == target.application.declaration)
        {
            return Err(UpdateError::InvalidEvidence);
        }
    }
    let source_ids: HashMap<_, _> = evidence
        .sources
        .iter()
        .map(|source| (source.declaration, source))
        .collect();
    if source_ids.len() != evidence.sources.len() {
        return Err(UpdateError::InvalidEvidence);
    }
    for dependencies in evidence
        .sources
        .iter()
        .map(|source| &source.dependencies)
        .chain(evidence.bodies.iter().map(|body| &body.dependencies))
        .chain(evidence.roots.iter().map(|root| &root.dependencies))
    {
        nested(dependencies.len())?;
        for dependency in dependencies {
            budget.visit()?;
            match dependency {
                Dependency::Source(id) if !source_ids.contains_key(id) => {
                    return Err(UpdateError::InvalidEvidence);
                }
                Dependency::Body(id) if !body_ids.contains_key(id) => {
                    return Err(UpdateError::MissingBody(id.clone()));
                }
                _ => {}
            }
        }
    }
    let mut roots = HashSet::new();
    for root in &evidence.roots {
        budget.visit()?;
        nested(root.selections.len())?;
        if root.kind == RootKind::StaticInitializer
            && (!root
                .dependencies
                .contains(&Dependency::Source(root.declaration.declaration))
                || source_ids
                    .get(&root.declaration.declaration)
                    .is_none_or(|source| source.tokens.is_empty()))
        {
            return Err(UpdateError::InvalidEvidence);
        }
        if root.dependencies.is_empty() || !roots.insert((&root.declaration, root.kind)) {
            return Err(UpdateError::InvalidEvidence);
        }
    }
    Ok(())
}

fn operand_valid(operand: &CanonicalOperand, calls: usize) -> Result<(), UpdateError> {
    match operand {
        CanonicalOperand::Call(index) if *index as usize >= calls => {
            Err(UpdateError::InvalidEvidence)
        }
        CanonicalOperand::Type(ty) if !ty.within_wire_limits() => Err(UpdateError::LimitExceeded),
        CanonicalOperand::Application(application) => nested(application.arguments.len()),
        _ => Ok(()),
    }
}
