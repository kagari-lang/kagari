use super::*;
use crate::{
    callable::{CallableImplementation, MethodPolicy},
    declaration::ModuleDecl,
    scalar::BuiltinType,
    types::{FnDecl, Param},
};
use kagari_common::identity::ModuleIdentity;
use std::{collections::BTreeMap, slice};

fn fixture() -> (InterfaceCallContract, TraitDef) {
    let module = ModuleDecl::new(ModuleIdentity::single_file("generic-interface.kgr"));
    let owner = module.definition(DefinitionKind::Trait, "Map");
    let item = GenericParam {
        owner: owner.clone(),
        position: 0,
    };
    let key = GenericParam {
        owner: ModuleDecl::method_id(&owner, "map"),
        position: 0,
    };
    let contract = TraitDef {
        conversion_adapter: None,
        storage_access: None,
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
                    ty: Ty::SelfType(owner.clone()),
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
    let (mut call, contract) = fixture();
    let cancel = CancellationToken::default();
    let catalog = ProofCatalog::new(vec![], vec![], [], [], [], &cancel).unwrap();
    for key in [
        Ty::Builtin(BuiltinType::I32),
        Ty::Tuple(vec![Ty::Builtin(BuiltinType::String)]),
    ] {
        call.arguments = vec![key.clone()];
        let signature = call.signature(&contract, &cancel).unwrap();
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
    let (mut call, mut contract) = fixture();
    let cancel = CancellationToken::default();
    let marker = NominalTy {
        declaration: ModuleDecl::new(ModuleIdentity::single_file("generic-interface.kgr"))
            .definition(DefinitionKind::Trait, "Marker"),
        arguments: vec![],
        associated_types: BTreeMap::new(),
    };
    let key = contract.methods[0].generic_params[0].as_type();
    contract.methods[0].bounds.push(GenericBound {
        ty: key,
        constraints: vec![Constraint::Trait(marker.clone())],
    });
    let parameter = GenericParam {
        owner: ModuleDecl::new(ModuleIdentity::single_file("caller.kgr"))
            .definition(DefinitionKind::Function, "forward"),
        position: 0,
    };
    call.arguments = vec![parameter.as_type()];
    let evidence = GenericBound {
        ty: parameter.as_type(),
        constraints: vec![Constraint::Trait(marker.clone())],
    };
    let marker_contract = TraitDef {
        conversion_adapter: None,
        storage_access: None,
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
    bad.methods[0].return_type = Ty::SelfType(call.interface.declaration.clone());
    let catalog = ProofCatalog::new(vec![], vec![], [], [], [], &cancel).unwrap();
    assert!(!call.check(&bad, &catalog, &[], &[], &cancel).unwrap());
}
