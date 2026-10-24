//! Static expression evidence and explicit authored ownership before optimization.

use crate::source::{
    lower::{MirLoweringError, instances::MirLoweringOptions, module_contract, optimize_module},
    program::update_source,
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        map::DefinitionContext,
        mapping::{DefinitionMappingError, check_cancel},
        metadata::DefinitionMetadata,
        table::DefinitionId,
    },
};
use kagari_contract::{
    types::{
        ConcreteFunctionIdentity,
        named::{NamedContract, NamedDeclaration},
    },
    updates::{
        CanonicalOperand, SourceRecord, SourceToken, StaticInitializer, UpdateEvidence,
        limits::MAX_NESTED,
    },
};
use kagari_hir::{
    program::CheckedProgram,
    update_source::{
        FactKind, SourceBodyKind, SourceBudget, SourceEvidence, SourceFact, TokenRange,
    },
};
use kagari_mir::{function::MirModule, program::updates, verify::verify_mir};
use std::collections::HashMap;

pub(super) fn optimize(
    modules: Vec<MirModule>,
    evidence: UpdateEvidence,
    context: &DefinitionContext,
    options: &MirLoweringOptions,
) -> Result<(Vec<MirModule>, UpdateEvidence), MirLoweringError> {
    if options.optimization.is_none() {
        return Ok((modules, evidence));
    }
    let definitions = context.snapshot();
    let modules = modules
        .into_iter()
        .map(|module| {
            let metadata =
                DefinitionMetadata::checked(definitions.clone(), module, &options.cancel)?;
            let module = verify_mir(metadata, &options.cancel)?;
            optimize_module(module, options).map(|module| module.into_unverified().into_records())
        })
        .collect::<Result<Vec<_>, MirLoweringError>>()?;
    let evidence = updates::refresh(&modules, &evidence, &definitions, &options.cancel)
        .map_err(MirLoweringError::Update)?;
    Ok((modules, evidence))
}

/// Associate every lowered recipe with its actual authored owner, including
/// default bodies placed under a foreign impl and generated closure/resume bodies.
pub(super) fn attach_bodies(
    sources: &[SourceRecord],
    bodies: Vec<(DefinitionId, ConcreteFunctionIdentity)>,
    cancel: &CancellationToken,
) -> Result<Vec<SourceRecord>, DefinitionMappingError> {
    let mut sources = sources.to_vec();
    let mut indices = HashMap::new();
    let mut owners = HashMap::new();
    for (index, source) in sources.iter().enumerate() {
        check_cancel(cancel)?;
        if indices.insert(source.declaration, index).is_some() {
            return Err(DefinitionMappingError::InvalidContract);
        }
        if source.bodies.len() > MAX_NESTED {
            return Err(DefinitionMappingError::LimitExceeded);
        }
        for body in &source.bodies {
            check_cancel(cancel)?;
            if owners
                .insert(body.clone(), source.declaration)
                .is_some_and(|old| old != source.declaration)
            {
                return Err(DefinitionMappingError::InvalidContract);
            }
        }
    }
    for (source, body) in bodies {
        check_cancel(cancel)?;
        let index = indices
            .get(&source)
            .ok_or(DefinitionMappingError::InvalidContract)?;
        if owners
            .insert(body.clone(), source)
            .is_some_and(|old| old != source)
        {
            return Err(DefinitionMappingError::InvalidContract);
        }
        let source = &mut sources[*index];
        if !source.bodies.contains(&body) {
            if source.bodies.len() >= MAX_NESTED {
                return Err(DefinitionMappingError::LimitExceeded);
            }
            source.bodies.push(body);
        }
    }
    Ok(sources)
}

pub(super) struct SourceInputs {
    pub sources: Vec<SourceRecord>,
    pub initializers: Vec<StaticInitializer>,
    pub declarations: Vec<NamedDeclaration>,
}

pub(super) fn collect(
    program: &CheckedProgram,
    cancel: &CancellationToken,
) -> Result<SourceInputs, MirLoweringError> {
    let mut budget = SourceBudget::new(cancel);
    let mut evidence = SourceEvidence::default();
    let mut declarations = Vec::new();
    for module in program.modules() {
        check_cancel(cancel)?;
        let inventory = module_contract(module, cancel)?;
        let collected =
            update_source::collect(module, &inventory.records().named_declarations, &mut budget)?;
        evidence.bodies.extend(collected.bodies);
        evidence.initializers.extend(collected.initializers);
        declarations.extend(inventory.into_records().named_declarations);
    }
    evidence.canonical_order(program.root().definitions(), cancel)?;
    Ok(lower(evidence, declarations, cancel)?)
}

fn empty(declaration: DefinitionId) -> SourceRecord {
    SourceRecord {
        declaration,
        generic: None,
        tokens: vec![],
        inputs: vec![],
        calls: vec![],
        bodies: vec![],
    }
}

fn lower(
    evidence: SourceEvidence,
    declarations: Vec<NamedDeclaration>,
    cancel: &CancellationToken,
) -> Result<SourceInputs, DefinitionMappingError> {
    let mut sources = Vec::new();
    let mut indices = HashMap::new();
    for body in evidence.bodies {
        check_cancel(cancel)?;
        if indices.insert(body.declaration, sources.len()).is_some() {
            return Err(DefinitionMappingError::InvalidContract);
        }
        let mut record = empty(body.declaration);
        if body.kind == SourceBodyKind::StaticInitializer {
            let range = body
                .initializer
                .ok_or(DefinitionMappingError::InvalidContract)?;
            if range.start != 0 || range.end != body.tokens.len() || body.tokens.is_empty() {
                return Err(DefinitionMappingError::InvalidContract);
            }
            record.tokens = body
                .tokens
                .into_iter()
                .map(|token| SourceToken {
                    kind: token.kind as u32,
                    text: token.text,
                })
                .collect();
            // Local fact vocabulary, with lengths delimiting each fact and range.
            record.inputs.push(CanonicalOperand::Unsigned(1));
            for fact in &body.facts {
                append_fact(&mut record, fact, cancel)?;
            }
        }
        sources.push(record);
    }
    // Inventory/provenance remains complete without retaining ordinary computations.
    for declaration in &declarations {
        check_cancel(cancel)?;
        if !matches!(declaration.contract, NamedContract::Alias(_))
            && !indices.contains_key(&declaration.declaration)
        {
            indices.insert(declaration.declaration, sources.len());
            sources.push(empty(declaration.declaration));
        }
    }
    let initializers = evidence
        .initializers
        .into_iter()
        .map(|initializer| StaticInitializer {
            declaration: initializer.declaration,
        })
        .collect();
    Ok(SourceInputs {
        sources,
        initializers,
        declarations,
    })
}

fn range_inputs(record: &mut SourceRecord, range: TokenRange) {
    record.inputs.extend([
        CanonicalOperand::Unsigned(range.start as u64),
        CanonicalOperand::Unsigned(range.end as u64),
    ]);
}

fn append_fact(
    record: &mut SourceRecord,
    fact: &SourceFact,
    cancel: &CancellationToken,
) -> Result<(), DefinitionMappingError> {
    check_cancel(cancel)?;
    let added = 6usize
        .checked_add(fact.targets.len())
        .and_then(|count| count.checked_add(fact.types.len()))
        .and_then(|count| {
            fact.related
                .len()
                .checked_mul(2)
                .and_then(|related| count.checked_add(related))
        })
        .ok_or(DefinitionMappingError::LimitExceeded)?;
    if record
        .inputs
        .len()
        .checked_add(added)
        .is_none_or(|length| length > MAX_NESTED)
    {
        return Err(DefinitionMappingError::LimitExceeded);
    }
    record
        .inputs
        .push(CanonicalOperand::Unsigned(fact_kind(fact.kind)));
    range_inputs(record, fact.site);
    record
        .inputs
        .push(CanonicalOperand::Unsigned(fact.targets.len() as u64));
    record.inputs.extend(
        fact.targets
            .iter()
            .copied()
            .map(CanonicalOperand::Declaration),
    );
    record
        .inputs
        .push(CanonicalOperand::Unsigned(fact.types.len() as u64));
    record
        .inputs
        .extend(fact.types.iter().cloned().map(CanonicalOperand::Type));
    record
        .inputs
        .push(CanonicalOperand::Unsigned(fact.related.len() as u64));
    for range in &fact.related {
        range_inputs(record, *range);
    }
    Ok(())
}

fn fact_kind(kind: FactKind) -> u64 {
    match kind {
        FactKind::Reference => 0,
        FactKind::Type => 1,
        FactKind::Call => 2,
        FactKind::FunctionValue => 3,
        FactKind::TraitCall => 4,
        FactKind::Signature => 5,
        FactKind::Protocol => 6,
        FactKind::Member => 8,
        FactKind::Construction => 9,
        FactKind::DeclaredCoercion => 10,
        FactKind::BoundCoercion => 11,
        FactKind::IntrinsicCoercion => 12,
        FactKind::InterfaceUpcast => 13,
        FactKind::AssociatedConstant => 14,
        FactKind::Closure => 16,
        FactKind::AsyncClosure => 17,
        FactKind::AsyncBlock => 18,
        FactKind::Await => 19,
        FactKind::Capture => 20,
        FactKind::Binding => 21,
        FactKind::HostAccess => 22,
    }
}

#[cfg(test)]
mod tests;
