use super::*;
use kagari_common::identity::{ModuleIdentity, map::DefinitionContext};
use kagari_types::{
    callable::CallableImplementation,
    declaration::TraitDef,
    scalar::BuiltinType,
    ty::{GenericBound, GenericParam},
};

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
