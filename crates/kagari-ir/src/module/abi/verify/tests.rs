use super::*;
use crate::bytecode::{BytecodeVerificationError, verify_module};
use crate::module::abi::ParameterAbi;

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
            kagari_hir::builtin::surface::StandardTypeConstraint::HashKey,
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

#[test]
fn interface_tables_require_distinct_local_impl_identities() {
    let original = crate::tests::common::bytecode_ok(
        "struct Player { val value: i32 } pub trait Display { fn show(self) -> i32; } impl Display for Player { fn show(self) -> i32 { self.value } } fn main() -> i32 { 1 }",
    );
    let table_index = original
        .public_items
        .iter()
        .position(|item| matches!(item, PublicAbiItem::InterfaceTable(_)))
        .expect("checked interface table");
    let PublicAbiItem::InterfaceTable(table) = &original.public_items[table_index] else {
        unreachable!()
    };
    assert_eq!(table.declaration.module, original.identity);
    assert_eq!(table.declaration.path.len(), 1);
    assert_eq!(table.declaration.path[0].kind, DefinitionKind::Impl);
    assert!(table.declaration.path[0].name.is_empty());
    for corruption in 0..3 {
        let mut module = original.clone();
        let PublicAbiItem::InterfaceTable(table) = &mut module.public_items[table_index] else {
            unreachable!()
        };
        match corruption {
            0 => table.declaration.module.package.0 = "foreign".into(),
            1 => table.declaration.path[0].kind = DefinitionKind::Trait,
            _ => table.declaration.path[0].name = "fabricated".into(),
        }
        assert!(matches!(
            verify_module(&module),
            Err(BytecodeVerificationError::InvalidPublicAbi)
        ));
    }
    for corruption in 0..3 {
        let mut module = original.clone();
        let PublicAbiItem::InterfaceTable(table) = &mut module.public_items[table_index] else {
            unreachable!()
        };
        match corruption {
            0 => table.methods[0].return_type = AbiType::Builtin(BuiltinType::Bool),
            1 => table.methods[0].params[0].mutable = true,
            _ => table.methods[0].params[0].ty = AbiType::Builtin(BuiltinType::I32),
        }
        assert!(matches!(
            verify_module(&module),
            Err(BytecodeVerificationError::InvalidPublicAbi)
        ));
    }
    let mut wrong_trait = original.clone();
    let PublicAbiItem::InterfaceTable(table) = &mut wrong_trait.public_items[table_index] else {
        unreachable!()
    };
    let AbiType::Trait(reference) = &mut table.trait_type else {
        unreachable!()
    };
    reference.declaration.path[0].occurrence = 1;
    assert!(matches!(
        verify_module(&wrong_trait),
        Err(BytecodeVerificationError::InvalidPublicAbi)
    ));
    let mut duplicate = original.clone();
    duplicate
        .public_items
        .push(duplicate.public_items[table_index].clone());
    assert!(matches!(
        verify_module(&duplicate),
        Err(BytecodeVerificationError::InvalidPublicAbi)
    ));
    for corruption in 0..3 {
        let mut module = original.clone();
        let PublicAbiItem::InterfaceTable(table) = &mut module.public_items[table_index] else {
            unreachable!()
        };
        match corruption {
            0 => table.methods.clear(),
            1 => table.methods[0].name = "other".into(),
            _ => table.methods.push(table.methods[0].clone()),
        }
        assert!(matches!(
            verify_module(&module),
            Err(BytecodeVerificationError::InvalidPublicAbi)
        ));
    }
}

#[test]
fn public_signatures_reject_foreign_parameters_invalid_arity_and_escaped_self() {
    let original = crate::tests::common::bytecode_ok(
        "pub fn plain() -> i32 { 1 } pub trait Identity { fn same<T: Eq + Hash + PartialEq>(self, value: T) -> T; }",
    );
    for corruption in 0..8 {
        let mut module = original.clone();
        let (functions, traits) = module.public_items.split_at_mut(1);
        let PublicAbiItem::Function(function) = &mut functions[0] else {
            panic!("public function")
        };
        let PublicAbiItem::Trait(interface) = &mut traits[0] else {
            panic!("public trait")
        };
        match corruption {
            0 => {
                interface.methods[0].generic_params[0]
                    .owner
                    .module
                    .package
                    .0 = "foreign".into()
            }
            1 => {
                if let AbiType::Parameter { position, .. } = &mut interface.methods[0].params[1].ty
                {
                    *position = 99;
                }
            }
            2 => {
                function.return_type = AbiType::SelfType(owner(
                    &module.identity,
                    &[],
                    DefinitionKind::Trait,
                    "Identity",
                ))
            }
            3 => {
                function.return_type = AbiType::StandardEnum {
                    kind: StandardEnumKind::Result,
                    args: vec![AbiType::Builtin(BuiltinType::I32)],
                }
            }
            4 => {
                function.return_type = AbiType::Struct(NominalAbiType {
                    associated_types: Default::default(),
                    declaration: owner(&module.identity, &[], DefinitionKind::Trait, "Identity"),
                    arguments: vec![],
                })
            }
            5 => function.generic_params.push(GenericParameterAbi {
                owner: owner(&module.identity, &[], DefinitionKind::Function, "plain"),
                position: 0,
            }),
            6 => interface.methods[0].bounds[0].constraints.reverse(),
            _ => {
                let constraint = interface.methods[0].bounds[0].constraints[0].clone();
                interface.methods[0].bounds[0]
                    .constraints
                    .insert(0, constraint);
            }
        }
        assert!(
            matches!(
                verify_module(&module),
                Err(BytecodeVerificationError::InvalidPublicAbi)
            ),
            "corruption {corruption}"
        );
    }
    let mut module = original;
    let PublicAbiItem::Trait(interface) = &mut module.public_items[1] else {
        panic!("public trait")
    };
    interface.methods[0].return_type =
        AbiType::SelfType(owner(&module.identity, &[], DefinitionKind::Trait, "Other"));
    assert!(matches!(
        verify_module(&module),
        Err(BytecodeVerificationError::InvalidPublicAbi)
    ));
}
