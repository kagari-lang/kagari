mod correspondence;
mod initializers;
mod native;
mod repairs;
mod shared;

use crate::{
    callable::{
        construction::ConstructionMethod, generic::GenericBody, interface::InterfaceCallContract,
    },
    ids::FunctionRef,
    native_import::NativeImport,
    types::{
        ConcreteFunctionIdentity, named::correspondence::DeclarationCorrespondence,
        proofs::ProofCatalog,
    },
    updates::{
        admission::{Inventory, validate_executable_edges, validate_inventory},
        demands::{
            Coverage, NativeClosure, check_coverage, expand_requests, publication_demands,
            union_demands,
        },
        initializers::compare_initializers,
        limits::{Budget, Limits, UpdateError},
        wire::UpdateEvidenceRecord,
        *,
    },
};
use bincode::{DefaultOptions, Options};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionKind, ModuleIdentity, map::DefinitionContext, scoped::DefinitionReferences,
        table::DefinitionId,
    },
};
use kagari_types::{
    callable::{CallableImplementation, MethodPolicy, Signature},
    declaration::{FnDecl, NativeDeclaration},
    scalar::BuiltinType,
    ty::{GenericParam, NominalTy, Ty},
};

struct Fixture {
    context: DefinitionContext,
    root: DefinitionId,
    cancel: CancellationToken,
}

impl Fixture {
    fn new() -> Self {
        let context = DefinitionContext::new().unwrap();
        let root = context
            .intern_root(&ModuleIdentity::single_file("update.kgr"))
            .unwrap();
        Self {
            context,
            root,
            cancel: CancellationToken::default(),
        }
    }

    fn identity(&self, name: &str) -> ConcreteFunctionIdentity {
        ConcreteFunctionIdentity {
            declaration: self
                .context
                .intern_child(self.root, DefinitionKind::Function, name, 0)
                .unwrap(),
            arguments: vec![],
        }
    }

    fn body(&self, name: &str, calls: Vec<CallRequirement>) -> BodyRecord {
        let origin = self.identity(name);
        BodyRecord {
            classification: BodyClassification::Named(origin.declaration),
            requirement: BodyRequirement {
                origin,
                generic: None,
                calls,
            },
            signature: Signature {
                params: vec![],
                result: Ty::Builtin(BuiltinType::I32),
            },
            captures: vec![],
            format: 1,
            instructions: vec![CanonicalInstruction {
                opcode: 1,
                operands: vec![CanonicalOperand::Signed(1)],
            }],
            actual_format: 1,
            actual_instructions: vec![CanonicalInstruction {
                opcode: 1,
                operands: vec![CanonicalOperand::Signed(1)],
            }],
        }
    }

    fn evidence(&self) -> UpdateEvidence {
        let helper = self.body("helper", vec![]);
        let hash = self.body(
            "hash",
            vec![CallRequirement::Direct(helper.requirement.origin.clone())],
        );
        let handler = self.body(
            "handler",
            vec![CallRequirement::Direct(helper.requirement.origin.clone())],
        );
        let sources = vec![SourceRecord {
            declaration: hash.requirement.origin.declaration,
            generic: None,
            tokens: vec![],
            inputs: vec![],
            calls: vec![],
            bodies: vec![hash.requirement.origin.clone()],
        }];
        let bodies = vec![hash, helper, handler];
        let targets = bodies
            .iter()
            .enumerate()
            .map(|(index, body)| CurrentTarget {
                application: body.requirement.origin.clone(),
                body: body.requirement.origin.clone(),
                module: 0,
                function: FunctionRef::new(index),
            })
            .collect();
        UpdateEvidence {
            source_manifest: None,
            sources,
            bodies,
            initializers: vec![],
            targets,
            selections: vec![],
        }
    }

    fn initializer_evidence(&self) -> UpdateEvidence {
        let mut evidence = self.evidence();
        let declaration = evidence.sources[0].declaration;
        evidence
            .initializers
            .push(StaticInitializer { declaration });
        evidence.sources[0].tokens = vec![SourceToken {
            kind: 1,
            text: "helper".into(),
        }];
        evidence.sources[0].inputs = vec![CanonicalOperand::Declaration(
            self.identity("helper").declaration,
        )];
        evidence.bodies[0].classification = BodyClassification::StaticInitializer(declaration);
        evidence.targets.remove(0);
        evidence
    }

    fn catalog(&self) -> ProofCatalog<'static> {
        ProofCatalog::new(vec![], [], [], [], &self.context.snapshot(), &self.cancel).unwrap()
    }

    fn budget(&self) -> Budget<'_> {
        Budget::new(Limits::default(), &self.cancel).unwrap()
    }
}

fn coverage(evidence: &UpdateEvidence) -> Coverage<'_> {
    Coverage {
        evidence,
        methods: &[],
        constructions: &[],
        layouts: &[],
        natives: &[],
    }
}

#[test]
fn initializer_references_stay_fixed_but_ordinary_callees_can_change() {
    let f = Fixture::new();
    let mut old = f.initializer_evidence();
    old.bodies[1]
        .requirement
        .calls
        .push(CallRequirement::Direct(f.identity("handler")));
    let mut candidate = old.clone();
    candidate.bodies[2].instructions[0].opcode = 2;
    candidate.bodies[1].instructions[0].opcode = 3;
    compare_initializers(&old, &candidate, &mut f.budget()).unwrap();
    candidate.sources[0].inputs[0] = CanonicalOperand::Declaration(f.identity("other").declaration);
    assert!(matches!(
        compare_initializers(&old, &candidate, &mut f.budget()),
        Err(UpdateError::InitializerChange { .. })
    ));
}

#[test]
fn initializer_tokens_remain_frozen_even_when_instructions_are_identical() {
    let f = Fixture::new();
    let mut old = f.initializer_evidence();
    old.sources[0].tokens = vec![
        SourceToken {
            kind: 2,
            text: "1".into(),
        },
        SourceToken {
            kind: 3,
            text: "+".into(),
        },
        SourceToken {
            kind: 2,
            text: "2".into(),
        },
    ];
    let mut candidate = old.clone();
    candidate.sources[0].tokens = vec![SourceToken {
        kind: 2,
        text: "3".into(),
    }];
    assert_eq!(old.bodies, candidate.bodies);
    assert!(compare_initializers(&old, &candidate, &mut f.budget()).is_err());
}

#[test]
fn unknown_calls_do_not_freeze_unrelated_code_inventory_or_placement() {
    let f = Fixture::new();
    let mut old = f.initializer_evidence();
    old.bodies[0].requirement.calls.extend([
        CallRequirement::Unknown(UnknownReason::FunctionValue),
        CallRequirement::Unknown(UnknownReason::InterfaceDispatch),
        CallRequirement::Unknown(UnknownReason::NativeReentry),
    ]);
    let mut candidate = old.clone();
    candidate.targets[0].function = FunctionRef::new(9);
    candidate.bodies[2].instructions[0].opcode = 9;
    candidate.bodies.push(f.body("added", vec![]));
    candidate
        .selections
        .push(CallRequirement::Layout(Ty::Builtin(BuiltinType::I32)));
    compare_initializers(&old, &candidate, &mut f.budget()).unwrap();
}

#[test]
fn missing_unused_body_is_not_an_empty_certificate() {
    let f = Fixture::new();
    let old = f.evidence();
    let sources: Vec<_> = old
        .sources
        .iter()
        .map(|record| record.declaration)
        .collect();
    let bodies: Vec<_> = old
        .bodies
        .iter()
        .map(|record| record.requirement.origin.clone())
        .collect();
    let inventory = Inventory {
        sources: &sources,
        bodies: &bodies,
        targets: &old.targets,
        initializers: &old.initializers,
        selections: &old.selections,
    };
    validate_inventory(&old, &inventory, &f.context.snapshot(), &mut f.budget()).unwrap();
    let mut candidate = old.clone();
    let missing = candidate.bodies.pop().unwrap().requirement.origin;
    candidate.targets.pop();
    assert_eq!(
        validate_inventory(
            &candidate,
            &inventory,
            &f.context.snapshot(),
            &mut f.budget()
        ),
        Err(UpdateError::MissingBody(missing))
    );
}

#[test]
fn old_only_demand_requires_candidate_specialization_and_union_keeps_old_edges() {
    let f = Fixture::new();
    let old = f.evidence();
    let mut candidate = old.clone();
    candidate.bodies[0].requirement.calls.clear();
    candidate.bodies[2].requirement.calls.clear();
    let history = publication_demands(&old, &mut f.budget()).unwrap();
    let next = publication_demands(&candidate, &mut f.budget()).unwrap();
    let union = union_demands(&history, &next, &f.context.snapshot(), &mut f.budget()).unwrap();
    assert_eq!(
        union_demands(&union, &next, &f.context.snapshot(), &mut f.budget()).unwrap(),
        union
    );
    check_coverage(&union, &coverage(&candidate), &f.catalog(), &mut f.budget()).unwrap();
    let missing = candidate.targets.remove(1).application;
    assert_eq!(
        check_coverage(&union, &coverage(&candidate), &f.catalog(), &mut f.budget()),
        Err(UpdateError::MissingTarget(missing))
    );
}

#[test]
fn substitution_closes_old_shared_body_calls_without_compiling() {
    let f = Fixture::new();
    let mut candidate = f.evidence();
    let mut shared = f.body("shared", vec![]);
    let parameter = GenericParam {
        owner: shared.requirement.origin.declaration,
        position: 0,
    };
    shared.requirement.generic = Some(GenericBody {
        parameters: vec![parameter.clone()],
        bounds: vec![],
    });
    let mut called = f.identity("specialization");
    called.arguments.push(parameter.as_type());
    shared
        .requirement
        .calls
        .push(CallRequirement::Direct(called.clone()));
    let shared_origin = shared.requirement.origin.clone();
    candidate.bodies.push(shared);
    let ty = Ty::Builtin(BuiltinType::I32);
    called.arguments = vec![ty.clone()];
    let mut specialization = f.body("specialization", vec![]);
    specialization.requirement.origin = called.clone();
    candidate.targets.push(CurrentTarget {
        application: called.clone(),
        body: called.clone(),
        module: 0,
        function: FunctionRef::new(20),
    });
    candidate.bodies.push(specialization);
    let mut envelope = publication_demands(&candidate, &mut f.budget()).unwrap();
    envelope.applications.push(BodyApplication {
        body: shared_origin,
        arguments: vec![ty],
    });
    check_coverage(
        &envelope,
        &coverage(&candidate),
        &f.catalog(),
        &mut f.budget(),
    )
    .unwrap();
    candidate.targets.pop();
    assert_eq!(
        check_coverage(
            &envelope,
            &coverage(&candidate),
            &f.catalog(),
            &mut f.budget()
        ),
        Err(UpdateError::MissingTarget(called))
    );
}

#[test]
fn bounded_wire_roundtrip_and_correspondence_do_not_depend_on_process_ids() {
    let f = Fixture::new();
    let original = f.initializer_evidence();
    let mut references = vec![];
    original
        .visit_definition_ids(
            &mut |id| {
                references.push(*id);
                Ok(())
            },
            &f.cancel,
        )
        .unwrap();
    let encoding = f.context.snapshot().encode(references).unwrap();
    let record = UpdateEvidenceRecord::encode(&original, &encoding, &mut f.budget()).unwrap();
    let codec = DefaultOptions::new()
        .with_fixint_encoding()
        .with_little_endian()
        .with_limit(64 * 1024 * 1024);
    let bytes = codec.serialize(&record).unwrap();
    let record: UpdateEvidenceRecord = codec.deserialize(&bytes).unwrap();
    let decoding = encoding.table.decode().unwrap();
    let decoded = record.decode(&decoding, &mut f.budget()).unwrap();
    let mut correspondence = DeclarationCorrespondence::new().unwrap();
    let old = correspondence
        .import(&original, [], &f.context.snapshot(), &f.cancel)
        .unwrap();
    let new = correspondence
        .import(&decoded, [], &decoding.table, &f.cancel)
        .unwrap();
    assert_eq!(old.records(), new.records());
    compare_initializers(old.records(), new.records(), &mut f.budget()).unwrap();
    let invalid = u64::MAX.to_le_bytes();
    assert!(codec.deserialize::<UpdateEvidenceRecord>(&invalid).is_err());
}

#[test]
fn dynamic_coverage_includes_each_candidate_receiver_and_never_uses_old_bodies() {
    let f = Fixture::new();
    let mut candidate = f.evidence();
    let interface = NominalTy {
        declaration: f
            .context
            .intern_child(f.root, DefinitionKind::Trait, "Read", 0)
            .unwrap(),
        arguments: vec![],
        associated_types: Default::default(),
    };
    let call = CallRequirement::Interface(Box::new(InterfaceCallContract {
        constructions: vec![],
        normalizations: vec![],
        receiver: None,
        operations: vec![],
        interface: interface.clone(),
        method_slot: 0,
        arguments: vec![],
    }));
    let methods = [
        ConstructionMethod {
            interface: interface.clone(),
            receiver: Ty::Builtin(BuiltinType::I32),
            slot: 0,
            body: candidate.bodies[1].requirement.origin.clone(),
            parameters: vec![],
            arguments: vec![],
        },
        ConstructionMethod {
            interface,
            receiver: Ty::Builtin(BuiltinType::Bool),
            slot: 0,
            body: candidate.bodies[2].requirement.origin.clone(),
            parameters: vec![],
            arguments: vec![],
        },
    ];
    let mut envelope = publication_demands(&candidate, &mut f.budget()).unwrap();
    envelope.requirements.push(call);
    let mut available = coverage(&candidate);
    available.methods = &methods;
    check_coverage(&envelope, &available, &f.catalog(), &mut f.budget()).unwrap();
    let removed = candidate.bodies.pop().unwrap().requirement.origin;
    candidate.targets.pop();
    let mut available = coverage(&candidate);
    available.methods = &methods;
    assert_eq!(
        check_coverage(&envelope, &available, &f.catalog(), &mut f.budget()),
        Err(UpdateError::MissingTarget(removed))
    );
}

#[test]
fn native_callback_absence_is_not_sealed_no_reentry_evidence() {
    let f = Fixture::new();
    let candidate = f.evidence();
    let native = NativeImport {
        receiver_access: None,
        result_adapter: None,
        generic: None,
        instance: f.identity("native"),
        binding: f.identity("binding").declaration,
        signature: Signature {
            params: vec![],
            result: Ty::Builtin(BuiltinType::I32),
        },
        requirements: vec![],
        callables: vec![],
    };
    let mut envelope = publication_demands(&candidate, &mut f.budget()).unwrap();
    envelope
        .requirements
        .push(CallRequirement::Native(Box::new(native.clone())));
    let declaration = NativeDeclaration {
        concrete_result: None,
        declaration: native.instance.declaration,
        function: FnDecl {
            name: "native".into(),
            host_policy: None,
            method_policy: MethodPolicy::default(),
            implementation: CallableImplementation::Native(native.binding),
            generic_params: vec![],
            bounds: vec![],
            params: vec![],
            return_type: native.signature.result.clone(),
        },
        callable_requirements: vec![],
    };
    let catalog = ProofCatalog::new(
        vec![],
        [],
        [],
        [&declaration],
        &f.context.snapshot(),
        &f.cancel,
    )
    .unwrap();
    let unproved = [NativeClosure {
        import: &native,
        callbacks: None,
    }];
    let mut available = coverage(&candidate);
    available.natives = &unproved;
    check_coverage(&envelope, &available, &catalog, &mut f.budget()).unwrap();
    let sealed = [NativeClosure {
        import: &native,
        callbacks: Some(&[]),
    }];
    available.natives = &sealed;
    check_coverage(&envelope, &available, &catalog, &mut f.budget()).unwrap();
    let callbacks = [CallRequirement::Direct(f.identity("absent_callback"))];
    let trusted = [NativeClosure {
        import: &native,
        callbacks: Some(&callbacks),
    }];
    available.natives = &trusted;
    assert_eq!(
        check_coverage(&envelope, &available, &catalog, &mut f.budget()),
        Err(UpdateError::MissingTarget(f.identity("absent_callback")))
    );
}

#[test]
fn limits_and_cancellation_fail_instead_of_truncating() {
    let f = Fixture::new();
    let evidence = f.evidence();
    let mut budget = Budget::new(
        Limits {
            visits: 1,
            ..Limits::default()
        },
        &f.cancel,
    )
    .unwrap();
    assert_eq!(
        compare_initializers(&evidence, &evidence, &mut budget),
        Err(UpdateError::LimitExceeded)
    );
    let mut budget = Budget::new(
        Limits {
            applications: 0,
            ..Limits::default()
        },
        &f.cancel,
    )
    .unwrap();
    assert_eq!(
        publication_demands(&evidence, &mut budget),
        Err(UpdateError::LimitExceeded)
    );
    let encoding = f
        .context
        .snapshot()
        .encode([
            f.identity("hash").declaration,
            f.identity("helper").declaration,
            f.identity("handler").declaration,
        ])
        .unwrap();
    let mut budget = Budget::new(
        Limits {
            bytes: 1,
            ..Limits::default()
        },
        &f.cancel,
    )
    .unwrap();
    assert!(matches!(
        UpdateEvidenceRecord::encode(&evidence, &encoding, &mut budget),
        Err(UpdateError::LimitExceeded)
    ));
    let envelope = publication_demands(&evidence, &mut f.budget()).unwrap();
    let mut budget = Budget::new(
        Limits {
            bytes: 1,
            ..Limits::default()
        },
        &f.cancel,
    )
    .unwrap();
    assert_eq!(
        union_demands(&envelope, &envelope, &f.context.snapshot(), &mut budget),
        Err(UpdateError::LimitExceeded)
    );
    let mut too_wide = evidence.clone();
    too_wide.sources[0].inputs = vec![CanonicalOperand::Local(0); 4097];
    assert!(matches!(
        UpdateEvidenceRecord::encode(&too_wide, &encoding, &mut f.budget()),
        Err(UpdateError::LimitExceeded)
    ));
    let mut distinct = Budget::new(
        Limits {
            applications: 3,
            ..Limits::default()
        },
        &f.cancel,
    )
    .unwrap();
    publication_demands(&evidence, &mut distinct).unwrap();
    publication_demands(&evidence, &mut distinct).unwrap();
    let mut budget = f.budget();
    f.cancel.cancel();
    assert_eq!(
        compare_initializers(&evidence, &evidence, &mut budget),
        Err(UpdateError::Cancelled)
    );
}
