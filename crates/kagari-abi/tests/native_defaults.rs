//! Portable defaults select actual native templates without source analysis.
use kagari_abi::{
    callable::{CallableImplementation, MethodPolicy, NativeDefaultApplication},
    declaration::ImplDecl,
    native_import::callables::NativeCallableRequirement,
    scalar::BuiltinType,
    types::{
        AbiType, AssociatedTypeAbi, ConstraintAbi, FunctionAbi, GenericBoundAbi,
        GenericParameterAbi, InterfaceTableAbi, NativeDeclaration, NominalAbiType, ParameterAbi,
        PublicAbiItem, TraitAbi,
        proofs::{ProofCatalog, implementation::Implementation},
        substitution::{TypeSubstitution, TypeTransformError},
        verify,
    },
};
use kagari_common::{
    cancellation::CancellationToken,
    collection::CollectionAccess,
    identity::{
        DefinitionId, DefinitionKind, DefinitionPathSegment, ModuleIdentity, PackageId,
        associated_type_id,
    },
};
use std::collections::BTreeMap;

fn identity(kind: DefinitionKind, name: &str) -> DefinitionId {
    DefinitionId {
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

fn scalar() -> AbiType {
    AbiType::Builtin(BuiltinType::I32)
}

fn interface(owner: &DefinitionId, arguments: Vec<AbiType>) -> NominalAbiType {
    NominalAbiType {
        declaration: owner.clone(),
        arguments,
        associated_types: BTreeMap::new(),
    }
}

fn function(name: &str, implementation: CallableImplementation, parameter: AbiType) -> FunctionAbi {
    FunctionAbi {
        name: name.into(),
        implementation,
        method_policy: MethodPolicy::default(),
        generic_params: vec![],
        bounds: vec![],
        params: vec![ParameterAbi {
            name: "value".into(),
            ty: parameter,
            mutable: false,
        }],
        return_type: scalar(),
    }
}

struct Fixture {
    owner: DefinitionId,
    contract: TraitAbi,
    template: NativeDeclaration,
    table: InterfaceTableAbi,
}

impl Fixture {
    fn new() -> Self {
        let owner = identity(DefinitionKind::Trait, "Echo");
        let declaration = identity(DefinitionKind::Function, "default_echo");
        let parameter = GenericParameterAbi {
            owner: declaration.clone(),
            position: 0,
        };
        let mut body = function(
            "default_echo",
            CallableImplementation::Native(identity(DefinitionKind::Function, "handler")),
            parameter.as_type(),
        );
        body.generic_params.push(parameter.clone());
        body.bounds.push(GenericBoundAbi {
            ty: parameter.as_type(),
            constraints: vec![ConstraintAbi::Trait(interface(&owner, vec![]))],
        });
        let application = NativeDefaultApplication {
            declaration: declaration.clone(),
            arguments: vec![AbiType::SelfType(owner.clone())],
        };
        let mut method = function(
            "echo",
            CallableImplementation::NativeDefault(application),
            AbiType::SelfType(owner.clone()),
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
            contract: TraitAbi {
                name: "Echo".into(),
                generic_params: vec![],
                bounds: vec![],
                supertraits: vec![],
                associated_types: vec![],
                associated_consts: vec![],
                methods: vec![method],
            },
            template: NativeDeclaration {
                declaration,
                function: body,
                callable_requirements: vec![],
            },
            table: InterfaceTableAbi {
                declaration: identity(DefinitionKind::Impl, ""),
                name: "script_default".into(),
                generic_params: vec![],
                bounds: vec![],
                trait_type: AbiType::Trait(interface(&owner, vec![])),
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
                AbiType::Trait(ty) => ty.clone(),
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
            PublicAbiItem::Trait(fixture.contract.clone()),
            PublicAbiItem::InterfaceTable(Box::new(fixture.table.clone())),
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
            0 => fixture.template.function.return_type = AbiType::Builtin(BuiltinType::Bool),
            1 => fixture.template.function.params[0].ty = AbiType::Builtin(BuiltinType::Bool),
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
                fixture.template.function.bounds[0].constraints = vec![ConstraintAbi::Trait(
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
                application.arguments[0] = AbiType::Builtin(BuiltinType::Bool);
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
        vec![ConstraintAbi::Trait(interface(&parent_owner, vec![]))];
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
    let parameter = GenericParameterAbi {
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
    application.arguments[0] = AbiType::SelfType(identity(DefinitionKind::Trait, "Other"));
    assert!(
        verify::validate(
            &[PublicAbiItem::Trait(fixture.contract.clone())],
            &fixture.owner.module,
            &CancellationToken::default()
        )
        .is_err()
    );
    let method = fixture.table.methods[0].clone();
    assert!(
        verify::validate(
            &[PublicAbiItem::Function(method)],
            &fixture.owner.module,
            &CancellationToken::default()
        )
        .is_err()
    );
}

#[test]
fn trait_arguments_and_self_follow_the_declared_template_order() {
    let mut fixture = Fixture::new();
    let item = GenericParameterAbi {
        owner: fixture.owner.clone(),
        position: 0,
    };
    fixture.contract.generic_params.push(item.clone());
    fixture.contract.methods[0].params.push(ParameterAbi {
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
    application.arguments = vec![item.as_type(), AbiType::SelfType(fixture.owner.clone())];
    let value = GenericParameterAbi {
        owner: fixture.template.declaration.clone(),
        position: 0,
    };
    let receiver = GenericParameterAbi {
        owner: fixture.template.declaration.clone(),
        position: 1,
    };
    fixture.template.function.generic_params = vec![value.clone(), receiver.clone()];
    fixture.template.function.params = vec![
        ParameterAbi {
            name: "receiver".into(),
            ty: receiver.as_type(),
            mutable: false,
        },
        ParameterAbi {
            name: "value".into(),
            ty: value.as_type(),
            mutable: false,
        },
    ];
    fixture.template.function.return_type = value.as_type();
    fixture.template.function.bounds[0] = GenericBoundAbi {
        ty: receiver.as_type(),
        constraints: vec![ConstraintAbi::Trait(interface(
            &fixture.owner,
            vec![value.as_type()],
        ))],
    };
    let boolean = AbiType::Builtin(BuiltinType::Bool);
    fixture.table.trait_type = AbiType::Trait(interface(&fixture.owner, vec![scalar()]));
    fixture.table.for_type = boolean.clone();
    fixture.table.methods[0].params = vec![
        ParameterAbi {
            name: "self".into(),
            ty: boolean.clone(),
            mutable: false,
        },
        ParameterAbi {
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
    fixture.contract.associated_types.push(AssociatedTypeAbi {
        generic_params: vec![],
        parameter_bounds: vec![],
        declaration: output.clone(),
        bounds: vec![],
    });
    let projection = AbiType::Projection {
        receiver: Box::new(AbiType::SelfType(fixture.owner.clone())),
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
    let parameter = GenericParameterAbi {
        owner: fixture.template.declaration.clone(),
        position: 1,
    };
    fixture
        .template
        .function
        .generic_params
        .push(parameter.clone());
    fixture.template.function.return_type = parameter.as_type();
    let ConstraintAbi::Trait(required) = &mut fixture.template.function.bounds[0].constraints[0]
    else {
        unreachable!()
    };
    required
        .associated_types
        .insert(output.clone(), parameter.as_type());
    let AbiType::Trait(implemented) = &mut fixture.table.trait_type else {
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

    fixture.template.function.return_type = AbiType::Builtin(BuiltinType::Bool);
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
    let parameter = GenericParameterAbi {
        owner: declaration.clone(),
        position: 0,
    };
    let applied = interface(&fixture.owner, vec![]);
    let native = ImplDecl {
        generic_params: vec![parameter.clone()],
        bounds: vec![GenericBoundAbi {
            ty: parameter.as_type(),
            constraints: vec![ConstraintAbi::Trait(applied.clone())],
        }],
        trait_type: Some(applied.clone()),
        for_type: AbiType::Array(Box::new(parameter.as_type()), CollectionAccess::Mutable),
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
    requirement.receiver = AbiType::Array(Box::new(scalar()), CollectionAccess::Mutable);
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
    requirement.receiver = AbiType::Array(
        Box::new(AbiType::Builtin(BuiltinType::Bool)),
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
