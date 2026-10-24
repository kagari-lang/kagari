use super::*;
use crate::{
    callable::{
        construction::evidence::{
            ConstructionRequirement, ConstructionSelection, ConstructionWitness,
        },
        shared::SharedCall,
    },
    types::proofs::ProofCatalog,
    updates::admission::validate_semantics,
};
use kagari_types::{
    callable::{CallableImplementation, MethodPolicy},
    declaration::{FnDecl, NativeDeclaration, TraitDef},
    ty::GenericBound,
};
use std::slice;

#[test]
fn actual_arithmetic_and_call_index_meaning_are_independently_checked() {
    let f = Fixture::new();
    let declared = f.body("body", vec![]);
    assert_eq!(declared.format, declared.actual_format);
    assert_eq!(declared.instructions, declared.actual_instructions);
    validate_executable_edges(&declared, &declared, &mut f.budget()).unwrap();
    let mut extracted = declared.clone();
    extracted.actual_instructions[0].opcode = 2;
    assert_eq!(declared.requirement.calls, extracted.requirement.calls);
    assert_eq!(
        validate_executable_edges(&declared, &extracted, &mut f.budget()),
        Err(UpdateError::InvalidEvidence)
    );
    let mut preserved = declared.clone();
    preserved.instructions[0].opcode = 3;
    validate_executable_edges(&preserved, &declared, &mut f.budget()).unwrap();
    let mut declared = f.body(
        "calls",
        vec![
            CallRequirement::Direct(f.identity("a")),
            CallRequirement::Direct(f.identity("b")),
        ],
    );
    declared.actual_instructions[0].operands = vec![CanonicalOperand::Call(0)];
    let mut extracted = declared.clone();
    extracted.requirement.calls.swap(0, 1);
    assert_eq!(
        validate_executable_edges(&declared, &extracted, &mut f.budget()),
        Err(UpdateError::InvalidEvidence)
    );
    let old = f.initializer_evidence();
    let mut candidate = old.clone();
    candidate.bodies[0].actual_instructions[0].opcode = 99;
    assert_eq!(old.sources, candidate.sources);
    assert!(matches!(
        compare_initializers(&old, &candidate, &mut f.budget()),
        Err(UpdateError::InitializerChange { .. })
    ));
    let mut candidate = old.clone();
    candidate.bodies[0].actual_format += 1;
    assert!(compare_initializers(&old, &candidate, &mut f.budget()).is_err());
}

#[test]
fn one_walker_batches_all_missing_products_then_strictly_revalidates() {
    let f = Fixture::new();
    let mut candidate = f.evidence();
    let function = f.identity("requested");
    let shared = f.identity("shared");
    let implementation = f.identity("implementation");
    let interface = NominalTy {
        declaration: f
            .context
            .intern_child(f.root, DefinitionKind::Trait, "Marker", 0)
            .unwrap(),
        arguments: vec![],
        associated_types: Default::default(),
    };
    let ty = Ty::Builtin(BuiltinType::I32);
    let call = SharedCall {
        constructions: vec![],
        receiver_access: None,
        instance: shared.clone(),
        implementation: CallableImplementation::Script,
        arguments: vec![ty.clone()],
        signature: Signature {
            params: vec![],
            result: ty.clone(),
        },
        operations: vec![],
    };
    let mut envelope = publication_demands(&candidate, &mut f.budget()).unwrap();
    envelope.requirements.extend([
        CallRequirement::Unknown(UnknownReason::FunctionValue),
        CallRequirement::Direct(function.clone()),
        CallRequirement::Direct(function.clone()),
        CallRequirement::Shared(Box::new(call)),
        CallRequirement::Interface(Box::new(InterfaceCallContract {
            constructions: vec![],
            normalizations: vec![],
            receiver: Some(ty.clone()),
            operations: vec![],
            interface: interface.clone(),
            method_slot: 0,
            arguments: vec![],
        })),
        CallRequirement::Construction(Box::new(ConstructionWitness {
            requirement: ConstructionRequirement {
                receiver: ty.clone(),
                interface: interface.clone(),
            },
            selection: ConstructionSelection::Declared {
                implementation: implementation.clone(),
                operations: vec![],
                constructions: vec![],
            },
        })),
        CallRequirement::Layout(ty.clone()),
    ]);
    let mut budget = f.budget();
    let requests =
        expand_requests(&envelope, &coverage(&candidate), &f.catalog(), &mut budget).unwrap();
    assert_eq!(requests.functions, slice::from_ref(&function));
    assert_eq!(
        requests.shared,
        [BodyApplication {
            body: shared.clone(),
            arguments: vec![ty.clone()]
        }]
    );
    assert_eq!(
        requests.interfaces,
        [InterfaceRequest {
            interface: interface.clone(),
            receiver: Some(ty.clone()),
            method_slot: 0,
            arguments: vec![]
        }]
    );
    assert_eq!(requests.constructions, slice::from_ref(&implementation));
    assert_eq!(requests.layouts, slice::from_ref(&ty));
    assert_eq!(
        check_coverage(
            &envelope,
            &coverage(&candidate),
            &f.catalog(),
            &mut f.budget()
        ),
        Err(UpdateError::MissingTarget(function.clone()))
    );
    let imported = DeclarationCorrespondence::new()
        .unwrap()
        .import(&requests, [], &f.context.snapshot(), &f.cancel)
        .unwrap();
    assert_eq!(imported.records().interfaces[0].method_slot, 0);
    assert_eq!(
        imported
            .definitions()
            .resolve(imported.records().functions[0].declaration)
            .unwrap()
            .to_path(),
        f.context
            .snapshot()
            .resolve(function.declaration)
            .unwrap()
            .to_path()
    );
    // Simulate the existing planner supplying the batch; no second demand solver.
    candidate.bodies.push(f.body("requested", vec![]));
    candidate.targets.push(CurrentTarget {
        application: function.clone(),
        body: function.clone(),
        module: 0,
        function: FunctionRef::new(10),
    });
    let mut body = f.body("shared", vec![]);
    body.requirement.generic = Some(GenericBody {
        parameters: vec![GenericParam {
            owner: shared.declaration,
            position: 0,
        }],
        bounds: vec![],
    });
    candidate.bodies.push(body);
    let next = publication_demands(&candidate, &mut budget).unwrap();
    let envelope = union_demands(&envelope, &next, &f.context.snapshot(), &mut budget).unwrap();
    let methods = [ConstructionMethod {
        interface,
        receiver: ty.clone(),
        slot: 0,
        body: function,
        parameters: vec![],
        arguments: vec![],
    }];
    let constructions = [implementation];
    let layouts = [ty];
    let mut available = coverage(&candidate);
    available.methods = &methods;
    available.constructions = &constructions;
    available.layouts = &layouts;
    assert!(
        expand_requests(&envelope, &available, &f.catalog(), &mut budget)
            .unwrap()
            .is_empty()
    );
    check_coverage(&envelope, &available, &f.catalog(), &mut budget).unwrap();
}

#[test]
fn unknown_preserves_uncalled_published_targets_without_freezing_bodies() {
    let f = Fixture::new();
    let mut old = f.evidence();
    old.bodies[2].requirement.calls = vec![CallRequirement::Unknown(UnknownReason::FunctionValue)];
    let history = publication_demands(&old, &mut f.budget()).unwrap();
    check_coverage(&history, &coverage(&old), &f.catalog(), &mut f.budget()).unwrap();
    let mut candidate = old.clone();
    let missing = candidate.targets.pop().unwrap().application;
    assert_eq!(
        check_coverage(
            &history,
            &coverage(&candidate),
            &f.catalog(),
            &mut f.budget()
        ),
        Err(UpdateError::MissingTarget(missing.clone()))
    );
    assert_eq!(
        expand_requests(
            &history,
            &coverage(&candidate),
            &f.catalog(),
            &mut f.budget()
        )
        .unwrap()
        .functions,
        [missing]
    );
    let mut candidate = old.clone();
    candidate.bodies[0].actual_instructions[0].opcode = 99;
    let added = f.body("added", vec![]);
    let added_identity = added.requirement.origin.clone();
    candidate.targets.push(CurrentTarget {
        application: added_identity.clone(),
        body: added_identity.clone(),
        module: 0,
        function: FunctionRef::new(candidate.bodies.len()),
    });
    candidate.bodies.push(added);
    compare_initializers(&old, &candidate, &mut f.budget()).unwrap();
    assert_eq!(
        check_coverage(
            &history,
            &coverage(&candidate),
            &f.catalog(),
            &mut f.budget(),
        ),
        Err(UpdateError::MissingBody(added_identity)),
    );
    let published = publication_demands(&candidate, &mut f.budget()).unwrap();
    let demands =
        union_demands(&history, &published, &f.context.snapshot(), &mut f.budget()).unwrap();
    check_coverage(
        &demands,
        &coverage(&candidate),
        &f.catalog(),
        &mut f.budget(),
    )
    .unwrap();
}

#[test]
fn checked_shared_native_registration_and_unknown_callbacks_are_supported() {
    let f = Fixture::new();
    let mut candidate = f.evidence();
    let identity = f.identity("native");
    let binding = f.identity("native_binding").declaration;
    let parameter = GenericParam {
        owner: identity.declaration,
        position: 0,
    };
    let signature = Signature {
        params: vec![],
        result: Ty::Builtin(BuiltinType::I32),
    };
    let native = NativeImport {
        receiver_access: None,
        result_adapter: None,
        generic: Some(GenericBody {
            parameters: vec![parameter.clone()],
            bounds: vec![],
        }),
        instance: ConcreteFunctionIdentity {
            declaration: identity.declaration,
            arguments: vec![parameter.as_type()],
        },
        binding,
        signature: signature.clone(),
        requirements: vec![],
        callables: vec![],
    };
    let declaration = NativeDeclaration {
        concrete_result: None,
        declaration: identity.declaration,
        function: FnDecl {
            host_policy: None,
            method_policy: MethodPolicy::default(),
            name: "native".into(),
            implementation: CallableImplementation::Native(binding),
            generic_params: vec![parameter],
            bounds: vec![],
            params: vec![],
            return_type: signature.result.clone(),
        },
        callable_requirements: vec![],
    };

    candidate.bodies[2]
        .requirement
        .calls
        .push(CallRequirement::Shared(Box::new(SharedCall {
            constructions: vec![],
            receiver_access: None,
            instance: native.instance.clone(),
            implementation: CallableImplementation::Native(binding),
            arguments: vec![Ty::Builtin(BuiltinType::I32)],
            signature,
            operations: vec![],
        })));
    let definitions = f.context.snapshot();
    let catalog =
        ProofCatalog::new(vec![], [], [], [&declaration], &definitions, &f.cancel).unwrap();
    validate_semantics(&candidate, [&native], &catalog, &mut f.budget()).unwrap();
    assert_eq!(
        validate_semantics(&candidate, [], &catalog, &mut f.budget()),
        Err(UpdateError::MissingNative(native.instance.clone()))
    );
    let mut invalid = native.clone();
    invalid.binding = identity.declaration;
    assert_eq!(
        validate_semantics(&candidate, [&native, &invalid], &catalog, &mut f.budget()),
        Err(UpdateError::InvalidEvidence)
    );
    let envelope = publication_demands(&candidate, &mut f.budget()).unwrap();
    let natives = [NativeClosure {
        import: &native,
        callbacks: None,
    }];
    let mut available = coverage(&candidate);
    available.natives = &natives;
    check_coverage(&envelope, &available, &catalog, &mut f.budget()).unwrap();
    assert!(
        expand_requests(&envelope, &available, &catalog, &mut f.budget())
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        check_coverage(&envelope, &coverage(&candidate), &catalog, &mut f.budget()),
        Err(UpdateError::MissingNative(native.instance.clone()))
    );
    assert_eq!(
        expand_requests(&envelope, &coverage(&candidate), &catalog, &mut f.budget()).unwrap(),
        DemandRequests {
            functions: vec![native.instance.clone()],
            ..Default::default()
        },
    );
    let invalid_natives = [NativeClosure {
        import: &invalid,
        callbacks: None,
    }];
    available.natives = &invalid_natives;
    assert_eq!(
        expand_requests(&envelope, &available, &catalog, &mut f.budget()),
        Err(UpdateError::InvalidEvidence),
    );

    // A retained concrete import must be requested even when current source only
    // calls a different application. Supplying the request never proves coverage.
    let mut concrete = native.clone();
    concrete.generic = None;
    concrete.instance.arguments = vec![Ty::Builtin(BuiltinType::I32)];
    let mut retained = publication_demands(&candidate, &mut f.budget()).unwrap();
    retained
        .requirements
        .push(CallRequirement::Native(Box::new(concrete.clone())));
    let mut available = coverage(&candidate);
    available.natives = &natives;
    assert_eq!(
        check_coverage(&retained, &available, &catalog, &mut f.budget()),
        Err(UpdateError::MissingNative(concrete.instance.clone())),
    );
    assert_eq!(
        expand_requests(&retained, &available, &catalog, &mut f.budget()).unwrap(),
        DemandRequests {
            functions: vec![concrete.instance.clone()],
            ..Default::default()
        },
    );
    let supplied = [
        NativeClosure {
            import: &native,
            callbacks: None,
        },
        NativeClosure {
            import: &concrete,
            callbacks: None,
        },
    ];
    available.natives = &supplied;
    check_coverage(&retained, &available, &catalog, &mut f.budget()).unwrap();
    assert!(
        expand_requests(&retained, &available, &catalog, &mut f.budget())
            .unwrap()
            .is_empty()
    );
}

#[test]
fn symbolic_forwarding_uses_verified_binder_authority_not_an_empty_dependency_claim() {
    let f = Fixture::new();
    let mut candidate = f.evidence();
    let mut body = f.body("erase", vec![]);
    let parameter = GenericParam {
        owner: body.requirement.origin.declaration,
        position: 0,
    };
    let interface = NominalTy {
        declaration: f
            .context
            .intern_child(f.root, DefinitionKind::Trait, "Tag", 0)
            .unwrap(),
        arguments: vec![],
        associated_types: Default::default(),
    };
    let bound = GenericBound {
        ty: parameter.as_type(),
        constraints: vec![interface.clone()],
    };
    body.requirement.generic = Some(GenericBody {
        parameters: vec![parameter.clone()],
        bounds: vec![bound],
    });
    body.requirement
        .calls
        .push(CallRequirement::Construction(Box::new(
            ConstructionWitness {
                requirement: ConstructionRequirement {
                    receiver: parameter.as_type(),
                    interface: interface.clone(),
                },
                selection: ConstructionSelection::Forward,
            },
        )));
    candidate.bodies.push(body);
    let contract = TraitDef {
        name: "Tag".into(),
        conversion_adapter: None,
        generic_params: vec![],
        bounds: vec![],
        supertraits: vec![],
        associated_types: vec![],
        associated_consts: vec![],
        methods: vec![],
    };
    let definitions = f.context.snapshot();
    let catalog = ProofCatalog::new(
        vec![],
        [],
        [(interface.declaration, &contract)],
        [],
        &definitions,
        &f.cancel,
    )
    .unwrap();
    validate_semantics(&candidate, [], &catalog, &mut f.budget()).unwrap();
    let envelope = publication_demands(&candidate, &mut f.budget()).unwrap();
    check_coverage(&envelope, &coverage(&candidate), &catalog, &mut f.budget()).unwrap();
    candidate
        .bodies
        .last_mut()
        .unwrap()
        .requirement
        .generic
        .as_mut()
        .unwrap()
        .bounds
        .clear();
    assert_eq!(
        validate_semantics(&candidate, [], &catalog, &mut f.budget()),
        Err(UpdateError::InvalidEvidence)
    );
}

#[test]
fn request_expansion_observes_bytes_work_and_cancellation() {
    let f = Fixture::new();
    let candidate = f.evidence();
    let mut envelope = publication_demands(&candidate, &mut f.budget()).unwrap();
    envelope
        .requirements
        .push(CallRequirement::Direct(f.identity("missing")));
    let mut budget = Budget::new(
        Limits {
            bytes: 0,
            ..Limits::default()
        },
        &f.cancel,
    )
    .unwrap();
    assert_eq!(
        expand_requests(&envelope, &coverage(&candidate), &f.catalog(), &mut budget),
        Err(UpdateError::LimitExceeded)
    );
    let mut budget = Budget::new(
        Limits {
            visits: 0,
            ..Limits::default()
        },
        &f.cancel,
    )
    .unwrap();
    assert_eq!(
        expand_requests(&envelope, &coverage(&candidate), &f.catalog(), &mut budget),
        Err(UpdateError::LimitExceeded)
    );
    let catalog = f.catalog();
    let mut budget = f.budget();
    f.cancel.cancel();
    assert_eq!(
        expand_requests(&envelope, &coverage(&candidate), &catalog, &mut budget),
        Err(UpdateError::Cancelled)
    );
}
