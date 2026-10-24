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
        let mut evidence = fixture.evidence();
        evidence.roots[0]
            .dependencies
            .push(Dependency::Unknown(UnknownDependency { edge: 7 }));
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
                    generic_params: vec![parameter.clone()],
                    bounds: vec![],
                    receiver: receiver.clone(),
                    trait_type: None,
                }),
            });
            evidence.sources.push(SourceRecord {
                declaration: owner,
                generic: Some(GenericBody {
                    parameters: vec![parameter],
                    bounds: vec![],
                }),
                tokens: vec![SourceToken {
                    kind: 1,
                    text: "impl".into(),
                }],
                inputs: vec![CanonicalOperand::Type(receiver)],
                calls: vec![CallRequirement::Direct(fixture.identity("helper"))],
                dependencies: members.iter().copied().map(Dependency::Source).collect(),
            });
        }
        for (member, name) in members.into_iter().zip(["a", "b"]) {
            evidence.sources.push(SourceRecord {
                declaration: member,
                generic: None,
                tokens: vec![SourceToken {
                    kind: 2,
                    text: name.into(),
                }],
                inputs: vec![],
                calls: vec![],
                dependencies: vec![],
            });
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
        let bodies: Vec<_> = ["hash", "helper", "handler"]
            .into_iter()
            .map(|name| self.fixture.identity(name))
            .collect();
        validate_inventory(
            &self.evidence,
            &Inventory {
                sources: &self.source_inventory,
                bodies: &bodies,
                targets: &self.evidence.targets,
                roots: &self.evidence.roots,
                selections: &self.evidence.selections,
            },
            &self.fixture.context.snapshot(),
            &mut self.fixture.budget(),
        )
    }
}

#[test]
fn equal_split_headers_coalesce_after_import_without_losing_members_or_unknown_freeze() {
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
        compare_protected(raw.records(), raw.records(), &mut split.fixture.budget()),
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
    let header = &old.records().sources[1];
    assert_eq!(header.dependencies.len(), 2);
    for dependency in &header.dependencies {
        let Dependency::Source(member) = dependency else {
            panic!()
        };
        assert!(
            old.records()
                .sources
                .iter()
                .any(|source| source.declaration == *member)
        );
    }
    compare_protected(old.records(), new.records(), &mut split.fixture.budget()).unwrap();
    let mut changed = new.records().clone();
    changed.sources[2].tokens[0].text.push('!');
    assert!(matches!(
        compare_protected(old.records(), &changed, &mut split.fixture.budget()),
        Err(UpdateError::ProtectedChange {
            unknown_edge: Some(7),
            ..
        })
    ));
    let mut added = new.records().clone();
    let mut source = added.sources[2].clone();
    source.declaration = added.bodies[2].requirement.origin.declaration;
    added.sources.push(source);
    assert!(matches!(
        compare_protected(old.records(), &added, &mut split.fixture.budget()),
        Err(UpdateError::ProtectedChange { .. })
    ));
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
fn conflicting_header_records_are_not_unioned_or_silently_dropped() {
    let mut split = SplitHeaders::new(true);
    let original = split.evidence.clone();
    for field in 0..5 {
        split.evidence = original.clone();
        let source = &mut split.evidence.sources[2];
        match field {
            0 => source.tokens[0].text.push('!'),
            1 => source.inputs.push(CanonicalOperand::Signed(99)),
            2 => source.calls.clear(),
            3 => {
                source.dependencies.pop();
            }
            4 => source.generic = None,
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
    for source in &mut split.evidence.sources[1..3] {
        source.dependencies[1] = Dependency::Source(duplicate);
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
