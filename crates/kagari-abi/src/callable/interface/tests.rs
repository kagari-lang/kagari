use super::*;
use crate::{
    callable::{CallableImplementation, MethodPolicy},
    declaration::ModuleDecl,
    scalar::BuiltinType,
    types::{FunctionAbi, ParameterAbi},
};
use kagari_common::identity::ModuleIdentity;
use std::{collections::BTreeMap, slice};

fn fixture() -> (InterfaceCallContract, TraitAbi) {
    let module = ModuleDecl::new(ModuleIdentity::single_file("generic-interface.kgr"));
    let owner = module.definition(DefinitionKind::Trait, "Map");
    let item = GenericParameterAbi {
        owner: owner.clone(),
        position: 0,
    };
    let key = GenericParameterAbi {
        owner: ModuleDecl::method_id(&owner, "map"),
        position: 0,
    };
    let contract = TraitAbi {
        name: "Map".into(),
        generic_params: vec![item.clone()],
        bounds: vec![],
        supertraits: vec![],
        associated_types: vec![],
        associated_consts: vec![],
        methods: vec![FunctionAbi {
            name: "map".into(),
            implementation: CallableImplementation::Required,
            method_policy: MethodPolicy::default(),
            generic_params: vec![key.clone()],
            bounds: vec![],
            params: vec![
                ParameterAbi {
                    name: "self".into(),
                    ty: AbiType::SelfType(owner.clone()),
                    mutable: false,
                },
                ParameterAbi {
                    name: "f".into(),
                    ty: AbiType::Function {
                        params: vec![item.as_type()],
                        result: Box::new(key.as_type()),
                    },
                    mutable: false,
                },
            ],
            return_type: AbiType::Tuple(vec![item.as_type(), key.as_type()]),
        }],
    };
    let call = InterfaceCallContract {
        receiver: None,
        operations: vec![],
        interface: NominalAbiType {
            declaration: owner,
            arguments: vec![AbiType::Builtin(BuiltinType::I32)],
            associated_types: BTreeMap::new(),
        },
        method_slot: 0,
        arguments: vec![AbiType::Builtin(BuiltinType::String)],
    };
    (call, contract)
}

#[test]
fn interface_application_substitutes_both_binders_inside_callbacks_and_results() {
    let (mut call, contract) = fixture();
    let cancel = CancellationToken::default();
    let catalog = ProofCatalog::new(vec![], vec![], [], [], [], &cancel).unwrap();
    for key in [
        AbiType::Builtin(BuiltinType::I32),
        AbiType::Tuple(vec![AbiType::Builtin(BuiltinType::String)]),
    ] {
        call.arguments = vec![key.clone()];
        let signature = call.signature(&contract, &cancel).unwrap();
        assert_eq!(
            signature.params[1],
            AbiType::Function {
                params: vec![AbiType::Builtin(BuiltinType::I32)],
                result: Box::new(key.clone()),
            }
        );
        assert_eq!(
            signature.result,
            AbiType::Tuple(vec![AbiType::Builtin(BuiltinType::I32), key])
        );
        assert!(call.check(&contract, &catalog, &[], &[], &cancel).unwrap());
    }
}

#[test]
fn forwarded_parameters_require_the_callers_scope_and_bound_evidence() {
    let (mut call, mut contract) = fixture();
    let cancel = CancellationToken::default();
    let marker = NominalAbiType {
        declaration: ModuleDecl::new(ModuleIdentity::single_file("generic-interface.kgr"))
            .definition(DefinitionKind::Trait, "Marker"),
        arguments: vec![],
        associated_types: BTreeMap::new(),
    };
    let key = contract.methods[0].generic_params[0].as_type();
    contract.methods[0].bounds.push(GenericBoundAbi {
        ty: key,
        constraints: vec![ConstraintAbi::Trait(marker.clone())],
    });
    let parameter = GenericParameterAbi {
        owner: ModuleDecl::new(ModuleIdentity::single_file("caller.kgr"))
            .definition(DefinitionKind::Function, "forward"),
        position: 0,
    };
    call.arguments = vec![parameter.as_type()];
    let evidence = GenericBoundAbi {
        ty: parameter.as_type(),
        constraints: vec![ConstraintAbi::Trait(marker.clone())],
    };
    let marker_contract = TraitAbi {
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
    let (call, contract) = fixture();
    let cancel = CancellationToken::default();
    for mutation in 0..4 {
        let mut bad = call.clone();
        match mutation {
            0 => bad.arguments.clear(),
            1 => bad.interface.arguments.clear(),
            2 => bad.method_slot = 9,
            _ => bad.interface.declaration.path[0].name = "Other".into(),
        }
        assert!(bad.signature(&contract, &cancel).is_err());
    }
    let mut bad = contract.clone();
    bad.methods[0].return_type = AbiType::SelfType(call.interface.declaration.clone());
    let catalog = ProofCatalog::new(vec![], vec![], [], [], [], &cancel).unwrap();
    assert!(!call.check(&bad, &catalog, &[], &[], &cancel).unwrap());
}
