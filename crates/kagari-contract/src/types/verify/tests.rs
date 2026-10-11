use crate::types::verify::*;
use kagari_common::identity::{map::DefinitionContext, table::DefinitionId};
use kagari_stdlib::{catalog, namespaces};
use kagari_types::collection::CollectionAccess;
use kagari_types::{
    callable::CallableImplementation,
    declaration::{Param, conversion::ConversionAdapter},
    scalar::BuiltinType,
    ty::{
        GenericBound,
        matching::{ImplementationPattern, projection_output},
    },
};

fn owner(
    context: &DefinitionContext,
    module: &ModuleIdentity,
    kind: DefinitionKind,
    name: &str,
) -> DefinitionId {
    let root = context.intern_root(module).unwrap();
    context.intern_child(root, kind, name, 0).unwrap()
}

#[test]
fn installed_conversion_adapters_validate_shapes_and_member_ownership() {
    let context = DefinitionContext::new().unwrap();
    let module = catalog::shared()
        .unwrap()
        .into_iter()
        .find(|module| module.records().identity == namespaces::module("core", "convert"))
        .unwrap()
        .import_into(&context, &CancellationToken::default())
        .unwrap();
    let cancel = CancellationToken::default();
    for name in ["Into", "TryInto", "TryFrom"] {
        let contract = module
            .records()
            .traits
            .iter()
            .find(|contract| contract.name == name)
            .unwrap();
        let valid = |contract: &TraitDef| {
            validate(
                &[PublicItem::Trait(contract.clone())],
                &module.records().identity,
                &cancel,
                &context.snapshot(),
            )
            .is_ok()
        };
        assert!(valid(contract));
        let mut changed = contract.clone();
        changed.methods[0].return_type = Ty::Builtin(BuiltinType::Bool);
        assert!(!valid(&changed));
        changed = contract.clone();
        changed.storage_access = Some(CollectionAccess::ReadOnly);
        assert!(!valid(&changed));
        changed = contract.clone();
        match changed.conversion_adapter.as_mut().unwrap() {
            ConversionAdapter::Reverse { origin, .. } => {
                *origin = module
                    .records()
                    .definition(DefinitionKind::Trait, name, &context.snapshot())
                    .unwrap();
            }
            ConversionAdapter::Forward { method, .. } => {
                let parent = context.snapshot().parent(*method).unwrap().unwrap();
                *method = context
                    .intern_child(parent, DefinitionKind::Method, "undeclared", 0)
                    .unwrap();
            }
        }
        assert!(!valid(&changed));
        changed = contract.clone();
        let method = match changed.conversion_adapter.as_mut().unwrap() {
            ConversionAdapter::Reverse { method, .. }
            | ConversionAdapter::Forward { method, .. } => method,
        };
        let definitions = context.snapshot();
        let parent = definitions.parent(*method).unwrap().unwrap();
        let segment = definitions.resolve(*method).unwrap().last().unwrap();
        *method = context
            .intern_child(
                parent,
                DefinitionKind::Function,
                segment.name,
                segment.occurrence,
            )
            .unwrap();
        assert!(!valid(&changed));
    }
}

#[test]
fn requirements_are_not_executable_functions_or_forged_native_entries() {
    let context = DefinitionContext::new().unwrap();
    let module = ModuleIdentity::single_file("requirements.kgr");
    owner(&context, &module, DefinitionKind::Function, "read");
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
        validate(
            &[PublicItem::Function(function.clone())],
            &module,
            &cancel,
            &context.snapshot(),
        )
        .is_ok()
    };
    assert!(!valid(&function));
    function.implementation = CallableImplementation::Script;
    assert!(valid(&function));
    function.method_policy.override_allowed = false;
    assert!(!valid(&function));
    function.method_policy.override_allowed = true;
    function.implementation =
        CallableImplementation::Native(owner(&context, &module, DefinitionKind::Trait, "forged"));
    assert!(!valid(&function));
}

#[test]
fn required_methods_cannot_forbid_an_implementation() {
    let context = DefinitionContext::new().unwrap();
    let module = ModuleIdentity::single_file("method-policy.kgr");
    let interface_id = owner(&context, &module, DefinitionKind::Trait, "Reader");
    context
        .intern_child(interface_id, DefinitionKind::Method, "read", 0)
        .unwrap();
    let cancel = CancellationToken::default();
    let mut interface = TraitDef {
        conversion_adapter: None,
        storage_access: None,
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
        validate(
            &[PublicItem::Trait(interface.clone())],
            &module,
            &cancel,
            &context.snapshot(),
        )
        .is_ok()
    };
    assert!(valid(&interface));
    interface.methods[0].method_policy.override_allowed = false;
    assert!(!valid(&interface));
    interface.methods[0].implementation = CallableImplementation::Script;
    assert!(valid(&interface));
}

#[test]
fn interface_method_contract_substitutes_self_and_method_binders_inside_containers() {
    let context = DefinitionContext::new().unwrap();
    let module = ModuleIdentity::single_file("interface.kgr");
    let trait_owner = owner(&context, &module, DefinitionKind::Trait, "Read");
    let impl_owner = owner(&context, &module, DefinitionKind::Impl, "");
    let trait_method = context
        .intern_child(trait_owner, DefinitionKind::Method, "read", 0)
        .unwrap();
    let impl_method = context
        .intern_child(impl_owner, DefinitionKind::Method, "read", 0)
        .unwrap();
    let for_type = Ty::Struct(NominalTy {
        associated_types: Default::default(),
        declaration: owner(&context, &module, DefinitionKind::Struct, "Player"),
        arguments: Vec::new(),
    });
    let mut declared = FnDecl {
        method_policy: Default::default(),
        implementation: CallableImplementation::Required,
        name: "read".into(),
        generic_params: vec![GenericParam {
            owner: trait_method,
            position: 0,
        }],
        bounds: Vec::new(),
        params: vec![Param {
            name: "input".into(),
            ty: Ty::Array(Box::new(Ty::Tuple(vec![
                Ty::SelfType(trait_owner),
                Ty::Parameter {
                    owner: trait_method,
                    position: 0,
                },
            ]))),
            mutable: false,
        }],
        return_type: Ty::Builtin(BuiltinType::I32),
    };
    let mut implemented = FnDecl {
        method_policy: Default::default(),
        implementation: CallableImplementation::Script,
        name: "read".into(),
        generic_params: vec![GenericParam {
            owner: impl_method,
            position: 0,
        }],
        bounds: Vec::new(),
        params: vec![Param {
            name: "renamed".into(),
            ty: Ty::Array(Box::new(Ty::Tuple(vec![
                for_type.clone(),
                Ty::Parameter {
                    owner: impl_method,
                    position: 0,
                },
            ]))),
            mutable: false,
        }],
        return_type: Ty::Builtin(BuiltinType::I32),
    };
    let cancel = CancellationToken::default();
    let trait_instance = NominalTy {
        declaration: trait_owner,
        arguments: Vec::new(),
        associated_types: Default::default(),
    };
    let make_table = || InterfaceTable {
        name: "Read".into(),
        associated_type_families: Vec::new(),
        associated_consts: Vec::new(),
        declaration: impl_owner,
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
        &context.snapshot()
    ));
    declared.implementation = CallableImplementation::Script;
    declared.method_policy.override_allowed = false;
    assert!(!same_method_contract(
        &declared,
        &implemented,
        &trait_instance,
        &make_table(),
        &cancel,
        &context.snapshot()
    ));
    declared.method_policy.override_allowed = true;
    assert!(same_method_contract(
        &declared,
        &implemented,
        &trait_instance,
        &make_table(),
        &cancel,
        &context.snapshot()
    ));
    declared.implementation = CallableImplementation::Required;
    let original_param = implemented.params[0].ty.clone();
    implemented.params[0].ty = Ty::Array(Box::new(Ty::Tuple(vec![
        for_type.clone(),
        Ty::Builtin(BuiltinType::Bool),
    ])));
    assert!(!same_method_contract(
        &declared,
        &implemented,
        &trait_instance,
        &make_table(),
        &cancel,
        &context.snapshot()
    ));
    implemented.params[0].ty = original_param;
    declared.bounds.push(GenericBound {
        ty: Ty::Parameter {
            owner: trait_method,
            position: 0,
        },
        constraints: vec![Constraint::Standard(
            kagari_types::surface::StandardTypeConstraint::HashKey,
        )],
    });
    implemented.bounds.push(GenericBound {
        ty: Ty::Parameter {
            owner: impl_method,
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
        &context.snapshot()
    ));
    implemented.bounds[0].constraints.clear();
    assert!(!same_method_contract(
        &declared,
        &implemented,
        &trait_instance,
        &make_table(),
        &cancel,
        &context.snapshot()
    ));
    let marker = owner(&context, &module, DefinitionKind::Trait, "Marker");
    let applied = |parameter_owner| {
        Constraint::Trait(NominalTy {
            associated_types: Default::default(),
            declaration: marker,
            arguments: vec![Ty::Array(Box::new(Ty::Parameter {
                owner: parameter_owner,
                position: 0,
            }))],
        })
    };
    declared.bounds[0].constraints = vec![applied(trait_method)];
    implemented.bounds[0].constraints = vec![applied(impl_method)];
    assert!(same_method_contract(
        &declared,
        &implemented,
        &trait_instance,
        &make_table(),
        &cancel,
        &context.snapshot()
    ));
    let Constraint::Trait(instance) = &mut implemented.bounds[0].constraints[0] else {
        unreachable!()
    };
    instance.arguments[0] = Ty::Array(Box::new(Ty::Builtin(BuiltinType::Bool)));
    assert!(!same_method_contract(
        &declared,
        &implemented,
        &trait_instance,
        &make_table(),
        &cancel,
        &context.snapshot()
    ));
}

#[test]
fn dependency_projection_deferral_requires_a_successful_linked_comparison() {
    let context = DefinitionContext::new().unwrap();
    let module = ModuleIdentity::single_file("caller.kgr");
    let interface_id = owner(&context, &module, DefinitionKind::Trait, "Read");
    let dependency_id = owner(
        &context,
        &ModuleIdentity::single_file("dependency.kgr"),
        DefinitionKind::Trait,
        "Value",
    );
    let member = context
        .intern_child(dependency_id, DefinitionKind::AssociatedType, "Item", 0)
        .unwrap();
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
            member: member,
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
        declaration: owner(&context, &module, DefinitionKind::Impl, ""),
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
        &cancel,
        &context.snapshot()
    ));
    let normalize = |ty: &Ty| {
        normalize_projections(
            ty,
            &|interface, receiver, member, arguments| {
                projection_output(
                    ImplementationPattern {
                        protocol: match &dependency.trait_type {
                            Ty::Trait(interface) => Protocol::from_definition(
                                &interface.declaration,
                                &context.snapshot(),
                            ),
                            _ => None,
                        },
                        storage_access: None,
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
