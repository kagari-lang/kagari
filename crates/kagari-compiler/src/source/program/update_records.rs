//! Translate complete checked templates before executable reachability or simplification.

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
    callable::generic::GenericBody,
    types::{
        ConcreteFunctionIdentity,
        named::{NamedContract, NamedDeclaration},
    },
    updates::{
        CanonicalOperand, Dependency, ProtectedRoot, RootKind, SourceRecord, SourceToken,
        UnknownDependency, UpdateEvidence, limits::MAX_NESTED,
    },
};
use kagari_hir::{
    program::CheckedProgram,
    update_source::{
        FactKind, SourceBodyKind, SourceBudget, SourceEvidence, SourceFact, SourceRootKind,
        TokenRange, UnknownDependency as SourceUnknown,
    },
};
use kagari_mir::{function::MirModule, program::updates, verify::verify_mir};
use kagari_types::ty::Ty;
use std::collections::{BTreeMap, HashMap, HashSet, btree_map::Entry};

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
    let indices = sources
        .iter()
        .enumerate()
        .map(|(index, source)| (source.declaration, index))
        .collect::<HashMap<_, _>>();
    for (source, body) in bodies {
        check_cancel(cancel)?;
        let index = indices
            .get(&source)
            .ok_or(DefinitionMappingError::InvalidContract)?;
        let source = &mut sources[*index];
        let edge = Dependency::Body(body);
        if !source.dependencies.contains(&edge) {
            if source.dependencies.len() >= MAX_NESTED {
                return Err(DefinitionMappingError::LimitExceeded);
            }
            source.dependencies.push(edge);
        }
    }
    Ok(sources)
}

pub(super) struct SourceInputs {
    pub sources: Vec<SourceRecord>,
    pub roots: Vec<ProtectedRoot>,
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
        evidence.roots.extend(collected.roots);
        evidence.selections.extend(collected.selections);
        declarations.extend(inventory.into_records().named_declarations);
    }
    evidence.canonical_order(program.root().definitions(), cancel)?;
    let mut inputs = lower(evidence, declarations, cancel)?;
    update_source::complete_inherent_owners(
        &mut inputs.sources,
        &inputs.declarations,
        program.root().definitions(),
        cancel,
    )?;
    Ok(inputs)
}

fn empty(declaration: DefinitionId) -> SourceRecord {
    SourceRecord {
        declaration,
        generic: None,
        tokens: vec![],
        inputs: vec![],
        calls: vec![],
        dependencies: vec![],
    }
}

fn lower(
    evidence: SourceEvidence,
    declarations: Vec<NamedDeclaration>,
    cancel: &CancellationToken,
) -> Result<SourceInputs, DefinitionMappingError> {
    let implementations = declarations
        .iter()
        .filter(|record| matches!(record.contract, NamedContract::Implementation(_)))
        .cloned()
        .collect::<Vec<_>>();
    let mut records = BTreeMap::new();
    let mut order = Vec::new();
    for body in &evidence.bodies {
        check_cancel(cancel)?;
        if records
            .insert(body.declaration, empty(body.declaration))
            .is_some()
        {
            return Err(DefinitionMappingError::InvalidContract);
        }
        order.push(body.declaration);
    }
    // Selection-only owners are real checked impl declarations, not invented bodies.
    // They retain unused generic relationships even when no method instance exists.
    for owner in evidence
        .selections
        .iter()
        .filter_map(|fact| fact.targets.first())
        .copied()
        .chain(evidence.roots.iter().map(|root| root.declaration))
        .chain(implementations.iter().map(|record| record.declaration))
    {
        if let Entry::Vacant(entry) = records.entry(owner) {
            entry.insert(empty(owner));
            order.push(owner);
        }
    }
    let source_ids = records.keys().copied().collect::<HashSet<_>>();
    for body in evidence.bodies {
        check_cancel(cancel)?;
        let record = records
            .get_mut(&body.declaration)
            .expect("collected source owner");
        record.generic = (!body.parameters.is_empty()).then_some(GenericBody {
            parameters: body.parameters,
            bounds: body.bounds,
        });
        record.tokens = body
            .tokens
            .into_iter()
            .map(|token| SourceToken {
                kind: token.kind as u32,
                text: token.text,
            })
            .collect();
        // Versioned local vocabulary; lengths delimit every fact and token range.
        record.inputs.push(CanonicalOperand::Unsigned(1));
        record
            .inputs
            .push(CanonicalOperand::Unsigned(match body.kind {
                SourceBodyKind::Function => 0,
                SourceBodyKind::AsyncFunction => 1,
                SourceBodyKind::Constant => 2,
                SourceBodyKind::StaticInitializer => 3,
                SourceBodyKind::Native => 4,
                SourceBodyKind::Required => 5,
            }));
        record.inputs.push(CanonicalOperand::Unsigned(u64::from(
            body.initializer.is_some(),
        )));
        if let Some(range) = body.initializer {
            range_inputs(record, range);
        }
        for fact in &body.facts {
            append_fact(record, fact, &source_ids, cancel)?;
        }
        bounded(record)?;
    }
    for fact in evidence.selections {
        check_cancel(cancel)?;
        let owner = fact
            .targets
            .first()
            .ok_or(DefinitionMappingError::InvalidContract)?;
        append_fact(
            records.get_mut(owner).expect("selection source owner"),
            &fact,
            &source_ids,
            cancel,
        )?;
    }
    for implementation in implementations {
        check_cancel(cancel)?;
        let NamedContract::Implementation(header) = implementation.contract else {
            unreachable!()
        };
        let record = records
            .get_mut(&implementation.declaration)
            .expect("checked implementation source owner");
        record.generic = (!header.generic_params.is_empty()).then_some(GenericBody {
            parameters: header.generic_params,
            bounds: header.bounds.clone(),
        });
        check_input_length(record, 4 + usize::from(header.trait_type.is_some()))?;
        // Checked header-only owners include inherent and memberless impls. This
        // records their actual contracts, not an invented executable body/token stream.
        record.inputs.extend([
            CanonicalOperand::Unsigned(200),
            CanonicalOperand::Type(header.receiver),
            CanonicalOperand::Unsigned(u64::from(header.trait_type.is_some())),
            CanonicalOperand::Unsigned(header.bounds.len() as u64),
        ]);
        if let Some(interface) = header.trait_type {
            record.inputs.push(CanonicalOperand::Type(interface));
        }
        for bound in header.bounds {
            check_cancel(cancel)?;
            check_input_length(record, 2 + bound.constraints.len())?;
            record.inputs.extend([
                CanonicalOperand::Type(bound.ty),
                CanonicalOperand::Unsigned(bound.constraints.len() as u64),
            ]);
            record.inputs.extend(
                bound
                    .constraints
                    .into_iter()
                    .map(|interface| CanonicalOperand::Type(Ty::Trait(interface))),
            );
        }
    }
    let mut roots = Vec::new();
    for root in evidence.roots {
        check_cancel(cancel)?;
        let kind = match root.kind {
            SourceRootKind::Hash => RootKind::KeyHash,
            SourceRootKind::Equality => RootKind::KeyEq,
            SourceRootKind::StaticInitializer => RootKind::StaticInitializer,
        };
        let record = records
            .get_mut(&root.declaration)
            .expect("protected source owner");
        let added = 4usize
            .checked_add(usize::from(root.receiver.is_some()))
            .and_then(|count| count.checked_add(usize::from(root.interface.is_some())))
            .and_then(|count| count.checked_add(root.members.len()))
            .ok_or(DefinitionMappingError::LimitExceeded)?;
        check_input_length(record, added)?;
        record.inputs.extend([
            CanonicalOperand::Unsigned(match kind {
                RootKind::KeyHash => 100,
                RootKind::KeyEq => 101,
                RootKind::StaticInitializer => 102,
            }),
            CanonicalOperand::Unsigned(u64::from(root.receiver.is_some())),
            CanonicalOperand::Unsigned(u64::from(root.interface.is_some())),
            CanonicalOperand::Unsigned(root.members.len() as u64),
        ]);
        if let Some(receiver) = root.receiver {
            record.inputs.push(CanonicalOperand::Type(receiver));
        }
        if let Some(interface) = root.interface {
            record
                .inputs
                .push(CanonicalOperand::Type(Ty::Trait(interface)));
        }
        for member in root.members {
            check_cancel(cancel)?;
            record.inputs.push(CanonicalOperand::Declaration(member));
            if source_ids.contains(&member) {
                dependency(record, Dependency::Source(member));
            }
        }
        bounded(record)?;
        roots.push(ProtectedRoot {
            declaration: ConcreteFunctionIdentity {
                declaration: root.declaration,
                arguments: vec![],
            },
            kind,
            dependencies: vec![Dependency::Source(root.declaration)],
            selections: vec![],
        });
    }
    let sources = order
        .into_iter()
        .map(|id| records.remove(&id).expect("source inventory"))
        .collect();
    Ok(SourceInputs {
        sources,
        roots,
        declarations,
    })
}

fn dependency(record: &mut SourceRecord, dependency: Dependency) {
    if !record.dependencies.contains(&dependency) {
        record.dependencies.push(dependency);
    }
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
    sources: &HashSet<DefinitionId>,
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
    check_input_length(record, added)?;
    record
        .inputs
        .push(CanonicalOperand::Unsigned(fact_kind(fact.kind)));
    range_inputs(record, fact.site);
    record
        .inputs
        .push(CanonicalOperand::Unsigned(fact.targets.len() as u64));
    for id in &fact.targets {
        record.inputs.push(CanonicalOperand::Declaration(*id));
        if sources.contains(id) {
            dependency(record, Dependency::Source(*id));
        }
    }
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
    if matches!(fact.kind, FactKind::Unknown(_)) {
        dependency(
            record,
            Dependency::Unknown(UnknownDependency {
                edge: u32::try_from(record.inputs.len())
                    .map_err(|_| DefinitionMappingError::LimitExceeded)?,
            }),
        );
    }
    bounded(record)
}

fn check_input_length(record: &SourceRecord, added: usize) -> Result<(), DefinitionMappingError> {
    if record
        .inputs
        .len()
        .checked_add(added)
        .is_none_or(|length| length > MAX_NESTED)
    {
        Err(DefinitionMappingError::LimitExceeded)
    } else {
        Ok(())
    }
}

fn bounded(record: &SourceRecord) -> Result<(), DefinitionMappingError> {
    if record.inputs.len() > MAX_NESTED || record.dependencies.len() > MAX_NESTED {
        Err(DefinitionMappingError::LimitExceeded)
    } else {
        Ok(())
    }
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
        FactKind::Selection => 7,
        FactKind::Member => 8,
        FactKind::Construction => 9,
        FactKind::DeclaredCoercion => 10,
        FactKind::BoundCoercion => 11,
        FactKind::IntrinsicCoercion => 12,
        FactKind::InterfaceUpcast => 13,
        FactKind::AssociatedConstant => 14,
        FactKind::NativeRequirement => 15,
        FactKind::Closure => 16,
        FactKind::AsyncClosure => 17,
        FactKind::AsyncBlock => 18,
        FactKind::Await => 19,
        FactKind::Capture => 20,
        FactKind::Binding => 21,
        FactKind::HostAccess => 22,
        FactKind::Unknown(reason) => {
            32 + match reason {
                SourceUnknown::FunctionValue => 0,
                SourceUnknown::FutureResume => 1,
                SourceUnknown::InterfaceDispatch => 2,
                SourceUnknown::NativeReentry => 3,
                SourceUnknown::RuntimeHelper => 4,
                SourceUnknown::HostAccess => 5,
                SourceUnknown::MissingCheckedFact => 6,
            }
        }
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod impl_owner_tests {
    use super::*;
    use kagari_contract::{
        types::named::correspondence::DeclarationCorrespondence,
        updates::{
            limits::{Budget, UpdateError},
            protected::compare_protected,
        },
    };
    use kagari_hir::analysis::AnalysisDatabase;
    use kagari_source::source_database::{SourceDatabase, SourceLayer};
    use kagari_stdlib::catalog;

    const JOINED: &str = r#"
struct Key<T> { val value: T }
impl<T> Key<T> {
    fn first(self) -> i32 { self.second() }
    fn second(self) -> i32 { helper() }
}
fn helper() -> i32 { 7 }
static val ROOT: i32 = Key { value: 0 }.first();
"#;
    const SPLIT: &str = r#"
struct Key<T> { val value: T }
impl<T> Key<T> { fn second(self) -> i32 { helper() } }
fn helper() -> i32 { 7 }
impl<T> Key<T> { fn first(self) -> i32 { self.second() } }
static val ROOT: i32 = Key { value: 0 }.first();
"#;

    fn checked(text: &str) -> CheckedProgram {
        let mut sources = SourceDatabase::default();
        let root = sources
            .set("owners.kgr", text.into(), SourceLayer::Base)
            .unwrap();
        let mut database = AnalysisDatabase::default();
        database.set_native_modules(catalog::shared().unwrap());
        database
            .snapshot(sources.snapshot(), &Default::default())
            .unwrap()
            .check_program(root, &Default::default())
            .unwrap()
    }

    fn mapped_sources(
        text: &str,
        correspondence: &mut DeclarationCorrespondence,
    ) -> DefinitionMetadata<UpdateEvidence> {
        let program = checked(text);
        let inputs = collect(&program, &Default::default()).unwrap();
        let definitions = program.root().definitions();
        let owner = program.root().lowered.source.module_identity();
        let evidence = UpdateEvidence {
            sources: inputs
                .sources
                .into_iter()
                .filter(|source| definitions.resolve(source.declaration).unwrap().module() == owner)
                .collect(),
            roots: inputs
                .roots
                .into_iter()
                .filter(|root| {
                    definitions
                        .resolve(root.declaration.declaration)
                        .unwrap()
                        .module()
                        == owner
                })
                .collect(),
            bodies: vec![],
            targets: vec![],
            selections: vec![],
        };
        correspondence
            .import(
                &evidence,
                inputs.declarations,
                definitions,
                &Default::default(),
            )
            .unwrap()
    }

    #[test]
    fn split_impl_headers_preserve_complete_members_and_reordered_source_facts() {
        let mut correspondence = DeclarationCorrespondence::new().unwrap();
        let joined = mapped_sources(JOINED, &mut correspondence);
        let split = mapped_sources(SPLIT, &mut correspondence);
        assert_eq!(
            split.records().sources.len(),
            joined.records().sources.len() + 1
        );
        let mut sources = HashMap::new();
        let mut repeated = 0;
        for source in &split.records().sources {
            if let Some(previous) = sources.insert(source.declaration, source) {
                // Per-declaration coverage survives publication. This assertion
                // establishes the exact equality required by shared import, not a
                // production deduplication or weaker protected-code comparison.
                assert_eq!(previous, source);
                repeated += 1;
                let names: Vec<_> = source
                    .dependencies
                    .iter()
                    .map(|dependency| {
                        let Dependency::Source(member) = dependency else {
                            panic!("member edge")
                        };
                        split
                            .definitions()
                            .resolve(*member)
                            .unwrap()
                            .last()
                            .unwrap()
                            .name
                            .to_owned()
                    })
                    .collect();
                assert_eq!(names, ["first", "second"]);
            }
        }
        assert_eq!(repeated, 1);
        for source in &joined.records().sources {
            assert_eq!(sources.remove(&source.declaration), Some(source));
        }
        assert!(sources.is_empty());
        assert_eq!(joined.records().roots, split.records().roots);
    }

    #[test]
    fn impl_member_and_transitive_helper_changes_are_not_merged_away() {
        let cancel = CancellationToken::default();
        let mut correspondence = DeclarationCorrespondence::new().unwrap();
        let previous = mapped_sources(JOINED, &mut correspondence);
        compare_protected(
            previous.records(),
            previous.records(),
            &mut Budget::new(Default::default(), &cancel).unwrap(),
        )
        .unwrap();
        for changed in [
            JOINED.replace(
                "fn second(self) -> i32 { helper() }",
                "fn second(self) -> i32 { 8 }",
            ),
            JOINED.replace("fn helper() -> i32 { 7 }", "fn helper() -> i32 { 8 }"),
        ] {
            let candidate = mapped_sources(&changed, &mut correspondence);
            assert!(matches!(
                compare_protected(
                    previous.records(),
                    candidate.records(),
                    &mut Budget::new(Default::default(), &cancel).unwrap()
                ),
                Err(UpdateError::ProtectedChange { .. })
            ));
        }
        let program = checked(SPLIT);
        let mut inputs = collect(&program, &cancel).unwrap();
        let header = inputs.declarations.iter().find(|record| matches!(&record.contract,
            NamedContract::Implementation(header) if header.trait_type.is_none()
                && program.root().definitions().resolve(record.declaration).unwrap().module() == program.root().lowered.source.module_identity()
        )).unwrap().declaration;
        inputs
            .sources
            .iter_mut()
            .find(|source| source.declaration == header)
            .unwrap()
            .tokens
            .push(SourceToken {
                kind: 0,
                text: "conflicting code".into(),
            });
        assert_eq!(
            update_source::complete_inherent_owners(
                &mut inputs.sources,
                &inputs.declarations,
                program.root().definitions(),
                &cancel
            ),
            Err(DefinitionMappingError::InvalidContract)
        );
    }
}
