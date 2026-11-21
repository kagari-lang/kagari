use super::*;
use kagari_common::identity::{ModuleIdentity, map::DefinitionContext};
use kagari_types::{
    callable::CallableImplementation,
    declaration::{Param, TraitDef},
    scalar::BuiltinType::{self, I8, I32, I64, U8},
    ty::{GenericBound, GenericParam},
};

#[test]
fn optimized_from_requires_the_proven_conversion_not_just_a_scalar_result() {
    let context = DefinitionContext::new().unwrap();
    let root = context.intern_root(&Protocol::From.module()).unwrap();
    let owner = context
        .intern_child(root, DefinitionKind::Trait, "From", 0)
        .unwrap();
    let member = context
        .intern_child(owner, DefinitionKind::Method, "from", 0)
        .unwrap();
    let parameter = GenericParam { owner, position: 0 };
    let contract = TraitDef {
        name: "From".into(),
        generic_params: vec![parameter.clone()],
        bounds: vec![],
        supertraits: vec![],
        associated_consts: vec![],
        associated_types: vec![],
        conversion_adapter: None,
        methods: vec![FnDecl {
            name: "from".into(),
            implementation: CallableImplementation::Required,
            generic_params: vec![],
            bounds: vec![],
            host_policy: None,
            method_policy: Default::default(),
            params: vec![Param {
                name: "value".into(),
                ty: parameter.as_type(),
                mutable: false,
            }],
            return_type: Ty::SelfType(owner),
        }],
    };
    let definitions = context.snapshot();
    let catalog = ProofCatalog::new(
        vec![],
        [],
        [(owner, &contract)],
        [],
        &definitions,
        &Default::default(),
    )
    .unwrap();
    let check = |source, target, actual_source, callee, entry| {
        let method = QualifiedMethod {
            receiver: Ty::Builtin(target),
            member,
            interface: NominalTy {
                declaration: owner,
                arguments: vec![Ty::Builtin(source)],
                associated_types: Default::default(),
            },
        };
        method.check_target(
            &catalog,
            vec![],
            &AppliedCall {
                callee,
                entry,
                arguments: vec![Value::Parameter(0)],
                types: vec![],
                result: Value::Result(0),
                signature: Signature {
                    params: vec![Ty::Builtin(actual_source)],
                    result: Ty::Builtin(target),
                },
            },
        )
    };

    check(U8, I64, U8, Value::Convert, None).unwrap();
    for error in [
        check(U8, I64, I8, Value::Convert, None), // different extension
        check(I32, I8, I32, Value::Convert, None), // legal cast, not a From
        check(
            U8,
            I64,
            U8,
            Value::Definition(member),
            Some(CallableImplementation::Script),
        ), // no selected Script impl
    ] {
        assert_eq!(error.unwrap_err().reason(), invalid().reason());
    }
}

#[test]
fn checked_generic_slot_still_has_to_use_the_installed_receiver_and_method_arguments() {
    let context = DefinitionContext::new().unwrap();
    let identity = ModuleIdentity::single_file("forward.kgr");
    let root = context.intern_root(&identity).unwrap();
    let owner = context
        .intern_child(root, DefinitionKind::Trait, "Read", 0)
        .unwrap();
    let member = context
        .intern_child(owner, DefinitionKind::Method, "read", 0)
        .unwrap();
    let caller = context
        .intern_child(root, DefinitionKind::Function, "forward", 0)
        .unwrap();
    let parameters = |owner| [0, 1].map(|position| GenericParam { owner, position });
    let interface = NominalTy {
        declaration: owner,
        arguments: vec![],
        associated_types: Default::default(),
    };
    let contract = TraitDef {
        name: "Read".into(),
        generic_params: vec![],
        methods: vec![FnDecl {
            name: "read".into(),
            implementation: CallableImplementation::Required,
            generic_params: parameters(member).into(),
            host_policy: None,
            method_policy: Default::default(),
            bounds: vec![],
            params: vec![],
            return_type: Ty::Builtin(BuiltinType::I32),
        }],
        bounds: vec![],
        supertraits: vec![],
        associated_consts: vec![],
        associated_types: vec![],
        conversion_adapter: None,
    };
    let module = BytecodeModule {
        identity,
        public_items: vec![PublicItem::Trait(contract.clone())],
        ..Default::default()
    };
    let definitions = context.snapshot();
    let cancel = CancellationToken::default();
    let catalog =
        ProofCatalog::new(vec![], [], [(owner, &contract)], [], &definitions, &cancel).unwrap();
    let scope = parameters(caller);
    let assumptions = scope.each_ref().map(|p| GenericBound {
        ty: p.as_type(),
        constraints: vec![interface.clone()],
    });
    let expected = QualifiedMethod {
        receiver: scope[0].as_type(),
        interface: interface.clone(),
        member,
    };
    let arguments = scope.each_ref().map(GenericParam::as_type);
    let check = |receiver: Ty, types: Vec<Ty>| {
        let slot = InterfaceCallContract {
            receiver: Some(receiver),
            interface: interface.clone(),
            method_slot: 0,
            arguments: types.clone(),
            constructions: vec![],
            operations: vec![],
            normalizations: vec![],
        };
        // All three operations are admitted under the same bounds and have the same
        // signature. Only the installed expression distinguishes them.
        assert!(
            slot.check(&contract, &catalog, &scope, &assumptions, &cancel)
                .unwrap()
        );
        let (selected, signature) =
            QualifiedMethod::checked_slot(&slot, &module, &definitions).unwrap();
        assert_eq!(
            signature,
            Signature {
                params: vec![],
                result: Ty::Builtin(BuiltinType::I32)
            }
        );
        expected.check_target(
            &catalog,
            arguments.to_vec(),
            &AppliedCall {
                callee: Value::TraitMethod(Box::new(selected)),
                entry: None,
                types,
                arguments: vec![],
                result: Value::Result(0),
                signature,
            },
        )
    };
    check(scope[0].as_type(), arguments.to_vec()).unwrap();
    assert_eq!(
        check(scope[1].as_type(), arguments.to_vec())
            .unwrap_err()
            .reason(),
        invalid().reason()
    );
    assert_eq!(
        check(
            scope[0].as_type(),
            arguments.iter().rev().cloned().collect()
        )
        .unwrap_err()
        .reason(),
        invalid().reason()
    );
}
