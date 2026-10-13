use super::*;
use kagari_common::identity::{ModuleIdentity, map::DefinitionContext, table::DefinitionId};
use kagari_types::{
    callable::{CallableImplementation, MethodPolicy},
    declaration::{AssociatedTypeDef, FnDecl, Param},
    scalar::BuiltinType,
};
use std::{collections::BTreeMap, slice};

fn id(context: &DefinitionContext, module: &str, kind: DefinitionKind, name: &str) -> DefinitionId {
    let root = context
        .intern_root(&ModuleIdentity::single_file(module))
        .unwrap();
    context.intern_child(root, kind, name, 0).unwrap()
}

fn fixture(context: &DefinitionContext) -> (InterfaceCallContract, TraitDef) {
    let owner = id(
        context,
        "generic-interface.kgr",
        DefinitionKind::Trait,
        "Map",
    );
    let item = GenericParam { owner, position: 0 };
    let key = GenericParam {
        owner: context
            .intern_child(owner, DefinitionKind::Method, "map", 0)
            .unwrap(),
        position: 0,
    };
    let contract = TraitDef {
        conversion_adapter: None,
        name: "Map".into(),
        generic_params: vec![item.clone()],
        bounds: vec![],
        supertraits: vec![],
        associated_types: vec![],
        associated_consts: vec![],
        methods: vec![FnDecl {
            name: "map".into(),
            implementation: CallableImplementation::Required,
            method_policy: MethodPolicy::default(),
            generic_params: vec![key.clone()],
            bounds: vec![],
            params: vec![
                Param {
                    name: "self".into(),
                    ty: Ty::SelfType(owner),
                    mutable: false,
                },
                Param {
                    name: "f".into(),
                    ty: Ty::Function {
                        params: vec![item.as_type()],
                        result: Box::new(key.as_type()),
                    },
                    mutable: false,
                },
            ],
            return_type: Ty::Tuple(vec![item.as_type(), key.as_type()]),
        }],
    };
    let call = InterfaceCallContract {
        normalizations: vec![],
        receiver: None,
        operations: vec![],
        interface: NominalTy {
            declaration: owner,
            arguments: vec![Ty::Builtin(BuiltinType::I32)],
            associated_types: BTreeMap::new(),
        },
        method_slot: 0,
        arguments: vec![Ty::Builtin(BuiltinType::String)],
    };
    (call, contract)
}

#[test]
fn interface_application_substitutes_both_binders_inside_callbacks_and_results() {
    let context = DefinitionContext::new().unwrap();
    let (mut call, contract) = fixture(&context);
    let cancel = CancellationToken::default();
    let catalog =
        ProofCatalog::new(vec![], vec![], [], [], [], &context.snapshot(), &cancel).unwrap();
    for key in [
        Ty::Builtin(BuiltinType::I32),
        Ty::Tuple(vec![Ty::Builtin(BuiltinType::String)]),
    ] {
        call.arguments = vec![key.clone()];
        let signature = call
            .signature(&contract, &cancel, &context.snapshot())
            .unwrap();
        assert_eq!(
            signature.params[1],
            Ty::Function {
                params: vec![Ty::Builtin(BuiltinType::I32)],
                result: Box::new(key.clone()),
            }
        );
        assert_eq!(
            signature.result,
            Ty::Tuple(vec![Ty::Builtin(BuiltinType::I32), key])
        );
        assert!(call.check(&contract, &catalog, &[], &[], &cancel).unwrap());
    }
}

#[test]
fn forwarded_parameters_require_the_callers_scope_and_bound_evidence() {
    let context = DefinitionContext::new().unwrap();
    let (mut call, mut contract) = fixture(&context);
    let cancel = CancellationToken::default();
    let marker = NominalTy {
        declaration: id(
            &context,
            "generic-interface.kgr",
            DefinitionKind::Trait,
            "Marker",
        ),
        arguments: vec![],
        associated_types: BTreeMap::new(),
    };
    let key = contract.methods[0].generic_params[0].as_type();
    contract.methods[0].bounds.push(GenericBound {
        ty: key,
        constraints: vec![Constraint::Trait(marker.clone())],
    });
    let parameter = GenericParam {
        owner: id(&context, "caller.kgr", DefinitionKind::Function, "forward"),
        position: 0,
    };
    call.arguments = vec![parameter.as_type()];
    let evidence = GenericBound {
        ty: parameter.as_type(),
        constraints: vec![Constraint::Trait(marker.clone())],
    };
    let marker_contract = TraitDef {
        conversion_adapter: None,
        name: "Marker".into(),
        generic_params: vec![],
        bounds: vec![],
        supertraits: vec![],
        associated_types: vec![],
        associated_consts: vec![],
        methods: vec![],
    };
    let catalog = ProofCatalog::new(
        vec![],
        vec![],
        [],
        [(marker.declaration, &marker_contract)],
        [],
        &context.snapshot(),
        &cancel,
    )
    .unwrap();
    assert!(
        !call
            .check(
                &contract,
                &catalog,
                &[],
                slice::from_ref(&evidence),
                &cancel
            )
            .unwrap()
    );
    assert!(
        !call
            .check(
                &contract,
                &catalog,
                slice::from_ref(&parameter),
                &[],
                &cancel
            )
            .unwrap()
    );
    assert!(
        call.check(&contract, &catalog, &[parameter], &[evidence], &cancel)
            .unwrap()
    );
}

#[test]
fn malformed_applications_and_erased_self_positions_are_rejected() {
    let context = DefinitionContext::new().unwrap();
    let (call, contract) = fixture(&context);
    let cancel = CancellationToken::default();
    for mutation in 0..4 {
        let mut bad = call.clone();
        match mutation {
            0 => bad.arguments.clear(),
            1 => bad.interface.arguments.clear(),
            2 => bad.method_slot = 9,
            _ => {
                bad.interface.declaration = id(
                    &context,
                    "generic-interface.kgr",
                    DefinitionKind::Trait,
                    "Other",
                )
            }
        }
        assert!(
            bad.signature(&contract, &cancel, &context.snapshot())
                .is_err()
        );
    }
    let mut bad = contract.clone();
    bad.methods[0].return_type = Ty::SelfType(call.interface.declaration);
    let catalog =
        ProofCatalog::new(vec![], vec![], [], [], [], &context.snapshot(), &cancel).unwrap();
    assert!(!call.check(&bad, &catalog, &[], &[], &cancel).unwrap());
}

#[test]
fn partial_associated_outputs_require_scoped_receiver_evidence() {
    let context = DefinitionContext::new().unwrap();
    let (mut call, mut contract) = fixture(&context);
    let cancel = CancellationToken::default();
    let residual = context
        .intern_child(
            call.interface.declaration,
            DefinitionKind::AssociatedType,
            "Residual",
            0,
        )
        .unwrap();
    contract.associated_types.push(AssociatedTypeDef {
        declaration: residual,
        generic_params: vec![],
        parameter_bounds: vec![],
        bounds: vec![],
    });
    contract.methods[0].return_type = Ty::Projection {
        receiver: Box::new(Ty::SelfType(call.interface.declaration)),
        interface: Box::new(call.interface.clone()),
        member: residual,
        arguments: vec![],
    };
    assert!(
        call.signature(&contract, &cancel, &context.snapshot())
            .is_err()
    );
    let parameter = GenericParam {
        owner: id(&context, "caller.kgr", DefinitionKind::Function, "forward"),
        position: 0,
    };
    call.receiver = Some(parameter.as_type());
    let signature = call
        .signature(&contract, &cancel, &context.snapshot())
        .unwrap();
    assert_eq!(
        signature.result,
        Ty::Projection {
            receiver: Box::new(parameter.as_type()),
            interface: Box::new(call.interface.clone()),
            member: residual,
            arguments: vec![],
        }
    );
    let evidence = GenericBound {
        ty: parameter.as_type(),
        constraints: vec![Constraint::Trait(call.interface.clone())],
    };
    let catalog = ProofCatalog::new(
        vec![],
        vec![],
        [],
        [(call.interface.declaration, &contract)],
        [],
        &context.snapshot(),
        &cancel,
    )
    .unwrap();
    assert!(
        call.check(
            &contract,
            &catalog,
            slice::from_ref(&parameter),
            slice::from_ref(&evidence),
            &cancel
        )
        .unwrap()
    );
    assert!(
        !call
            .check(
                &contract,
                &catalog,
                &[],
                slice::from_ref(&evidence),
                &cancel
            )
            .unwrap()
    );
    assert!(
        !call
            .check(
                &contract,
                &catalog,
                slice::from_ref(&parameter),
                &[],
                &cancel
            )
            .unwrap()
    );
    call.interface.associated_types.insert(
        id(
            &context,
            "forged.kgr",
            DefinitionKind::AssociatedType,
            "Residual",
        ),
        Ty::Builtin(BuiltinType::I32),
    );
    assert!(
        call.signature(&contract, &cancel, &context.snapshot())
            .is_err()
    );
}

#[test]
fn static_constraint_calls_do_not_invent_a_receiver_argument() {
    let context = DefinitionContext::new().unwrap();
    let (mut call, mut contract) = fixture(&context);
    let cancel = CancellationToken::default();
    contract.methods[0].params.remove(0);
    contract.methods[0].return_type = Ty::SelfType(call.interface.declaration);
    assert!(
        call.signature(&contract, &cancel, &context.snapshot())
            .is_err()
    );
    let parameter = GenericParam {
        owner: id(&context, "caller.kgr", DefinitionKind::Function, "forward"),
        position: 0,
    };
    call.receiver = Some(parameter.as_type());
    let signature = call
        .signature(&contract, &cancel, &context.snapshot())
        .unwrap();
    assert_eq!(signature.params.len(), 1);
    assert_eq!(signature.result, parameter.as_type());
    let evidence = GenericBound {
        ty: parameter.as_type(),
        constraints: vec![Constraint::Trait(call.interface.clone())],
    };
    let catalog = ProofCatalog::new(
        vec![],
        vec![],
        [],
        [(call.interface.declaration, &contract)],
        [],
        &context.snapshot(),
        &cancel,
    )
    .unwrap();
    assert!(
        call.check(
            &contract,
            &catalog,
            slice::from_ref(&parameter),
            slice::from_ref(&evidence),
            &cancel
        )
        .unwrap()
    );
    assert!(
        !call
            .check(&contract, &catalog, &[parameter], &[], &cancel)
            .unwrap()
    );
}

#[test]
fn supplied_normalization_cannot_override_a_declared_signature() {
    let context = DefinitionContext::new().unwrap();
    let (mut call, contract) = fixture(&context);
    let cancel = CancellationToken::default();
    let source = call
        .signature(&contract, &cancel, &context.snapshot())
        .unwrap()
        .result;
    let forged = Ty::Tuple(vec![
        Ty::Builtin(BuiltinType::Bool),
        Ty::Builtin(BuiltinType::String),
    ]);
    call.normalizations.push(TypeNormalization {
        source,
        result: forged.clone(),
    });
    // Structural use can read the supplied physical signature, but independent
    // linked proof must reject the invented equality before execution.
    assert_eq!(
        call.signature(&contract, &cancel, &context.snapshot())
            .unwrap()
            .result,
        forged
    );
    let catalog =
        ProofCatalog::new(vec![], vec![], [], [], [], &context.snapshot(), &cancel).unwrap();
    assert!(!call.check(&contract, &catalog, &[], &[], &cancel).unwrap());
}

#[test]
fn dynamic_admission_checks_uncalled_members_and_call_constraints() {
    let context = DefinitionContext::new().unwrap();
    let (call, contract) = fixture(&context);
    let cancel = CancellationToken::default();
    for mutation in 0..5 {
        let mut changed = contract.clone();
        let self_type = Ty::SelfType(call.interface.declaration);
        let key = changed.methods[0].generic_params[0].as_type();
        match mutation {
            0 => changed.methods[0].params[1].ty = Ty::Array(Box::new(self_type)),
            1 => changed.methods[0].bounds.push(GenericBound {
                ty: key,
                constraints: vec![Constraint::Trait(NominalTy {
                    declaration: call.interface.declaration,
                    arguments: vec![self_type],
                    associated_types: BTreeMap::new(),
                })],
            }),
            2 => {
                let mut other = changed.methods[0].clone();
                other.name = "static_member".into();
                other.generic_params.clear();
                other.params.clear();
                other.return_type = Ty::Builtin(BuiltinType::I32);
                context
                    .intern_child(
                        call.interface.declaration,
                        DefinitionKind::Method,
                        &other.name,
                        0,
                    )
                    .unwrap();
                changed.methods.push(other);
            }
            3 => changed.methods[0].bounds.push(GenericBound {
                ty: self_type,
                constraints: vec![Constraint::Trait(call.interface.clone())],
            }),
            4 => {
                changed.methods.clear();
                changed.bounds.push(GenericBound {
                    ty: changed.generic_params[0].as_type(),
                    constraints: vec![Constraint::Trait(NominalTy {
                        declaration: call.interface.declaration,
                        arguments: vec![self_type],
                        associated_types: BTreeMap::new(),
                    })],
                });
            }
            _ => unreachable!(),
        }
        let valid = dynamic_interface_valid(
            &[],
            &call.interface,
            &|id| (*id == call.interface.declaration).then_some(&changed),
            &context.snapshot(),
            &cancel,
        );
        assert_eq!(valid.unwrap_or(false), mutation == 3, "mutation {mutation}");
    }
}
