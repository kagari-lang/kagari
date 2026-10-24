use crate::{
    callable::generic::GenericBody,
    slots::SemanticSlots,
    types::{
        ConcreteFunctionIdentity,
        named::{NamedContract, NamedDeclaration, verify::signature_matches},
    },
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{DefinitionKind, ModuleIdentity, map::DefinitionContext},
};
use kagari_types::{
    callable::CallableImplementation,
    declaration::{FnDecl, Param},
    scalar::BuiltinType,
    ty::{GenericBound, GenericParam, NominalTy, Ty},
    visibility::Visibility,
};
use std::slice;

#[test]
fn executable_bounds_cannot_add_an_unauthored_witness_requirement() {
    let context = DefinitionContext::new().unwrap();
    let root = context
        .intern_root(&ModuleIdentity::single_file("bound-forgery"))
        .unwrap();
    let function = context
        .intern_child(root, DefinitionKind::Function, "f", 0)
        .unwrap();
    let read = context
        .intern_child(root, DefinitionKind::Trait, "Read", 0)
        .unwrap();
    let parameter = GenericParam {
        owner: function,
        position: 0,
    };
    let mut record = NamedDeclaration {
        declaration: function,
        visibility: Visibility::Private,
        ambiguous_local_body: None,
        contract: NamedContract::Function(FnDecl {
            name: "f".into(),
            implementation: CallableImplementation::Script,
            host_policy: None,
            method_policy: Default::default(),
            generic_params: vec![parameter.clone()],
            bounds: vec![],
            params: vec![Param {
                name: "x".into(),
                ty: parameter.as_type(),
                mutable: false,
            }],
            return_type: Ty::Builtin(BuiltinType::I32),
        }),
    };
    let identity = ConcreteFunctionIdentity {
        declaration: function,
        arguments: vec![],
    };
    let mut semantic = SemanticSlots {
        generic: Some(GenericBody {
            parameters: vec![parameter.clone()],
            bounds: vec![],
        }),
        params: [(0, parameter.as_type())].into(),
        result: Some(Ty::Builtin(BuiltinType::I32)),
        ..Default::default()
    };
    assert!(signature_matches(
        slice::from_ref(&record),
        &identity,
        &semantic,
        &context.snapshot(),
        &Default::default()
    ));
    semantic
        .generic
        .as_mut()
        .unwrap()
        .bounds
        .push(GenericBound {
            ty: parameter.as_type(),
            constraints: vec![NominalTy {
                declaration: read,
                arguments: vec![],
                associated_types: Default::default(),
            }],
        });
    assert!(semantic.types_valid(Some(&identity), &Default::default(), &context.snapshot()));
    assert!(!signature_matches(
        slice::from_ref(&record),
        &identity,
        &semantic,
        &context.snapshot(),
        &Default::default()
    ));

    let NamedContract::Function(function) = &mut record.contract else {
        unreachable!()
    };
    function.bounds = semantic.generic.as_ref().unwrap().bounds.clone();
    let output = context
        .intern_child(read, DefinitionKind::AssociatedType, "Output", 0)
        .unwrap();
    function.bounds[0].constraints[0]
        .associated_types
        .insert(output, Ty::Builtin(BuiltinType::I32));
    let definitions = context.snapshot();
    let matches = |semantic: &SemanticSlots| {
        signature_matches(
            slice::from_ref(&record),
            &identity,
            semantic,
            &definitions,
            &Default::default(),
        )
    };
    // Requiring only Read is weaker than the authored Read<Output = i32>.
    assert!(matches(&semantic));
    let bound = semantic.generic.as_ref().unwrap().bounds[0].clone();
    semantic.generic.as_mut().unwrap().bounds.push(bound);
    assert!(matches(&semantic));
    semantic.generic.as_mut().unwrap().bounds[0].constraints[0]
        .associated_types
        .insert(output, Ty::Builtin(BuiltinType::Bool));
    assert!(!matches(&semantic));
    semantic.generic.as_mut().unwrap().bounds.clear();
    assert!(matches(&semantic));
    let cancel = CancellationToken::default();
    cancel.cancel();
    assert!(!signature_matches(
        slice::from_ref(&record),
        &identity,
        &semantic,
        &definitions,
        &cancel
    ));
}

#[test]
fn signature_substitution_combines_specialized_receivers_and_shared_method_binders() {
    let context = DefinitionContext::new().unwrap();
    let module = context
        .intern_root(&ModuleIdentity::single_file("named-signatures"))
        .unwrap();
    let owner = context
        .intern_child(module, DefinitionKind::Impl, "", 0)
        .unwrap();
    let method = context
        .intern_child(owner, DefinitionKind::Method, "read", 0)
        .unwrap();
    let structure = context
        .intern_child(module, DefinitionKind::Struct, "Box", 0)
        .unwrap();
    let receiver_parameter = GenericParam { owner, position: 0 };
    let first = GenericParam {
        owner: method,
        position: 0,
    };
    let second = GenericParam {
        owner: method,
        position: 1,
    };
    let receiver = |ty| {
        Ty::Struct(NominalTy {
            declaration: structure,
            arguments: vec![ty],
            associated_types: Default::default(),
        })
    };
    let record = NamedDeclaration {
        declaration: method,
        visibility: Visibility::Private,
        ambiguous_local_body: None,
        contract: NamedContract::Function(FnDecl {
            name: "read".into(),
            implementation: CallableImplementation::Script,
            host_policy: None,
            method_policy: Default::default(),
            generic_params: vec![receiver_parameter.clone(), first.clone(), second.clone()],
            bounds: vec![],
            params: vec![
                Param {
                    name: "self".into(),
                    ty: receiver(receiver_parameter.as_type()),
                    mutable: false,
                },
                Param {
                    name: "first".into(),
                    ty: first.as_type(),
                    mutable: false,
                },
                Param {
                    name: "second".into(),
                    ty: second.as_type(),
                    mutable: false,
                },
            ],
            return_type: Ty::Tuple(vec![
                receiver_parameter.as_type(),
                first.as_type(),
                second.as_type(),
            ]),
        }),
    };
    for specialized in [0, 1, 3] {
        let concrete = [
            Ty::Builtin(BuiltinType::I32),
            Ty::Builtin(BuiltinType::I64),
            Ty::Builtin(BuiltinType::Bool),
        ];
        let binders = (0..3 - specialized)
            .map(|position| GenericParam {
                owner: method,
                position,
            })
            .collect::<Vec<_>>();
        let identity = ConcreteFunctionIdentity {
            declaration: method,
            arguments: concrete[..specialized].to_vec(),
        };
        let arguments = identity
            .arguments
            .iter()
            .cloned()
            .chain(binders.iter().map(GenericParam::as_type))
            .collect::<Vec<_>>();
        let semantic = SemanticSlots {
            generic: (!binders.is_empty()).then(|| GenericBody {
                parameters: binders,
                bounds: vec![],
            }),
            params: [
                (0, receiver(arguments[0].clone())),
                (1, arguments[1].clone()),
                (2, arguments[2].clone()),
            ]
            .into(),
            result: Some(Ty::Tuple(arguments.clone())),
            ..Default::default()
        };
        let matches = |semantic: &SemanticSlots| {
            signature_matches(
                slice::from_ref(&record),
                &identity,
                semantic,
                &context.snapshot(),
                &Default::default(),
            )
        };
        assert!(semantic.types_valid(Some(&identity), &Default::default(), &context.snapshot()));
        assert!(matches(&semantic), "specialized prefix {specialized}");

        let mut wrong_receiver = semantic.clone();
        wrong_receiver
            .params
            .insert(0, receiver(Ty::Builtin(BuiltinType::U32)));
        assert!(!matches(&wrong_receiver));
        let mut wrong_result = semantic.clone();
        wrong_result.result = Some(Ty::Builtin(BuiltinType::Unit));
        assert!(!matches(&wrong_result));
        let mut missing_slot = semantic.clone();
        missing_slot.params.remove(&1);
        missing_slot.params.insert(3, arguments[1].clone());
        assert!(!matches(&missing_slot));

        let mut extra_identity_argument = identity.clone();
        extra_identity_argument
            .arguments
            .push(Ty::Builtin(BuiltinType::Bool));
        assert!(!signature_matches(
            slice::from_ref(&record),
            &extra_identity_argument,
            &semantic,
            &context.snapshot(),
            &Default::default()
        ));
        if specialized == 3 {
            continue;
        }
        // Shared entry symbols are owned/numbered by the body, not by their
        // ordinal position after the specialized prefix or by the receiver impl.
        let mut wrong_owner = semantic.clone();
        wrong_owner.params.insert(1, receiver_parameter.as_type());
        assert!(!matches(&wrong_owner));
        let mut wrong_position = semantic.clone();
        wrong_position.params.insert(
            1,
            Ty::Parameter {
                owner: method,
                position: 9,
            },
        );
        assert!(!matches(&wrong_position));
        let mut missing_binder = semantic.clone();
        missing_binder.generic.as_mut().unwrap().parameters.pop();
        assert!(!matches(&missing_binder));
        let mut reordered_binders = semantic.clone();
        reordered_binders
            .generic
            .as_mut()
            .unwrap()
            .parameters
            .swap(0, 1);
        assert!(!matches(&reordered_binders));
    }
}
