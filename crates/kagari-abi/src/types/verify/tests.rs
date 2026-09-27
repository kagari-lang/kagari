use crate::types::ParameterAbi;
use crate::types::verify::*;

#[test]
fn interface_method_contract_substitutes_self_and_method_binders_inside_containers() {
    let module = ModuleIdentity::single_file("interface.kgr");
    let trait_owner = owner(&module, &[], DefinitionKind::Trait, "Read");
    let impl_owner = owner(&module, &[], DefinitionKind::Impl, "");
    let trait_method = owner(&module, &trait_owner.path, DefinitionKind::Method, "read");
    let impl_method = owner(&module, &impl_owner.path, DefinitionKind::Method, "read");
    let for_type = AbiType::Struct(NominalAbiType {
        associated_types: Default::default(),
        declaration: owner(&module, &[], DefinitionKind::Struct, "Player"),
        arguments: Vec::new(),
    });
    let mut declared = FunctionAbi {
        name: "read".into(),
        generic_params: vec![GenericParameterAbi {
            owner: trait_method.clone(),
            position: 0,
        }],
        bounds: Vec::new(),
        params: vec![ParameterAbi {
            name: "input".into(),
            ty: AbiType::Array(
                Box::new(AbiType::Tuple(vec![
                    AbiType::SelfType(trait_owner.clone()),
                    AbiType::Parameter {
                        owner: trait_method.clone(),
                        position: 0,
                    },
                ])),
                CollectionAccess::Mutable,
            ),
            mutable: false,
        }],
        return_type: AbiType::Builtin(BuiltinType::I32),
    };
    let mut implemented = FunctionAbi {
        name: "read".into(),
        generic_params: vec![GenericParameterAbi {
            owner: impl_method.clone(),
            position: 0,
        }],
        bounds: Vec::new(),
        params: vec![ParameterAbi {
            name: "renamed".into(),
            ty: AbiType::Array(
                Box::new(AbiType::Tuple(vec![
                    for_type.clone(),
                    AbiType::Parameter {
                        owner: impl_method.clone(),
                        position: 0,
                    },
                ])),
                CollectionAccess::Mutable,
            ),
            mutable: false,
        }],
        return_type: AbiType::Builtin(BuiltinType::I32),
    };
    let cancel = CancellationToken::default();
    let trait_instance = NominalAbiType {
        declaration: trait_owner.clone(),
        arguments: Vec::new(),
        associated_types: Default::default(),
    };
    let make_table = || InterfaceTableAbi {
        name: "Read".into(),
        associated_type_families: Vec::new(),
        associated_consts: Vec::new(),
        declaration: impl_owner.clone(),
        for_type: for_type.clone(),
        trait_type: AbiType::Trait(trait_instance.clone()),
        generic_params: Vec::new(),
        bounds: Vec::new(),
        methods: Vec::new(),
        host_bridge: false,
        native_bridge: false,
    };
    assert!(same_method_contract(
        &declared,
        &implemented,
        &trait_instance,
        &make_table(),
        &cancel,
    ));
    let original_param = implemented.params[0].ty.clone();
    implemented.params[0].ty = AbiType::Array(
        Box::new(AbiType::Tuple(vec![
            for_type.clone(),
            AbiType::Builtin(BuiltinType::Bool),
        ])),
        CollectionAccess::Mutable,
    );
    assert!(!same_method_contract(
        &declared,
        &implemented,
        &trait_instance,
        &make_table(),
        &cancel,
    ));
    implemented.params[0].ty = original_param;
    declared.bounds.push(GenericBoundAbi {
        ty: AbiType::Parameter {
            owner: trait_method.clone(),
            position: 0,
        },
        constraints: vec![ConstraintAbi::Standard(
            crate::standard::surface::StandardTypeConstraint::HashKey,
        )],
    });
    implemented.bounds.push(GenericBoundAbi {
        ty: AbiType::Parameter {
            owner: impl_method.clone(),
            position: 0,
        },
        constraints: declared.bounds[0].constraints.clone(),
    });
    assert!(same_method_contract(
        &declared,
        &implemented,
        &trait_instance,
        &make_table(),
        &cancel,
    ));
    implemented.bounds[0].constraints.clear();
    assert!(!same_method_contract(
        &declared,
        &implemented,
        &trait_instance,
        &make_table(),
        &cancel,
    ));
    let marker = owner(&module, &[], DefinitionKind::Trait, "Marker");
    let applied = |parameter_owner| {
        ConstraintAbi::Trait(NominalAbiType {
            associated_types: Default::default(),
            declaration: marker.clone(),
            arguments: vec![AbiType::Array(
                Box::new(AbiType::Parameter {
                    owner: parameter_owner,
                    position: 0,
                }),
                CollectionAccess::Mutable,
            )],
        })
    };
    declared.bounds[0].constraints = vec![applied(trait_method.clone())];
    implemented.bounds[0].constraints = vec![applied(impl_method)];
    assert!(same_method_contract(
        &declared,
        &implemented,
        &trait_instance,
        &make_table(),
        &cancel,
    ));
    let ConstraintAbi::Trait(instance) = &mut implemented.bounds[0].constraints[0] else {
        unreachable!()
    };
    instance.arguments[0] = AbiType::Array(
        Box::new(AbiType::Builtin(BuiltinType::Bool)),
        CollectionAccess::Mutable,
    );
    assert!(!same_method_contract(
        &declared,
        &implemented,
        &trait_instance,
        &make_table(),
        &cancel,
    ));
}
