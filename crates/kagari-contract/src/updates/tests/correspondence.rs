use super::*;
use crate::{
    source::{
        AuthoredDeclaration, AuthoredKind,
        manifest::{SourceDocumentAnchor, SourceManifest},
    },
    types::{
        InterfaceBridge,
        named::{ImplementationContract, NamedContract, NamedDeclaration},
    },
    updates::correspondence::import_verified_evidence,
};
use kagari_common::{identity::mapping::DefinitionMappingError, span::Span};
use kagari_types::visibility::Visibility;

struct SplitHeaders {
    fixture: Fixture,
    evidence: UpdateEvidence,
    headers: Vec<NamedDeclaration>,
    source_inventory: Vec<DefinitionId>,
}

impl SplitHeaders {
    fn new(split: bool) -> Self {
        let fixture = Fixture::new();
        let mut evidence = fixture.initializer_evidence();
        let owners: Vec<_> = (0..if split { 2 } else { 1 })
            .map(|index| {
                fixture
                    .context
                    .intern_child(fixture.root, DefinitionKind::Impl, "", 10 + index)
                    .unwrap()
            })
            .collect();
        let members: Vec<_> = ["a", "b"]
            .into_iter()
            .enumerate()
            .map(|(index, name)| {
                fixture
                    .context
                    .intern_child(
                        owners[if split { index } else { 0 }],
                        DefinitionKind::Method,
                        name,
                        0,
                    )
                    .unwrap()
            })
            .collect();
        let source_inventory = [fixture.identity("hash").declaration]
            .into_iter()
            .chain(owners.iter().copied())
            .chain(members.iter().copied())
            .collect();
        let mut headers = Vec::new();
        for owner in owners {
            let parameter = GenericParam { owner, position: 0 };
            let receiver = Ty::Tuple(vec![parameter.as_type()]);
            headers.push(NamedDeclaration {
                declaration: owner,
                visibility: Visibility::Private,
                ambiguous_local_body: None,
                contract: NamedContract::Implementation(ImplementationContract {
                    bridge: None,
                    generic_params: vec![parameter],
                    bounds: vec![],
                    receiver,
                    trait_type: None,
                }),
            });
            evidence.sources.push(SourceRecord {
                declaration: owner,
                generic: None,
                tokens: vec![],
                inputs: vec![],
                calls: vec![],
                bodies: vec![],
            });
        }
        for (member, name) in members.into_iter().zip(["a", "b"]) {
            let mut body = fixture.body(name, vec![]);
            body.requirement.origin.declaration = member;
            body.classification = BodyClassification::Named(member);
            evidence.sources.push(SourceRecord {
                declaration: member,
                generic: None,
                tokens: vec![],
                inputs: vec![],
                calls: vec![],
                bodies: vec![body.requirement.origin.clone()],
            });
            evidence.targets.push(CurrentTarget {
                application: body.requirement.origin.clone(),
                body: body.requirement.origin.clone(),
                module: 0,
                function: FunctionRef::new(evidence.bodies.len()),
            });
            evidence.bodies.push(body);
        }
        Self {
            fixture,
            evidence,
            headers,
            source_inventory,
        }
    }

    fn attach_manifest(&mut self) {
        self.evidence.source_manifest = Some(SourceManifest {
            documents: vec![SourceDocumentAnchor {
                modules: vec![
                    self.fixture
                        .context
                        .snapshot()
                        .resolve(self.fixture.root)
                        .unwrap()
                        .module()
                        .clone(),
                ],
                uri: "update.kgr".into(),
                byte_length: 100,
                fingerprint: 1,
            }],
            declarations: self
                .evidence
                .sources
                .iter()
                .enumerate()
                .map(|(index, source)| AuthoredDeclaration {
                    declaration: source.declaration,
                    kind: AuthoredKind::Declaration,
                    document: 0,
                    range: Span::new(index * 10, index * 10 + 5),
                    fingerprint: index as u64,
                })
                .collect(),
        });
    }

    fn admit(&self) -> Result<(), UpdateError> {
        let bodies: Vec<_> = self
            .evidence
            .bodies
            .iter()
            .map(|body| body.requirement.origin.clone())
            .collect();
        validate_inventory(
            &self.evidence,
            &Inventory {
                sources: &self.source_inventory,
                bodies: &bodies,
                targets: &self.evidence.targets,
                initializers: &self.evidence.initializers,
                selections: &self.evidence.selections,
            },
            &self.fixture.context.snapshot(),
            &mut self.fixture.budget(),
        )
    }
}

#[test]
fn equal_split_headers_coalesce_without_losing_member_provenance() {
    let mut split = SplitHeaders::new(true);
    let mut merged = SplitHeaders::new(false);
    split.attach_manifest();
    merged.attach_manifest();
    let original_manifest = split.evidence.source_manifest.clone();
    split.admit().unwrap();
    merged.admit().unwrap();
    let mut correspondence = DeclarationCorrespondence::new().unwrap();
    let raw = correspondence
        .import(
            &split.evidence,
            split.headers.iter().cloned(),
            &split.fixture.context.snapshot(),
            &split.fixture.cancel,
        )
        .unwrap();
    assert_eq!(
        compare_initializers(raw.records(), raw.records(), &mut split.fixture.budget()),
        Err(UpdateError::InvalidEvidence)
    );
    let old = import_verified_evidence(
        &mut correspondence,
        &split.evidence,
        &split.headers,
        &split.fixture.context.snapshot(),
        &mut split.fixture.budget(),
    )
    .unwrap();
    let new = import_verified_evidence(
        &mut correspondence,
        &merged.evidence,
        &merged.headers,
        &merged.fixture.context.snapshot(),
        &mut merged.fixture.budget(),
    )
    .unwrap();
    assert_eq!(split.evidence.source_manifest, original_manifest);
    assert!(old.records().source_manifest.is_none());
    assert!(new.records().source_manifest.is_none());
    assert_eq!(split.evidence.sources.len(), 5);
    assert_eq!(old.records().sources.len(), 4);
    assert_eq!(old.records(), new.records());
    for source in &old.records().sources[2..] {
        assert_eq!(source.bodies.len(), 1);
        assert!(old.records().bodies.iter().any(|body| {
            body.requirement.origin == source.bodies[0]
                && body.classification == BodyClassification::Named(source.declaration)
        }));
    }
    compare_initializers(old.records(), new.records(), &mut split.fixture.budget()).unwrap();
    let mut changed = new.records().clone();
    changed.bodies[3].actual_instructions[0].opcode = 99;
    compare_initializers(old.records(), &changed, &mut split.fixture.budget()).unwrap();
}

#[test]
fn corresponding_header_provenance_is_unioned_without_merging_executable_bodies() {
    let mut split = SplitHeaders::new(true);
    for index in 0..2 {
        let body = split.evidence.sources[3 + index].bodies.pop().unwrap();
        split.evidence.sources[1 + index].bodies.push(body);
    }
    split.admit().unwrap();
    let imported = import_verified_evidence(
        &mut DeclarationCorrespondence::new().unwrap(),
        &split.evidence,
        &split.headers,
        &split.fixture.context.snapshot(),
        &mut split.fixture.budget(),
    )
    .unwrap();
    let header = &imported.records().sources[1];
    assert_eq!(header.bodies.len(), 2);
    assert_ne!(header.bodies[0], header.bodies[1]);
    assert_eq!(imported.records().bodies.len(), split.evidence.bodies.len());
    for body in &header.bodies {
        assert!(
            imported
                .records()
                .bodies
                .iter()
                .any(|record| &record.requirement.origin == body)
        );
    }
    // Distinct authored headers cannot both claim the same original executable.
    let duplicate = split.evidence.sources[1].bodies[0].clone();
    split.evidence.sources[2].bodies.push(duplicate);
    assert_eq!(
        import_verified_evidence(
            &mut DeclarationCorrespondence::new().unwrap(),
            &split.evidence,
            &split.headers,
            &split.fixture.context.snapshot(),
            &mut split.fixture.budget(),
        )
        .unwrap_err(),
        UpdateError::InvalidEvidence,
    );
}

#[test]
fn comparison_projection_validates_physical_manifest_before_coalescing() {
    let mut split = SplitHeaders::new(true);
    split.attach_manifest();
    let original = split.evidence.source_manifest.clone();
    for invalid_case in 0..3 {
        split.evidence.source_manifest = original.clone();
        let manifest = split.evidence.source_manifest.as_mut().unwrap();
        match invalid_case {
            0 => manifest.declarations[0].range.end = 101,
            1 => manifest.declarations[1].declaration = manifest.declarations[0].declaration,
            2 => {
                manifest.declarations.pop();
            }
            _ => unreachable!(),
        }
        let result = import_verified_evidence(
            &mut DeclarationCorrespondence::new().unwrap(),
            &split.evidence,
            &split.headers,
            &split.fixture.context.snapshot(),
            &mut split.fixture.budget(),
        );
        assert_eq!(
            result.unwrap_err(),
            if invalid_case == 2 {
                UpdateError::InvalidEvidence
            } else {
                UpdateError::Identity(DefinitionMappingError::InvalidContract)
            }
        );
    }
}

#[test]
fn initializer_facts_on_corresponding_headers_are_not_silently_dropped() {
    let mut split = SplitHeaders::new(true);
    let original = split.evidence.clone();
    for field in 0..4 {
        split.evidence = original.clone();
        let source = &mut split.evidence.sources[2];
        match field {
            0 => source.tokens.push(SourceToken {
                kind: 1,
                text: "!".into(),
            }),
            1 => source.inputs.push(CanonicalOperand::Signed(99)),
            2 => source
                .calls
                .push(CallRequirement::Direct(split.fixture.identity("helper"))),
            3 => {
                source.generic = Some(GenericBody {
                    parameters: vec![],
                    bounds: vec![],
                });
            }
            _ => unreachable!(),
        }
        split.admit().unwrap();
        let result = import_verified_evidence(
            &mut DeclarationCorrespondence::new().unwrap(),
            &split.evidence,
            &split.headers,
            &split.fixture.context.snapshot(),
            &mut split.fixture.budget(),
        );
        assert_eq!(
            result.unwrap_err(),
            UpdateError::InvalidEvidence,
            "field {field}"
        );
    }
}

#[test]
fn only_distinct_original_authored_inherent_headers_can_coalesce() {
    let mut split = SplitHeaders::new(true);
    let original = split.evidence.clone();
    // Even an identical original source record is not a correspondence collision.
    split
        .evidence
        .sources
        .push(split.evidence.sources[1].clone());
    assert!(split.admit().is_err());
    assert_eq!(
        import_verified_evidence(
            &mut DeclarationCorrespondence::new().unwrap(),
            &split.evidence,
            &split.headers,
            &split.fixture.context.snapshot(),
            &mut split.fixture.budget()
        )
        .unwrap_err(),
        UpdateError::InvalidEvidence
    );
    split.evidence = original;
    // Two distinct original members with the same name collapse too, but are not headers.
    let duplicate = split
        .fixture
        .context
        .intern_child(split.headers[1].declaration, DefinitionKind::Method, "a", 0)
        .unwrap();
    split.evidence.sources[4] = SourceRecord {
        declaration: duplicate,
        ..split.evidence.sources[3].clone()
    };
    split.evidence.sources[4].bodies.clear();
    assert_eq!(
        import_verified_evidence(
            &mut DeclarationCorrespondence::new().unwrap(),
            &split.evidence,
            &split.headers,
            &split.fixture.context.snapshot(),
            &mut split.fixture.budget()
        )
        .unwrap_err(),
        UpdateError::InvalidEvidence
    );
    let mut split = SplitHeaders::new(true);
    for record in &mut split.headers {
        let NamedContract::Implementation(header) = &mut record.contract else {
            panic!()
        };
        header.bridge = Some(InterfaceBridge::Intrinsic);
    }
    assert_eq!(
        import_verified_evidence(
            &mut DeclarationCorrespondence::new().unwrap(),
            &split.evidence,
            &split.headers,
            &split.fixture.context.snapshot(),
            &mut split.fixture.budget()
        )
        .unwrap_err(),
        UpdateError::InvalidEvidence
    );
    let trait_id = split
        .fixture
        .context
        .intern_child(split.fixture.root, DefinitionKind::Trait, "Marker", 0)
        .unwrap();
    for record in &mut split.headers {
        let NamedContract::Implementation(header) = &mut record.contract else {
            panic!()
        };
        header.bridge = None;
        header.trait_type = Some(Ty::Trait(NominalTy {
            declaration: trait_id,
            arguments: vec![],
            associated_types: Default::default(),
        }));
    }
    assert_eq!(
        import_verified_evidence(
            &mut DeclarationCorrespondence::new().unwrap(),
            &split.evidence,
            &split.headers,
            &split.fixture.context.snapshot(),
            &mut split.fixture.budget()
        )
        .unwrap_err(),
        UpdateError::InvalidEvidence
    );
}

#[test]
fn import_normalization_observes_the_shared_budget_and_cancellation() {
    let split = SplitHeaders::new(true);
    for limits in [
        Limits {
            visits: 0,
            ..Limits::default()
        },
        Limits {
            bytes: 0,
            ..Limits::default()
        },
    ] {
        let mut budget = Budget::new(limits, &split.fixture.cancel).unwrap();
        assert_eq!(
            import_verified_evidence(
                &mut DeclarationCorrespondence::new().unwrap(),
                &split.evidence,
                &split.headers,
                &split.fixture.context.snapshot(),
                &mut budget
            )
            .unwrap_err(),
            UpdateError::LimitExceeded
        );
    }
    let mut budget = split.fixture.budget();
    split.fixture.cancel.cancel();
    assert_eq!(
        import_verified_evidence(
            &mut DeclarationCorrespondence::new().unwrap(),
            &split.evidence,
            &split.headers,
            &split.fixture.context.snapshot(),
            &mut budget
        )
        .unwrap_err(),
        UpdateError::Cancelled
    );
}
