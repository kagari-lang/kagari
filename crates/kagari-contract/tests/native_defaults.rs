//! Portable defaults select actual native templates without source analysis.
use kagari_common::{
    cancellation::CancellationToken,
    collection::CollectionAccess,
    identity::{
        DefinitionKind, DefinitionPath, DefinitionPathSegment, ModuleIdentity, PackageId,
        associated_type_id,
    },
};
use kagari_contract::{
    callable::{CallableImplementation, MethodPolicy, NativeDefaultApplication},
    declaration::ImplDecl,
    native_import::callables::NativeCallableRequirement,
    scalar::BuiltinType,
    types::{
        AssociatedTypeDef, Constraint, FnDecl, GenericBound, GenericParam, InterfaceTable,
        NativeDeclaration, NominalTy, Param, PublicItem, TraitDef, Ty,
        proofs::{ProofCatalog, implementation::Implementation},
        substitution::{TypeSubstitution, TypeTransformError},
        verify,
    },
};
use std::collections::BTreeMap;

fn identity(kind: DefinitionKind, name: &str) -> DefinitionPath {
    DefinitionPath {
        module: ModuleIdentity {
            package: PackageId("game".into()),
            path: vec!["defaults".into()],
        },
        path: vec![DefinitionPathSegment {
            kind,
            name: name.into(),
            occurrence: 0,
        }],
    }
}

fn scalar() -> Ty {
    Ty::Builtin(BuiltinType::I32)
}

fn interface(owner: &DefinitionPath, arguments: Vec<Ty>) -> NominalTy {
    NominalTy {
        declaration: owner.clone(),
        arguments,
        associated_types: BTreeMap::new(),
    }
}

fn function(name: &str, implementation: CallableImplementation, parameter: Ty) -> FnDecl {
    FnDecl {
        name: name.into(),
        implementation,
        method_policy: MethodPolicy::default(),
        generic_params: vec![],
        bounds: vec![],
        params: vec![Param {
            name: "value".into(),
            ty: parameter,
            mutable: false,
        }],
        return_type: scalar(),
    }
}

struct Fixture {
    owner: DefinitionPath,
    contract: TraitDef,
    template: NativeDeclaration,
    table: InterfaceTable,
}

impl Fixture {
    fn new() -> Self {
        let owner = identity(DefinitionKind::Trait, "Echo");
        let declaration = identity(DefinitionKind::Function, "default_echo");
        let parameter = GenericParam {
            owner: declaration.clone(),
            position: 0,
        };
        let mut body = function(
            "default_echo",
            CallableImplementation::Native(identity(DefinitionKind::Function, "handler")),
            parameter.as_type(),
        );
        body.generic_params.push(parameter.clone());
        body.bounds.push(GenericBound {
            ty: parameter.as_type(),
            constraints: vec![Constraint::Trait(interface(&owner, vec![]))],
        });
        let application = NativeDefaultApplication {
            declaration: declaration.clone(),
            arguments: vec![Ty::SelfType(owner.clone())],
        };
        let mut method = function(
            "echo",
            CallableImplementation::NativeDefault(application),
            Ty::SelfType(owner.clone()),
        );
        method.method_policy.override_allowed = false;
        let mut derived = method.clone();
        derived.params[0].ty = scalar();
        let CallableImplementation::NativeDefault(application) = &mut derived.implementation else {
            unreachable!()
        };
        application.arguments[0] = scalar();
        Self {
            owner: owner.clone(),
            contract: TraitDef {
                name: "Echo".into(),
                generic_params: vec![],
                bounds: vec![],
                supertraits: vec![],
                associated_types: vec![],
                associated_consts: vec![],
                methods: vec![method],
            },
            template: NativeDeclaration {
                concrete_result: None,
                declaration,
                function: body,
                callable_requirements: vec![],
            },
            table: InterfaceTable {
                declaration: identity(DefinitionKind::Impl, ""),
                name: "script_default".into(),
                generic_params: vec![],
                bounds: vec![],
                trait_type: Ty::Trait(interface(&owner, vec![])),
                for_type: scalar(),
                methods: vec![derived],
                associated_consts: vec![],
                associated_type_families: vec![],
                host_bridge: false,
            },
        }
    }

    fn catalog(&self) -> Result<ProofCatalog<'_>, TypeTransformError> {
        ProofCatalog::new(
            vec![(&self.table).into()],
            vec![],
            [],
            [(self.owner.clone(), &self.contract)],
            [&self.template],
            &CancellationToken::default(),
        )
    }

    fn requirement(&self) -> NativeCallableRequirement {
        let mut member = self.owner.clone();
        member.path.push(DefinitionPathSegment {
            kind: DefinitionKind::Method,
            name: "echo".into(),
            occurrence: 0,
        });
        NativeCallableRequirement {
            receiver: self.table.for_type.clone(),
            interface: match &self.table.trait_type {
                Ty::Trait(ty) => ty.clone(),
                _ => unreachable!(),
            },
            member,
            arguments: vec![],
        }
    }
}

#[test]
fn final_default_selects_the_registered_template_and_concrete_receiver() {
    let fixture = Fixture::new();
    let cancel = CancellationToken::default();
    verify::validate(
        &[
            PublicItem::Trait(fixture.contract.clone()),
            PublicItem::InterfaceTable(Box::new(fixture.table.clone())),
        ],
        &fixture.owner.module,
        &cancel,
    )
    .unwrap();
    let catalog = fixture.catalog().unwrap();
    assert!(verify::interface_methods_match(
        &fixture.table,
        &fixture.contract,
        &|ty| catalog.normalize(ty, &cancel),
        &cancel
    ));
    let selected = catalog
        .select_callable(&fixture.requirement(), &cancel)
        .unwrap()
        .unwrap();
    assert_eq!(selected.instance.declaration, fixture.template.declaration);
    assert_eq!(selected.instance.arguments, [scalar()]);
    assert_eq!(
        selected.implementation,
        fixture.template.function.implementation
    );
    assert_eq!(selected.signature.params, [scalar()]);
    assert_eq!(selected.signature.result, scalar());
}

#[test]
fn default_contracts_reject_missing_templates_wrong_shapes_and_stronger_bounds() {
    let original = Fixture::new();
    assert!(
        ProofCatalog::new(
            vec![(&original.table).into()],
            vec![],
            [],
            [(original.owner.clone(), &original.contract)],
            [],
            &CancellationToken::default()
        )
        .is_err()
    );
    for case in 0..5 {
        let mut fixture = Fixture::new();
        match case {
            0 => fixture.template.function.return_type = Ty::Builtin(BuiltinType::Bool),
            1 => fixture.template.function.params[0].ty = Ty::Builtin(BuiltinType::Bool),
            2 => fixture.template.function.params[0].mutable = true,
            3 => {
                let CallableImplementation::NativeDefault(application) =
                    &mut fixture.contract.methods[0].implementation
                else {
                    unreachable!()
                };
                application.arguments.clear();
            }
            4 => {
                fixture.template.function.bounds[0].constraints = vec![Constraint::Trait(
                    interface(&identity(DefinitionKind::Trait, "Other"), vec![]),
                )]
            }
            _ => unreachable!(),
        }
        assert!(fixture.catalog().is_err(), "{case}");
    }
}

#[test]
fn final_defaults_reject_overrides_and_changed_template_applications() {
    for case in 0..3 {
        let mut fixture = Fixture::new();
        match case {
            0 => fixture.table.methods[0].implementation = CallableImplementation::Script,
            1 => {
                fixture.table.methods[0].implementation =
                    fixture.template.function.implementation.clone()
            }
            2 => {
                let CallableImplementation::NativeDefault(application) =
                    &mut fixture.table.methods[0].implementation
                else {
                    unreachable!()
                };
                application.arguments[0] = Ty::Builtin(BuiltinType::Bool);
            }
            _ => unreachable!(),
        }
        let catalog = fixture.catalog().unwrap();
        let cancel = CancellationToken::default();
        assert!(
            !verify::interface_methods_match(
                &fixture.table,
                &fixture.contract,
                &|ty| catalog.normalize(ty, &cancel),
                &cancel
            ),
            "{case}"
        );
    }
}

#[test]
fn default_template_obligations_include_declared_parent_traits() {
    let mut fixture = Fixture::new();
    let parent_owner = identity(DefinitionKind::Trait, "Parent");
    let mut parent = fixture.contract.clone();
    parent.name = "Parent".into();
    parent.methods.clear();
    fixture
        .contract
        .supertraits
        .push(interface(&parent_owner, vec![]));
    fixture.template.function.bounds[0].constraints =
        vec![Constraint::Trait(interface(&parent_owner, vec![]))];
    let cancel = CancellationToken::default();
    assert!(
        ProofCatalog::new(
            vec![],
            vec![],
            [],
            [
                (fixture.owner.clone(), &fixture.contract),
                (parent_owner.clone(), &parent)
            ],
            [&fixture.template],
            &cancel
        )
        .is_ok()
    );
    fixture.contract.supertraits.clear();
    assert!(
        ProofCatalog::new(
            vec![],
            vec![],
            [],
            [
                (fixture.owner.clone(), &fixture.contract),
                (parent_owner, &parent)
            ],
            [&fixture.template],
            &cancel
        )
        .is_err()
    );
}

#[test]
fn generic_interface_instantiation_preserves_explicit_default_argument_mapping() {
    let mut fixture = Fixture::new();
    let parameter = GenericParam {
        owner: fixture.table.declaration.clone(),
        position: 0,
    };
    fixture.table.generic_params.push(parameter.clone());
    fixture.table.for_type = parameter.as_type();
    fixture.table.methods[0].params[0].ty = parameter.as_type();
    let CallableImplementation::NativeDefault(application) =
        &mut fixture.table.methods[0].implementation
    else {
        unreachable!()
    };
    application.arguments[0] = parameter.as_type();
    let table = fixture.table.instantiate(&[scalar()]).unwrap();
    assert_eq!(table, Fixture::new().table);
    fixture.table = table;
    let selected = fixture
        .catalog()
        .unwrap()
        .select_callable(&fixture.requirement(), &CancellationToken::default())
        .unwrap()
        .unwrap();
    assert_eq!(selected.instance.arguments, [scalar()]);
}

#[test]
fn foreign_self_binders_and_defaults_outside_method_contracts_are_rejected() {
    let mut fixture = Fixture::new();
    let CallableImplementation::NativeDefault(application) =
        &mut fixture.contract.methods[0].implementation
    else {
        unreachable!()
    };
    application.arguments[0] = Ty::SelfType(identity(DefinitionKind::Trait, "Other"));
    assert!(
        verify::validate(
            &[PublicItem::Trait(fixture.contract.clone())],
            &fixture.owner.module,
            &CancellationToken::default()
        )
        .is_err()
    );
    let method = fixture.table.methods[0].clone();
    assert!(
        verify::validate(
            &[PublicItem::Function(method)],
            &fixture.owner.module,
            &CancellationToken::default()
        )
        .is_err()
    );
}

#[test]
fn trait_arguments_and_self_follow_the_declared_template_order() {
    let mut fixture = Fixture::new();
    let item = GenericParam {
        owner: fixture.owner.clone(),
        position: 0,
    };
    fixture.contract.generic_params.push(item.clone());
    fixture.contract.methods[0].params.push(Param {
        name: "other".into(),
        ty: item.as_type(),
        mutable: false,
    });
    fixture.contract.methods[0].return_type = item.as_type();
    let CallableImplementation::NativeDefault(application) =
        &mut fixture.contract.methods[0].implementation
    else {
        unreachable!()
    };
    application.arguments = vec![item.as_type(), Ty::SelfType(fixture.owner.clone())];
    let value = GenericParam {
        owner: fixture.template.declaration.clone(),
        position: 0,
    };
    let receiver = GenericParam {
        owner: fixture.template.declaration.clone(),
        position: 1,
    };
    fixture.template.function.generic_params = vec![value.clone(), receiver.clone()];
    fixture.template.function.params = vec![
        Param {
            name: "receiver".into(),
            ty: receiver.as_type(),
            mutable: false,
        },
        Param {
            name: "value".into(),
            ty: value.as_type(),
            mutable: false,
        },
    ];
    fixture.template.function.return_type = value.as_type();
    fixture.template.function.bounds[0] = GenericBound {
        ty: receiver.as_type(),
        constraints: vec![Constraint::Trait(interface(
            &fixture.owner,
            vec![value.as_type()],
        ))],
    };
    let boolean = Ty::Builtin(BuiltinType::Bool);
    fixture.table.trait_type = Ty::Trait(interface(&fixture.owner, vec![scalar()]));
    fixture.table.for_type = boolean.clone();
    fixture.table.methods[0].params = vec![
        Param {
            name: "self".into(),
            ty: boolean.clone(),
            mutable: false,
        },
        Param {
            name: "other".into(),
            ty: scalar(),
            mutable: false,
        },
    ];
    let CallableImplementation::NativeDefault(application) =
        &mut fixture.table.methods[0].implementation
    else {
        unreachable!()
    };
    application.arguments = vec![scalar(), boolean.clone()];
    let catalog = fixture.catalog().unwrap();
    let cancel = CancellationToken::default();
    assert!(verify::interface_methods_match(
        &fixture.table,
        &fixture.contract,
        &|ty| catalog.normalize(ty, &cancel),
        &cancel
    ));
    let selected = catalog
        .select_callable(&fixture.requirement(), &cancel)
        .unwrap()
        .unwrap();
    assert_eq!(selected.instance.arguments, [scalar(), boolean.clone()]);
    assert_eq!(selected.signature.params, [boolean, scalar()]);
    assert_eq!(selected.signature.result, scalar());
}

#[test]
fn associated_output_maps_to_the_registered_template_result() {
    let mut fixture = Fixture::new();
    let output = associated_type_id(&fixture.owner, "Output");
    fixture.contract.associated_types.push(AssociatedTypeDef {
        generic_params: vec![],
        parameter_bounds: vec![],
        declaration: output.clone(),
        bounds: vec![],
    });
    let projection = Ty::Projection {
        receiver: Box::new(Ty::SelfType(fixture.owner.clone())),
        interface: Box::new(interface(&fixture.owner, vec![])),
        member: output.clone(),
        arguments: vec![],
    };
    fixture.contract.methods[0].return_type = projection.clone();
    let CallableImplementation::NativeDefault(application) =
        &mut fixture.contract.methods[0].implementation
    else {
        unreachable!()
    };
    application.arguments.push(projection);
    let parameter = GenericParam {
        owner: fixture.template.declaration.clone(),
        position: 1,
    };
    fixture
        .template
        .function
        .generic_params
        .push(parameter.clone());
    fixture.template.function.return_type = parameter.as_type();
    let Constraint::Trait(required) = &mut fixture.template.function.bounds[0].constraints[0]
    else {
        unreachable!()
    };
    required
        .associated_types
        .insert(output.clone(), parameter.as_type());
    let Ty::Trait(implemented) = &mut fixture.table.trait_type else {
        unreachable!()
    };
    implemented.associated_types.insert(output, scalar());
    let CallableImplementation::NativeDefault(application) =
        &mut fixture.table.methods[0].implementation
    else {
        unreachable!()
    };
    application.arguments.push(scalar());
    let catalog = fixture.catalog().unwrap();
    let cancel = CancellationToken::default();
    assert!(verify::interface_methods_match(
        &fixture.table,
        &fixture.contract,
        &|ty| catalog.normalize(ty, &cancel),
        &cancel
    ));
    let selected = catalog
        .select_callable(&fixture.requirement(), &cancel)
        .unwrap()
        .unwrap();
    assert_eq!(selected.instance.arguments, [scalar(), scalar()]);
    assert_eq!(selected.signature.result, scalar());

    fixture.template.function.return_type = Ty::Builtin(BuiltinType::Bool);
    assert!(fixture.catalog().is_err());
}

#[test]
fn default_application_transforms_and_decoding_remain_bounded_and_cancellable() {
    let fixture = Fixture::new();
    let CallableImplementation::NativeDefault(application) =
        &fixture.contract.methods[0].implementation
    else {
        unreachable!()
    };
    let cancel = CancellationToken::default();
    let mut substitution = TypeSubstitution::default();
    let receiver = scalar();
    substitution.bind_receiver(&fixture.owner, &receiver);
    assert_eq!(
        application.apply(&substitution, &cancel).unwrap().arguments,
        [scalar()]
    );
    let bytes = bincode::serialize(application).unwrap();
    assert_eq!(
        bincode::deserialize::<NativeDefaultApplication>(&bytes).unwrap(),
        *application
    );
    let oversized = NativeDefaultApplication {
        declaration: application.declaration.clone(),
        arguments: vec![scalar(); 4097],
    };
    assert_eq!(
        oversized.apply(&substitution, &cancel),
        Err(TypeTransformError::LimitExceeded)
    );
    assert!(
        bincode::deserialize::<NativeDefaultApplication>(&bincode::serialize(&oversized).unwrap())
            .is_err()
    );
    let catalog = fixture.catalog().unwrap();
    cancel.cancel();
    assert_eq!(
        application.apply(&substitution, &cancel),
        Err(TypeTransformError::Cancelled)
    );
    let empty = NativeDefaultApplication {
        declaration: application.declaration.clone(),
        arguments: vec![],
    };
    assert_eq!(
        empty.apply(&substitution, &cancel),
        Err(TypeTransformError::Cancelled)
    );
    assert!(matches!(
        catalog.resolve_native_default(application, &cancel),
        Err(TypeTransformError::Cancelled)
    ));
}

#[test]
fn registered_impl_inherits_the_same_checked_default_as_an_executable_table() {
    let fixture = Fixture::new();
    let cancel = CancellationToken::default();
    let native = ImplDecl {
        generic_params: vec![],
        bounds: vec![],
        trait_type: Some(interface(&fixture.owner, vec![])),
        for_type: scalar(),
        methods: vec![],
    };
    let catalog = ProofCatalog::new(
        vec![Implementation::Native {
            declaration: &fixture.table.declaration,
            implementation: &native,
        }],
        vec![],
        [],
        [(fixture.owner.clone(), &fixture.contract)],
        [&fixture.template],
        &cancel,
    )
    .unwrap();
    let requirement = fixture.requirement();
    assert_eq!(
        catalog.select_callable(&requirement, &cancel).unwrap(),
        fixture
            .catalog()
            .unwrap()
            .select_callable(&requirement, &cancel)
            .unwrap(),
    );
}

#[test]
fn registered_generic_impl_checks_nested_obligations_before_selecting_its_default() {
    let fixture = Fixture::new();
    let cancel = CancellationToken::default();
    let declaration = identity(DefinitionKind::Impl, "array");
    let parameter = GenericParam {
        owner: declaration.clone(),
        position: 0,
    };
    let applied = interface(&fixture.owner, vec![]);
    let native = ImplDecl {
        generic_params: vec![parameter.clone()],
        bounds: vec![GenericBound {
            ty: parameter.as_type(),
            constraints: vec![Constraint::Trait(applied.clone())],
        }],
        trait_type: Some(applied.clone()),
        for_type: Ty::Array(Box::new(parameter.as_type()), CollectionAccess::Mutable),
        methods: vec![],
    };
    let catalog = ProofCatalog::new(
        vec![
            Implementation::Interface(&fixture.table),
            Implementation::Native {
                declaration: &declaration,
                implementation: &native,
            },
        ],
        vec![],
        [],
        [(fixture.owner.clone(), &fixture.contract)],
        [&fixture.template],
        &cancel,
    )
    .unwrap();
    let mut requirement = fixture.requirement();
    requirement.receiver = Ty::Array(Box::new(scalar()), CollectionAccess::Mutable);
    assert!(
        catalog
            .holds(&applied, &requirement.receiver, &[], &cancel)
            .unwrap()
    );
    let selected = catalog
        .select_callable(&requirement, &cancel)
        .unwrap()
        .unwrap();
    assert_eq!(
        selected.signature.params,
        vec![requirement.receiver.clone()]
    );
    assert_eq!(
        selected.instance.arguments,
        vec![requirement.receiver.clone()]
    );
    requirement.receiver = Ty::Array(
        Box::new(Ty::Builtin(BuiltinType::Bool)),
        CollectionAccess::Mutable,
    );
    assert!(
        !catalog
            .holds(&applied, &requirement.receiver, &[], &cancel)
            .unwrap()
    );
    assert!(
        catalog
            .select_callable(&requirement, &cancel)
            .unwrap()
            .is_none()
    );
}

#[test]
fn hidden_associated_arguments_are_rechecked_against_linked_contracts() {
    let mut fixture = Fixture::new();
    let output_owner = identity(DefinitionKind::Trait, "Source");
    let output = associated_type_id(&output_owner, "Output");
    let mut output_contract = fixture.contract.clone();
    output_contract.name = "Source".into();
    output_contract.methods.clear();
    output_contract.associated_types.push(AssociatedTypeDef {
        declaration: output.clone(),
        generic_params: vec![],
        parameter_bounds: vec![],
        bounds: vec![],
    });
    let mut output_table = fixture.table.clone();
    output_table.declaration = identity(DefinitionKind::Impl, "source");
    output_table.methods.clear();
    let mut output_interface = interface(&output_owner, vec![]);
    output_interface
        .associated_types
        .insert(output.clone(), scalar());
    output_table.trait_type = Ty::Trait(output_interface);
    let CallableImplementation::NativeDefault(application) =
        &mut fixture.contract.methods[0].implementation
    else {
        unreachable!()
    };
    application.arguments.push(Ty::Projection {
        receiver: Box::new(Ty::SelfType(fixture.owner.clone())),
        interface: Box::new(interface(&output_owner, vec![])),
        member: output,
        arguments: vec![],
    });
    fixture.template.function.generic_params.push(GenericParam {
        owner: fixture.template.declaration.clone(),
        position: 1,
    });
    let CallableImplementation::NativeDefault(application) =
        &mut fixture.table.methods[0].implementation
    else {
        unreachable!()
    };
    application.arguments.push(scalar());
    let cancel = CancellationToken::default();
    for correct in [true, false] {
        if !correct {
            let CallableImplementation::NativeDefault(application) =
                &mut fixture.table.methods[0].implementation
            else {
                unreachable!()
            };
            application.arguments[1] = Ty::Builtin(BuiltinType::Bool);
        }
        // The local check cannot resolve the Source implementation. The linked
        // check must accept the selected output and reject a forged argument.
        assert!(verify::interface_contract_matches(
            &fixture.table,
            &fixture.contract,
            &cancel
        ));
        let catalog = ProofCatalog::new(
            vec![(&fixture.table).into(), (&output_table).into()],
            vec![],
            [],
            [
                (fixture.owner.clone(), &fixture.contract),
                (output_owner.clone(), &output_contract),
            ],
            [&fixture.template],
            &cancel,
        )
        .unwrap();
        assert_eq!(
            verify::interface_methods_match(
                &fixture.table,
                &fixture.contract,
                &|ty| catalog.normalize(ty, &cancel),
                &cancel,
            ),
            correct
        );
    }
}

#[test]
fn unspecified_associated_output_proves_only_its_own_projection() {
    let fixture = Fixture::new();
    let mut contract = fixture.contract.clone();
    contract.methods.clear();
    let output = associated_type_id(&fixture.owner, "Output");
    contract.associated_types.push(AssociatedTypeDef {
        declaration: output.clone(),
        generic_params: vec![],
        parameter_bounds: vec![],
        bounds: vec![],
    });
    let cancel = CancellationToken::default();
    let catalog = ProofCatalog::new(
        vec![],
        vec![],
        [],
        [(fixture.owner.clone(), &contract)],
        [],
        &cancel,
    )
    .unwrap();
    let receiver = GenericParam {
        owner: identity(DefinitionKind::Function, "caller"),
        position: 0,
    }
    .as_type();
    let available = interface(&fixture.owner, vec![]);
    let assumptions = [GenericBound {
        ty: receiver.clone(),
        constraints: vec![Constraint::Trait(available.clone())],
    }];
    let projection = Ty::Projection {
        receiver: Box::new(receiver.clone()),
        interface: Box::new(available.clone()),
        member: output.clone(),
        arguments: vec![],
    };
    let proves = |value| {
        let mut required = available.clone();
        required.associated_types.insert(output.clone(), value);
        catalog
            .holds(&required, &receiver, &assumptions, &cancel)
            .unwrap()
    };
    assert!(proves(projection.clone()));
    assert!(!proves(scalar()));
    for variant in 0..4 {
        let mut forged = projection.clone();
        let Ty::Projection {
            receiver,
            interface,
            member,
            arguments,
        } = &mut forged
        else {
            unreachable!()
        };
        match variant {
            0 => **receiver = scalar(),
            1 => *member = associated_type_id(&fixture.owner, "Missing"),
            2 => interface.declaration = identity(DefinitionKind::Trait, "Other"),
            _ => arguments.push(scalar()),
        }
        assert!(!proves(forged));
    }
    let mut constrained = available.clone();
    constrained
        .associated_types
        .insert(output.clone(), scalar());
    let concrete_assumption = [GenericBound {
        ty: receiver.clone(),
        constraints: vec![Constraint::Trait(constrained.clone())],
    }];
    assert!(
        catalog
            .holds(&constrained, &receiver, &concrete_assumption, &cancel)
            .unwrap()
    );
    constrained
        .associated_types
        .insert(output, Ty::Builtin(BuiltinType::Bool));
    assert!(
        !catalog
            .holds(&constrained, &receiver, &concrete_assumption, &cancel)
            .unwrap()
    );
}
