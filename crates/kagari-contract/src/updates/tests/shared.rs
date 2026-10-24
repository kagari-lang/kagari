use super::*;
use crate::{
    callable::{
        construction::evidence::{
            ConstructionRequirement, ConstructionSelection, ConstructionWitness,
        },
        shared::SharedCall,
        witness::OperationWitness,
    },
    types::{InterfaceTable, proofs::implementation::Implementation},
    updates::admission::validate_semantics,
};
use kagari_types::{
    callable::{CallableImplementation, MethodPolicy},
    declaration::{FnDecl, Param, TraitDef, requirement::NativeCallableRequirement},
    ty::GenericBound,
};

struct SharedFixture {
    f: Fixture,
    interface: NominalTy,
    member: DefinitionId,
    contract: TraitDef,
    implementation: InterfaceTable,
}

impl SharedFixture {
    fn new() -> Self {
        let f = Fixture::new();
        let declaration = f
            .context
            .intern_child(f.root, DefinitionKind::Trait, "Read", 0)
            .unwrap();
        let member = f
            .context
            .intern_child(declaration, DefinitionKind::Method, "read", 0)
            .unwrap();
        let interface = NominalTy {
            declaration,
            arguments: vec![],
            associated_types: Default::default(),
        };
        let method = FnDecl {
            name: "read".into(),
            host_policy: None,
            method_policy: MethodPolicy::default(),
            implementation: CallableImplementation::Required,
            generic_params: vec![],
            bounds: vec![],
            params: vec![Param {
                name: "self".into(),
                ty: Ty::SelfType(declaration),
                mutable: false,
            }],
            return_type: Ty::Builtin(BuiltinType::I32),
        };
        let contract = TraitDef {
            name: "Read".into(),
            conversion_adapter: None,
            generic_params: vec![],
            bounds: vec![],
            supertraits: vec![],
            associated_types: vec![],
            associated_consts: vec![],
            methods: vec![method.clone()],
        };
        let implementation_id = f
            .context
            .intern_child(f.root, DefinitionKind::Impl, "ReadForI32", 0)
            .unwrap();
        f.context
            .intern_child(implementation_id, DefinitionKind::Method, "read", 0)
            .unwrap();
        let implementation = InterfaceTable {
            declaration: implementation_id,
            name: "ReadForI32".into(),
            bridge: None,
            generic_params: vec![],
            bounds: vec![],
            for_type: Ty::Builtin(BuiltinType::I32),
            trait_type: Ty::Trait(interface.clone()),
            associated_consts: vec![],
            associated_type_families: vec![],
            methods: vec![FnDecl {
                implementation: CallableImplementation::Script,
                params: vec![Param {
                    name: "self".into(),
                    ty: Ty::Builtin(BuiltinType::I32),
                    mutable: false,
                }],
                ..method
            }],
        };
        Self {
            f,
            interface,
            member,
            contract,
            implementation,
        }
    }

    fn catalog(&self) -> ProofCatalog<'_> {
        ProofCatalog::new(
            vec![Implementation::Interface(&self.implementation)],
            [],
            [(self.interface.declaration, &self.contract)],
            [],
            &self.f.context.snapshot(),
            &self.f.cancel,
        )
        .unwrap()
    }

    fn bound(&self, ty: Ty) -> GenericBound {
        GenericBound {
            ty,
            constraints: vec![self.interface.clone()],
        }
    }

    fn operation(&self, receiver: Ty) -> OperationWitness {
        OperationWitness::Forward(Box::new(NativeCallableRequirement {
            receiver,
            interface: self.interface.clone(),
            member: self.member,
            arguments: vec![],
        }))
    }

    fn callee(&self, constrained: bool) -> BodyRecord {
        let mut body = self.f.body("callee", vec![]);
        let parameter = GenericParam {
            owner: body.requirement.origin.declaration,
            position: 0,
        };
        body.requirement.generic = Some(GenericBody {
            parameters: vec![parameter.clone()],
            bounds: if constrained {
                vec![self.bound(parameter.as_type())]
            } else {
                vec![]
            },
        });
        if constrained {
            body.requirement
                .calls
                .push(CallRequirement::Operation(Box::new(
                    self.operation(parameter.as_type()),
                )));
        }
        body
    }

    fn caller(&self, callee: &BodyRecord, forwarded: bool) -> BodyRecord {
        let mut body = self.f.body("removed_closure", vec![]);
        body.classification = BodyClassification::Closure(self.f.identity("owner"));
        let parameter = GenericParam {
            owner: body.requirement.origin.declaration,
            position: 0,
        };
        let argument = if forwarded {
            parameter.as_type()
        } else {
            Ty::Builtin(BuiltinType::I32)
        };
        if forwarded {
            body.requirement.generic = Some(GenericBody {
                parameters: vec![parameter],
                bounds: vec![self.bound(argument.clone())],
            });
        }
        body.requirement
            .calls
            .push(CallRequirement::Shared(Box::new(SharedCall {
                instance: callee.requirement.origin.clone(),
                implementation: CallableImplementation::Script,
                arguments: vec![argument.clone()],
                signature: callee.signature.clone(),
                receiver_access: None,
                operations: if forwarded {
                    vec![self.operation(argument.clone())]
                } else {
                    vec![]
                },
                constructions: if forwarded {
                    vec![ConstructionWitness {
                        requirement: ConstructionRequirement {
                            receiver: argument,
                            interface: self.interface.clone(),
                        },
                        selection: ConstructionSelection::Forward,
                    }]
                } else {
                    vec![]
                },
            })));
        body
    }

    fn evidence(&self, callee: BodyRecord, caller: Option<BodyRecord>) -> UpdateEvidence {
        let targets = vec![CurrentTarget {
            application: callee.requirement.origin.clone(),
            body: callee.requirement.origin.clone(),
            module: 0,
            function: FunctionRef::new(0),
        }];
        let mut bodies = vec![callee];
        bodies.extend(caller);
        UpdateEvidence {
            source_manifest: None,
            sources: vec![],
            bodies,
            roots: vec![],
            targets,
            selections: vec![],
        }
    }

    fn envelope(&self, old: &UpdateEvidence, candidate: &UpdateEvidence) -> DemandEnvelope {
        union_demands(
            &publication_demands(old, &mut self.f.budget()).unwrap(),
            &publication_demands(candidate, &mut self.f.budget()).unwrap(),
            &self.f.context.snapshot(),
            &mut self.f.budget(),
        )
        .unwrap()
    }
}

#[test]
fn verified_dynamic_interface_allows_empty_targets_but_preserves_known_history() {
    let f = SharedFixture::new();
    let mut candidate = f.f.evidence();
    candidate
        .selections
        .push(CallRequirement::Interface(Box::new(
            InterfaceCallContract {
                interface: f.interface.clone(),
                receiver: None,
                method_slot: 0,
                arguments: vec![],
                operations: vec![],
                constructions: vec![],
                normalizations: vec![],
            },
        )));
    let definitions = f.f.context.snapshot();
    let catalog = ProofCatalog::new(
        vec![],
        [],
        [(f.interface.declaration, &f.contract)],
        [],
        &definitions,
        &f.f.cancel,
    )
    .unwrap();
    validate_semantics(&candidate, [], &catalog, &mut f.f.budget()).unwrap();
    let envelope = publication_demands(&candidate, &mut f.f.budget()).unwrap();
    check_coverage(
        &envelope,
        &coverage(&candidate),
        &catalog,
        &mut f.f.budget(),
    )
    .unwrap();
    assert!(
        expand_requests(
            &envelope,
            &coverage(&candidate),
            &catalog,
            &mut f.f.budget()
        )
        .unwrap()
        .is_empty()
    );

    // Empty dynamic coverage does not erase an independently published target.
    let historical = f.f.body("retained_read", vec![]);
    let target = historical.requirement.origin.clone();
    let mut old = candidate.clone();
    old.targets.push(CurrentTarget {
        application: target.clone(),
        body: target.clone(),
        module: 0,
        function: FunctionRef::new(old.bodies.len()),
    });
    old.bodies.push(historical);
    let history = publication_demands(&old, &mut f.f.budget()).unwrap();
    let catalog = ProofCatalog::new(
        vec![],
        [],
        [(f.interface.declaration, &f.contract)],
        [],
        &f.f.context.snapshot(),
        &f.f.cancel,
    )
    .unwrap();
    assert_eq!(
        check_coverage(&history, &coverage(&candidate), &catalog, &mut f.f.budget()),
        Err(UpdateError::MissingTarget(target.clone()))
    );
    let requests =
        expand_requests(&history, &coverage(&candidate), &catalog, &mut f.f.budget()).unwrap();
    assert_eq!(requests.functions, [target]);
    assert!(requests.interfaces.is_empty());
}

#[test]
fn verified_concrete_interface_still_requires_executable_coverage() {
    let f = SharedFixture::new();
    let mut candidate = f.f.evidence();
    let receiver = Ty::Builtin(BuiltinType::I32);
    let mut caller = f.f.body("read_selected", vec![]);
    let parameter = GenericParam {
        owner: caller.requirement.origin.declaration,
        position: 0,
    };
    caller.requirement.generic = Some(GenericBody {
        parameters: vec![parameter.clone()],
        bounds: vec![f.bound(parameter.as_type())],
    });
    caller
        .requirement
        .calls
        .push(CallRequirement::Interface(Box::new(
            InterfaceCallContract {
                interface: f.interface.clone(),
                receiver: Some(parameter.as_type()),
                method_slot: 0,
                arguments: vec![],
                operations: vec![],
                constructions: vec![],
                normalizations: vec![],
            },
        )));
    let application = BodyApplication {
        body: caller.requirement.origin.clone(),
        arguments: vec![receiver.clone()],
    };
    candidate.bodies.push(caller);
    let catalog = f.catalog();
    validate_semantics(&candidate, [], &catalog, &mut f.f.budget()).unwrap();
    let mut envelope = publication_demands(&candidate, &mut f.f.budget()).unwrap();
    envelope.applications.push(application);
    assert_eq!(
        check_coverage(
            &envelope,
            &coverage(&candidate),
            &catalog,
            &mut f.f.budget()
        ),
        Err(UpdateError::MissingCoverage)
    );
    let requests = expand_requests(
        &envelope,
        &coverage(&candidate),
        &catalog,
        &mut f.f.budget(),
    )
    .unwrap();
    assert_eq!(
        requests.interfaces,
        [InterfaceRequest {
            interface: f.interface.clone(),
            receiver: Some(receiver.clone()),
            method_slot: 0,
            arguments: vec![],
        }]
    );

    let missing = f.f.identity("missing_read_body");
    let methods = [ConstructionMethod {
        interface: f.interface.clone(),
        receiver,
        slot: 0,
        body: missing.clone(),
        parameters: vec![],
        arguments: vec![],
    }];
    let catalog = f.catalog();
    let mut available = coverage(&candidate);
    available.methods = &methods;
    assert_eq!(
        check_coverage(&envelope, &available, &catalog, &mut f.f.budget()),
        Err(UpdateError::MissingBody(missing.clone()))
    );
    let requests = expand_requests(&envelope, &available, &catalog, &mut f.f.budget()).unwrap();
    assert!(requests.interfaces.is_empty());
    assert_eq!(
        requests.shared,
        [BodyApplication {
            body: missing,
            arguments: vec![]
        }]
    );
}

#[test]
fn retained_witnessless_shared_call_rejects_stronger_candidate_after_caller_removal() {
    let f = SharedFixture::new();
    let old_callee = f.callee(false);
    let caller = f.caller(&old_callee, false);
    let old = f.evidence(old_callee.clone(), Some(caller.clone()));
    let unchanged = f.evidence(old_callee, None);
    let catalog = f.catalog();
    validate_semantics(&old, [], &catalog, &mut f.f.budget()).unwrap();
    let envelope = f.envelope(&old, &unchanged);
    check_coverage(
        &envelope,
        &coverage(&unchanged),
        &catalog,
        &mut f.f.budget(),
    )
    .unwrap();

    let candidate = f.evidence(f.callee(true), None);
    assert!(
        candidate
            .bodies
            .iter()
            .all(|body| body.requirement.origin != caller.requirement.origin)
    );
    // Local verification passes: the candidate body's forwarded operation has its
    // new declared bound. The removed historical caller is absent from this check.
    validate_semantics(&candidate, [], &catalog, &mut f.f.budget()).unwrap();
    assert!(
        catalog
            .holds(
                &f.interface,
                &Ty::Builtin(BuiltinType::I32),
                &[],
                &f.f.cancel
            )
            .unwrap()
    );
    let envelope = f.envelope(&old, &candidate);
    // Even though I32 satisfies Read, the historical call supplies neither the new
    // operation nor its constructor capability. Conformance cannot invent witnesses.
    assert_eq!(
        check_coverage(
            &envelope,
            &coverage(&candidate),
            &catalog,
            &mut f.f.budget()
        ),
        Err(UpdateError::InvalidEvidence)
    );
    assert_eq!(
        expand_requests(
            &envelope,
            &coverage(&candidate),
            &catalog,
            &mut f.f.budget()
        ),
        Err(UpdateError::InvalidEvidence)
    );
}

#[test]
fn retained_shared_forwarding_uses_the_historical_caller_scope_and_complete_witnesses() {
    let f = SharedFixture::new();
    let callee = f.callee(true);
    let old = f.evidence(callee.clone(), Some(f.caller(&callee, true)));
    let candidate = f.evidence(callee, None);
    let catalog = f.catalog();
    validate_semantics(&old, [], &catalog, &mut f.f.budget()).unwrap();
    let envelope = f.envelope(&old, &candidate);
    check_coverage(
        &envelope,
        &coverage(&candidate),
        &catalog,
        &mut f.f.budget(),
    )
    .unwrap();
    assert!(
        expand_requests(
            &envelope,
            &coverage(&candidate),
            &catalog,
            &mut f.f.budget()
        )
        .unwrap()
        .is_empty()
    );
    for missing in 0..3 {
        let mut historical = old.clone();
        let caller = &mut historical.bodies[1].requirement;
        let CallRequirement::Shared(call) = &mut caller.calls[0] else {
            panic!()
        };
        match missing {
            0 => call.operations.clear(),
            1 => call.constructions.clear(),
            2 => caller.generic.as_mut().unwrap().bounds.clear(),
            _ => unreachable!(),
        }
        let envelope = f.envelope(&historical, &candidate);
        assert_eq!(
            check_coverage(
                &envelope,
                &coverage(&candidate),
                &catalog,
                &mut f.f.budget()
            ),
            Err(UpdateError::InvalidEvidence)
        );
    }
}

#[test]
fn retained_shared_call_checks_the_current_instantiated_signature() {
    let f = SharedFixture::new();
    let callee = f.callee(false);
    let old = f.evidence(callee.clone(), Some(f.caller(&callee, false)));
    let mut candidate = f.evidence(callee, None);
    candidate.bodies[0].signature.result = Ty::Builtin(BuiltinType::Bool);
    let catalog = f.catalog();
    validate_semantics(&candidate, [], &catalog, &mut f.f.budget()).unwrap();
    let envelope = f.envelope(&old, &candidate);
    assert_eq!(
        check_coverage(
            &envelope,
            &coverage(&candidate),
            &catalog,
            &mut f.f.budget()
        ),
        Err(UpdateError::InvalidEvidence)
    );
}
