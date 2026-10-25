use crate::{
    callable::generic::GenericBody,
    slots::SemanticSlots,
    types::{
        ConcreteFunctionIdentity,
        named::{
            FunctionOrigin, ImplementationContract, NamedContract, NamedDeclaration, origins,
            verify::{SignatureScope, signature_matches},
        },
        proofs::ProofCatalog,
    },
};
use kagari_common::identity::{
    DefinitionKind, ModuleIdentity, map::DefinitionContext, table::DefinitionId,
};
use kagari_types::{
    callable::CallableImplementation,
    declaration::{FnDecl, Param},
    scalar::BuiltinType,
    ty::{GenericBound, GenericParam, NominalTy, Ty},
    visibility::Visibility,
};

fn bound(ty: Ty, declaration: DefinitionId) -> GenericBound {
    GenericBound {
        ty,
        constraints: vec![NominalTy {
            declaration,
            arguments: vec![],
            associated_types: Default::default(),
        }],
    }
}

fn function(
    declaration: DefinitionId,
    name: &str,
    parameters: Vec<GenericParam>,
    bounds: Vec<GenericBound>,
) -> NamedDeclaration {
    NamedDeclaration {
        declaration,
        visibility: Visibility::Private,
        ambiguous_local_body: None,
        contract: NamedContract::Function(FnDecl {
            name: name.into(),
            implementation: CallableImplementation::Script,
            host_policy: None,
            method_policy: Default::default(),
            params: vec![Param {
                name: "inputs".into(),
                ty: Ty::Tuple(parameters.iter().map(GenericParam::as_type).collect()),
                mutable: false,
            }],
            return_type: Ty::Builtin(BuiltinType::I32),
            generic_params: parameters,
            bounds,
        }),
    }
}

struct Fixture {
    context: DefinitionContext,
    records: Vec<NamedDeclaration>,
    identity: ConcreteFunctionIdentity,
    semantic: SemanticSlots,
}

fn fixture(nested: bool) -> Fixture {
    let context = DefinitionContext::new().unwrap();
    let root = context
        .intern_root(&ModuleIdentity::single_file("authored-bound-scopes"))
        .unwrap();
    let implementation = context
        .intern_child(root, DefinitionKind::Impl, "", 0)
        .unwrap();
    let method = context
        .intern_child(implementation, DefinitionKind::Method, "method", 0)
        .unwrap();
    let local = context
        .intern_child(method, DefinitionKind::Function, "local", 0)
        .unwrap();
    let read = context
        .intern_child(root, DefinitionKind::Trait, "Read", 0)
        .unwrap();
    let write = context
        .intern_child(root, DefinitionKind::Trait, "Write", 0)
        .unwrap();
    let receiver_parameter = GenericParam {
        owner: implementation,
        position: 0,
    };
    let method_parameter = GenericParam {
        owner: method,
        position: 0,
    };
    let local_parameter = GenericParam {
        owner: local,
        position: 0,
    };
    let mut records = vec![
        NamedDeclaration {
            declaration: implementation,
            visibility: Visibility::Private,
            ambiguous_local_body: None,
            contract: NamedContract::Implementation(ImplementationContract {
                bridge: None,
                generic_params: vec![receiver_parameter.clone()],
                receiver: Ty::Tuple(vec![receiver_parameter.as_type()]),
                trait_type: None,
                bounds: vec![bound(receiver_parameter.as_type(), read)],
            }),
        },
        function(
            method,
            "method",
            vec![receiver_parameter.clone(), method_parameter.clone()],
            vec![bound(method_parameter.as_type(), write)],
        ),
    ];
    let mut identity = ConcreteFunctionIdentity {
        declaration: method,
        arguments: vec![Ty::Builtin(BuiltinType::I64)],
    };
    let mut arguments = vec![Ty::Builtin(BuiltinType::I64), method_parameter.as_type()];
    let mut parameters = vec![method_parameter.clone()];
    let mut bounds = vec![
        bound(Ty::Builtin(BuiltinType::I64), read),
        bound(method_parameter.as_type(), write),
    ];
    if nested {
        records.push(function(
            local,
            "local",
            vec![
                receiver_parameter,
                method_parameter,
                local_parameter.clone(),
            ],
            vec![bound(local_parameter.as_type(), read)],
        ));
        identity = ConcreteFunctionIdentity {
            declaration: local,
            arguments: vec![
                Ty::Builtin(BuiltinType::I64),
                Ty::Builtin(BuiltinType::Bool),
            ],
        };
        arguments = vec![
            Ty::Builtin(BuiltinType::I64),
            Ty::Builtin(BuiltinType::Bool),
            local_parameter.as_type(),
        ];
        parameters = vec![local_parameter.clone()];
        bounds = vec![
            bound(Ty::Builtin(BuiltinType::I64), read),
            bound(Ty::Builtin(BuiltinType::Bool), write),
            bound(local_parameter.as_type(), read),
        ];
    }
    let definitions = context.snapshot();
    let bounds = GenericBound::normalize(bounds, &definitions, &Default::default()).unwrap();
    let semantic = SemanticSlots {
        generic: Some(GenericBody { parameters, bounds }),
        params: [(0, Ty::Tuple(arguments))].into(),
        result: Some(Ty::Builtin(BuiltinType::I32)),
        ..Default::default()
    };
    assert!(semantic.types_valid(Some(&identity), &Default::default(), &definitions));
    Fixture {
        context,
        records,
        identity,
        semantic,
    }
}

#[test]
fn executable_bounds_use_exact_ancestor_contracts_and_specialized_binders() {
    for nested in [false, true] {
        let fixture = fixture(nested);
        let root = fixture
            .context
            .intern_root(&ModuleIdentity::single_file("authored-bound-scopes"))
            .unwrap();
        let unrelated_impl = fixture
            .context
            .intern_child(root, DefinitionKind::Impl, "", 1)
            .unwrap();
        let definitions = fixture.context.snapshot();
        let matches = |records: &[NamedDeclaration], semantic: &SemanticSlots| {
            signature_matches(
                SignatureScope::Local,
                records,
                &fixture.identity,
                semantic,
                &definitions,
                &Default::default(),
            )
        };
        assert!(matches(&fixture.records, &fixture.semantic));
        let mut simplified = fixture.semantic.clone();
        simplified.generic.as_mut().unwrap().bounds.reverse();
        simplified.generic.as_mut().unwrap().bounds.pop();
        assert!(matches(&fixture.records, &simplified));
        assert!(!matches(&fixture.records[1..], &fixture.semantic));
        let mut forged = fixture.semantic.clone();
        forged
            .generic
            .as_mut()
            .unwrap()
            .bounds
            .iter_mut()
            .find(|bound| bound.ty == Ty::Builtin(BuiltinType::I64))
            .unwrap()
            .ty = Ty::Builtin(BuiltinType::U32);
        assert!(!matches(&fixture.records, &forged));
        let mut forged = fixture.semantic.clone();
        forged
            .generic
            .as_mut()
            .unwrap()
            .bounds
            .iter_mut()
            .find(|bound| {
                bound.ty == fixture.semantic.generic.as_ref().unwrap().parameters[0].as_type()
            })
            .unwrap()
            .ty = Ty::Parameter {
            owner: fixture.identity.declaration,
            position: 1,
        };
        assert!(!matches(&fixture.records, &forged));
        let mut unrelated = fixture.records.clone();
        unrelated[0].declaration = unrelated_impl;
        let NamedContract::Implementation(header) = &mut unrelated[0].contract else {
            unreachable!()
        };
        header.generic_params[0].owner = unrelated_impl;
        header.receiver = Ty::Tuple(vec![header.generic_params[0].as_type()]);
        header.bounds[0].ty = Ty::Builtin(BuiltinType::I64);
        assert!(!matches(&unrelated, &fixture.semantic));
        if nested {
            let mut missing_outer = fixture.records.clone();
            missing_outer.remove(1);
            assert!(!matches(&missing_outer, &fixture.semantic));
        }
    }
}

#[test]
fn imported_local_signature_resolves_authored_ancestors_not_placement_scope() {
    let fixture = fixture(true);
    let module = ModuleIdentity::single_file("placement");
    let root = fixture.context.intern_root(&module).unwrap();
    let placement = fixture
        .context
        .intern_child(root, DefinitionKind::Function, "local", 0)
        .unwrap();
    let identity = ConcreteFunctionIdentity {
        declaration: placement,
        arguments: fixture.identity.arguments.clone(),
    };
    let parameter = GenericParam {
        owner: placement,
        position: 0,
    };
    let mut semantic = fixture.semantic;
    let authored_parameter = semantic.generic.as_ref().unwrap().parameters[0].as_type();
    semantic.generic.as_mut().unwrap().parameters = vec![parameter.clone()];
    semantic
        .generic
        .as_mut()
        .unwrap()
        .bounds
        .iter_mut()
        .find(|bound| bound.ty == authored_parameter)
        .unwrap()
        .ty = parameter.as_type();
    let Ty::Tuple(arguments) = semantic.params.get_mut(&0).unwrap() else {
        unreachable!()
    };
    *arguments.last_mut().unwrap() = parameter.as_type();
    let definitions = fixture.context.snapshot();
    let body = semantic.generic.as_mut().unwrap();
    // Relocation changes binder identity, so restore the executable bound ordering.
    body.bounds = GenericBound::normalize(
        std::mem::take(&mut body.bounds),
        &definitions,
        &Default::default(),
    )
    .unwrap();
    assert_eq!(body.bounds.len(), 3);
    assert!(semantic.types_valid(Some(&identity), &Default::default(), &definitions));
    let origin = FunctionOrigin {
        placement,
        authored: fixture.identity.declaration,
    };
    let catalog = ProofCatalog::new(vec![], [], [], [], &definitions, &Default::default()).unwrap();
    let validate = |records: &[NamedDeclaration]| {
        origins::validate(
            &catalog,
            std::slice::from_ref(&origin),
            &module,
            &definitions,
            &Default::default(),
            [(&identity, &semantic)],
            |id| records.iter().find(|record| record.declaration == id),
        )
    };
    assert!(validate(&fixture.records).is_ok());
    assert!(validate(&fixture.records[1..]).is_err());
}
