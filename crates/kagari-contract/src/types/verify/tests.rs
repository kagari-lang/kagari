use crate::{
    callable::CallableImplementation,
    types::{Param, verify::*},
};

#[test]
fn requirements_are_not_executable_functions_or_forged_native_entries() {
    let module = ModuleIdentity::single_file("requirements.kgr");
    let cancel = CancellationToken::default();
    let mut function = FnDecl {
        method_policy: Default::default(),
        name: "read".into(),
        implementation: CallableImplementation::Required,
        generic_params: vec![],
        bounds: vec![],
        params: vec![],
        return_type: Ty::Builtin(BuiltinType::I32),
    };
    let valid = |function: &FnDecl| {
        validate(&[PublicItem::Function(function.clone())], &module, &cancel).is_ok()
    };
    assert!(!valid(&function));
    function.implementation = CallableImplementation::Script;
    assert!(valid(&function));
    function.method_policy.override_allowed = false;
    assert!(!valid(&function));
    function.method_policy.override_allowed = true;
    function.implementation =
        CallableImplementation::Native(owner(&module, &[], DefinitionKind::Trait, "forged"));
    assert!(!valid(&function));
}

#[test]
fn required_methods_cannot_forbid_an_implementation() {
    let module = ModuleIdentity::single_file("method-policy.kgr");
    let cancel = CancellationToken::default();
    let mut interface = TraitDef {
        name: "Reader".into(),
        associated_consts: vec![],
        associated_types: vec![],
        supertraits: vec![],
        generic_params: vec![],
        bounds: vec![],
        methods: vec![FnDecl {
            method_policy: Default::default(),
            name: "read".into(),
            implementation: CallableImplementation::Required,
            generic_params: vec![],
            bounds: vec![],
            params: vec![],
            return_type: Ty::Builtin(BuiltinType::I32),
        }],
    };
    let valid = |interface: &TraitDef| {
        validate(&[PublicItem::Trait(interface.clone())], &module, &cancel).is_ok()
    };
    assert!(valid(&interface));
    interface.methods[0].method_policy.override_allowed = false;
    assert!(!valid(&interface));
    interface.methods[0].implementation = CallableImplementation::Script;
    assert!(valid(&interface));
}

#[test]
fn interface_method_contract_substitutes_self_and_method_binders_inside_containers() {
    let module = ModuleIdentity::single_file("interface.kgr");
    let trait_owner = owner(&module, &[], DefinitionKind::Trait, "Read");
    let impl_owner = owner(&module, &[], DefinitionKind::Impl, "");
    let trait_method = owner(&module, &trait_owner.path, DefinitionKind::Method, "read");
    let impl_method = owner(&module, &impl_owner.path, DefinitionKind::Method, "read");
    let for_type = Ty::Struct(NominalTy {
        associated_types: Default::default(),
        declaration: owner(&module, &[], DefinitionKind::Struct, "Player"),
        arguments: Vec::new(),
    });
    let mut declared = FnDecl {
        method_policy: Default::default(),
        implementation: CallableImplementation::Required,
        name: "read".into(),
        generic_params: vec![GenericParam {
            owner: trait_method.clone(),
            position: 0,
        }],
        bounds: Vec::new(),
        params: vec![Param {
            name: "input".into(),
            ty: Ty::Array(
                Box::new(Ty::Tuple(vec![
                    Ty::SelfType(trait_owner.clone()),
                    Ty::Parameter {
                        owner: trait_method.clone(),
                        position: 0,
                    },
                ])),
                CollectionAccess::Mutable,
            ),
            mutable: false,
        }],
        return_type: Ty::Builtin(BuiltinType::I32),
    };
    let mut implemented = FnDecl {
        method_policy: Default::default(),
        implementation: CallableImplementation::Script,
        name: "read".into(),
        generic_params: vec![GenericParam {
            owner: impl_method.clone(),
            position: 0,
        }],
        bounds: Vec::new(),
        params: vec![Param {
            name: "renamed".into(),
            ty: Ty::Array(
                Box::new(Ty::Tuple(vec![
                    for_type.clone(),
                    Ty::Parameter {
                        owner: impl_method.clone(),
                        position: 0,
                    },
                ])),
                CollectionAccess::Mutable,
            ),
            mutable: false,
        }],
        return_type: Ty::Builtin(BuiltinType::I32),
    };
    let cancel = CancellationToken::default();
    let trait_instance = NominalTy {
        declaration: trait_owner.clone(),
        arguments: Vec::new(),
        associated_types: Default::default(),
    };
    let make_table = || InterfaceTable {
        name: "Read".into(),
        associated_type_families: Vec::new(),
        associated_consts: Vec::new(),
        declaration: impl_owner.clone(),
        for_type: for_type.clone(),
        trait_type: Ty::Trait(trait_instance.clone()),
        generic_params: Vec::new(),
        bounds: Vec::new(),
        methods: Vec::new(),
        host_bridge: false,
    };
    assert!(same_method_contract(
        &declared,
        &implemented,
        &trait_instance,
        &make_table(),
        &cancel,
    ));
    declared.implementation = CallableImplementation::Script;
    declared.method_policy.override_allowed = false;
    assert!(!same_method_contract(
        &declared,
        &implemented,
        &trait_instance,
        &make_table(),
        &cancel,
    ));
    declared.method_policy.override_allowed = true;
    assert!(same_method_contract(
        &declared,
        &implemented,
        &trait_instance,
        &make_table(),
        &cancel,
    ));
    declared.implementation = CallableImplementation::Required;
    let original_param = implemented.params[0].ty.clone();
    implemented.params[0].ty = Ty::Array(
        Box::new(Ty::Tuple(vec![
            for_type.clone(),
            Ty::Builtin(BuiltinType::Bool),
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
    declared.bounds.push(GenericBound {
        ty: Ty::Parameter {
            owner: trait_method.clone(),
            position: 0,
        },
        constraints: vec![Constraint::Standard(
            crate::standard::surface::StandardTypeConstraint::HashKey,
        )],
    });
    implemented.bounds.push(GenericBound {
        ty: Ty::Parameter {
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
        Constraint::Trait(NominalTy {
            associated_types: Default::default(),
            declaration: marker.clone(),
            arguments: vec![Ty::Array(
                Box::new(Ty::Parameter {
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
    let Constraint::Trait(instance) = &mut implemented.bounds[0].constraints[0] else {
        unreachable!()
    };
    instance.arguments[0] = Ty::Array(
        Box::new(Ty::Builtin(BuiltinType::Bool)),
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

#[test]
fn dependency_projection_deferral_requires_a_successful_linked_comparison() {
    let module = ModuleIdentity::single_file("caller.kgr");
    let interface_id = owner(&module, &[], DefinitionKind::Trait, "Read");
    let dependency_id = owner(
        &ModuleIdentity::single_file("dependency.kgr"),
        &[],
        DefinitionKind::Trait,
        "Value",
    );
    let member = identity::associated_type_id(&dependency_id, "Item");
    let receiver = Ty::Builtin(BuiltinType::I32);
    let dependency_view = NominalTy {
        declaration: dependency_id,
        arguments: vec![],
        associated_types: BTreeMap::new(),
    };
    let declared = FnDecl {
        method_policy: Default::default(),
        implementation: CallableImplementation::Required,
        name: "read".into(),
        generic_params: vec![],
        bounds: vec![],
        params: vec![],
        return_type: Ty::Projection {
            receiver: Box::new(receiver.clone()),
            interface: Box::new(dependency_view.clone()),
            member: member.clone(),
            arguments: vec![],
        },
    };
    let mut implemented = FnDecl {
        method_policy: Default::default(),
        implementation: CallableImplementation::Script,
        return_type: Ty::Builtin(BuiltinType::Bool),
        ..declared.clone()
    };
    let view = NominalTy {
        declaration: interface_id,
        arguments: vec![],
        associated_types: BTreeMap::new(),
    };
    let table = InterfaceTable {
        name: "Read".into(),
        declaration: owner(&module, &[], DefinitionKind::Impl, ""),
        generic_params: vec![],
        bounds: vec![],
        for_type: receiver.clone(),
        trait_type: Ty::Trait(view.clone()),
        associated_consts: vec![],
        associated_type_families: vec![],
        methods: vec![],
        host_bridge: false,
    };
    let dependency = InterfaceTable {
        trait_type: Ty::Trait(NominalTy {
            associated_types: [(member, receiver.clone())].into(),
            ..dependency_view
        }),
        ..table.clone()
    };
    let cancel = CancellationToken::default();
    assert!(same_method_contract(
        &declared,
        &implemented,
        &view,
        &table,
        &cancel
    ));
    let normalize = |ty: &Ty| {
        normalize_projections(
            ty,
            &|interface, receiver, member, arguments| {
                matching::projection_output(
                    matching::ImplementationPattern {
                        parameters: &dependency.generic_params,
                        receiver: &dependency.for_type,
                        interface: match &dependency.trait_type {
                            Ty::Trait(interface) => interface,
                            _ => unreachable!(),
                        },
                    },
                    &dependency.associated_type_families,
                    interface,
                    receiver,
                    member,
                    arguments,
                    &cancel,
                )
            },
            &cancel,
        )
    };
    assert!(!method_contract_matches(
        &declared,
        &implemented,
        &view,
        &table,
        &normalize,
        false,
        &cancel
    ));
    implemented.return_type = receiver;
    assert!(method_contract_matches(
        &declared,
        &implemented,
        &view,
        &table,
        &normalize,
        false,
        &cancel
    ));
    cancel.cancel();
    assert!(!method_contract_matches(
        &declared,
        &implemented,
        &view,
        &table,
        &normalize,
        false,
        &cancel
    ));
}
