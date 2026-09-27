use super::*;
use kagari_abi::scalar::BuiltinType;
use kagari_abi::types::{
    AssociatedTypeAbi, AssociatedTypeFamilyAbi, ConstraintAbi, InterfaceTableAbi,
};
use kagari_common::identity::{ModuleIdentity, associated_type_id};

fn id(kind: DefinitionKind, name: &str) -> DefinitionId {
    DefinitionId {
        module: ModuleIdentity::single_file("linked.kgr"),
        path: vec![DefinitionPathSegment {
            kind,
            name: name.into(),
            occurrence: 0,
        }],
    }
}
fn applied(name: &str) -> NominalAbiType {
    NominalAbiType {
        declaration: id(DefinitionKind::Trait, name),
        arguments: vec![],
        associated_types: Default::default(),
    }
}
fn record(name: &str) -> TraitAbi {
    TraitAbi {
        name: name.into(),
        generic_params: vec![],
        bounds: vec![],
        methods: vec![],
        default_methods: vec![],
        associated_consts: vec![],
        associated_types: vec![],
        supertraits: vec![],
    }
}
fn table(name: &str, interface: NominalAbiType) -> InterfaceTableAbi {
    InterfaceTableAbi {
        declaration: id(DefinitionKind::Impl, name),
        name: name.into(),
        generic_params: vec![],
        bounds: vec![],
        trait_type: AbiType::Trait(interface),
        for_type: AbiType::Builtin(BuiltinType::I32),
        methods: vec![],
        associated_consts: vec![],
        associated_type_families: vec![],
        host_bridge: false,
        native_bridge: false,
    }
}
fn module(items: Vec<PublicAbiItem>) -> BytecodeModule {
    BytecodeModule {
        identity: ModuleIdentity::single_file("linked.kgr"),
        public_items: items,
        ..Default::default()
    }
}

#[test]
fn linked_associated_bounds_reject_corrupted_outputs_and_missing_parent_implementations() {
    let mut declaration = record("Read");
    let mut interface = applied("Read");
    let member = associated_type_id(&interface.declaration, "Item");
    declaration.associated_types.push(AssociatedTypeAbi {
        declaration: member.clone(),
        generic_params: vec![],
        parameter_bounds: vec![],
        bounds: vec![ConstraintAbi::Trait(intrinsic::applied(
            StandardTrait::Hash,
            vec![],
        ))],
    });
    interface
        .associated_types
        .insert(member.clone(), AbiType::Builtin(BuiltinType::I32));
    let implementation = table("read", interface);
    let mut module = module(vec![
        PublicAbiItem::Trait(declaration.clone()),
        PublicAbiItem::InterfaceTable(Box::new(implementation.clone())),
    ]);
    assert!(trait_bounds_match(&module, &[&module], None));
    let PublicAbiItem::InterfaceTable(corrupt) = &mut module.public_items[1] else {
        unreachable!()
    };
    let AbiType::Trait(corrupt_interface) = &mut corrupt.trait_type else {
        unreachable!()
    };
    corrupt_interface
        .associated_types
        .insert(member, AbiType::Builtin(BuiltinType::F32));
    assert!(!trait_bounds_match(&module, &[&module], None));
    declaration.supertraits.push(applied("Parent"));
    module.public_items = vec![
        PublicAbiItem::Trait(declaration),
        PublicAbiItem::Trait(record("Parent")),
        PublicAbiItem::InterfaceTable(Box::new(implementation)),
    ];
    assert!(!trait_bounds_match(&module, &[&module], None));
    module
        .public_items
        .push(PublicAbiItem::InterfaceTable(Box::new(table(
            "parent",
            applied("Parent"),
        ))));
    assert!(trait_bounds_match(&module, &[&module], None));
}

#[test]
fn linked_family_bounds_use_declared_input_assumptions_and_check_unused_projections() {
    let interface = applied("Family");
    let member = associated_type_id(&interface.declaration, "Item");
    let parameter = GenericParameterAbi {
        owner: member.clone(),
        position: 0,
    };
    let hash = ConstraintAbi::Trait(intrinsic::applied(StandardTrait::Hash, vec![]));
    let mut declaration = record("Family");
    declaration.associated_types.push(AssociatedTypeAbi {
        declaration: member.clone(),
        generic_params: vec![parameter.clone()],
        parameter_bounds: vec![GenericBoundAbi {
            ty: parameter.as_type(),
            constraints: vec![hash.clone()],
        }],
        bounds: vec![hash],
    });
    let mut implementation = table("family", interface.clone());
    let input = GenericParameterAbi {
        owner: associated_type_id(&implementation.declaration, "Item"),
        position: 0,
    };
    implementation
        .associated_type_families
        .push(AssociatedTypeFamilyAbi {
            declaration: member.clone(),
            generic_params: vec![input.clone()],
            bounds: vec![],
            value: input.as_type(),
        });
    let mut module = module(vec![
        PublicAbiItem::Trait(declaration),
        PublicAbiItem::InterfaceTable(Box::new(implementation)),
    ]);
    assert!(trait_bounds_match(&module, &[&module], None));
    let PublicAbiItem::InterfaceTable(table) = &mut module.public_items[1] else {
        unreachable!()
    };
    table.associated_type_families[0].value = AbiType::Projection {
        receiver: Box::new(table.for_type.clone()),
        interface: Box::new(interface),
        member,
        arguments: vec![AbiType::Builtin(BuiltinType::F32)],
    };
    assert!(!trait_bounds_match(&module, &[&module], None));
}
