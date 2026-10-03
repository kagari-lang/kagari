use super::*;
use kagari_common::identity::{ModuleIdentity, associated_type_id};
use kagari_contract::{
    language::primitive,
    scalar::BuiltinType,
    types::{AssociatedTypeDef, AssociatedTypeFamily, Constraint, InterfaceTable},
};

fn id(kind: DefinitionKind, name: &str) -> DefinitionPath {
    DefinitionPath {
        module: ModuleIdentity::single_file("linked.kgr"),
        path: vec![DefinitionPathSegment {
            kind,
            name: name.into(),
            occurrence: 0,
        }],
    }
}

fn applied(name: &str) -> NominalTy {
    NominalTy {
        declaration: id(DefinitionKind::Trait, name),
        arguments: vec![],
        associated_types: Default::default(),
    }
}

fn record(name: &str) -> TraitDef {
    TraitDef {
        name: name.into(),
        generic_params: vec![],
        bounds: vec![],
        methods: vec![],
        associated_consts: vec![],
        associated_types: vec![],
        supertraits: vec![],
    }
}

fn table(name: &str, interface: NominalTy) -> InterfaceTable {
    InterfaceTable {
        declaration: id(DefinitionKind::Impl, name),
        name: name.into(),
        generic_params: vec![],
        bounds: vec![],
        trait_type: Ty::Trait(interface),
        for_type: Ty::Builtin(BuiltinType::I32),
        methods: vec![],
        associated_consts: vec![],
        associated_type_families: vec![],
        host_bridge: false,
    }
}

fn module(items: Vec<PublicItem>) -> BytecodeModule {
    BytecodeModule {
        identity: ModuleIdentity::single_file("linked.kgr"),
        public_items: items,
        ..Default::default()
    }
}

fn with_hash_bounds(module: &BytecodeModule) -> bool {
    let protocol = BytecodeModule {
        identity: primitive::applied(Protocol::Hash, vec![])
            .declaration
            .module,
        public_items: vec![PublicItem::Trait(record("Hash"))],
        ..Default::default()
    };
    verify_trait_bounds(module, &[module, &protocol], None).is_ok()
}

#[test]
fn linked_associated_bounds_reject_corrupted_outputs_and_missing_parent_implementations() {
    let mut declaration = record("Read");
    let mut interface = applied("Read");
    let member = associated_type_id(&interface.declaration, "Item");
    declaration.associated_types.push(AssociatedTypeDef {
        declaration: member.clone(),
        generic_params: vec![],
        parameter_bounds: vec![],
        bounds: vec![Constraint::Trait(primitive::applied(
            Protocol::Hash,
            vec![],
        ))],
    });
    interface
        .associated_types
        .insert(member.clone(), Ty::Builtin(BuiltinType::I32));
    let implementation = table("read", interface);
    let mut module = module(vec![
        PublicItem::Trait(declaration.clone()),
        PublicItem::InterfaceTable(Box::new(implementation.clone())),
    ]);
    assert!(with_hash_bounds(&module));
    let PublicItem::InterfaceTable(corrupt) = &mut module.public_items[1] else {
        unreachable!()
    };
    let Ty::Trait(corrupt_interface) = &mut corrupt.trait_type else {
        unreachable!()
    };
    corrupt_interface
        .associated_types
        .insert(member, Ty::Builtin(BuiltinType::F32));
    assert!(!with_hash_bounds(&module));
    declaration.supertraits.push(applied("Parent"));
    module.public_items = vec![
        PublicItem::Trait(declaration),
        PublicItem::Trait(record("Parent")),
        PublicItem::InterfaceTable(Box::new(implementation)),
    ];
    assert!(!with_hash_bounds(&module));
    module
        .public_items
        .push(PublicItem::InterfaceTable(Box::new(table(
            "parent",
            applied("Parent"),
        ))));
    assert!(with_hash_bounds(&module));
}

#[test]
fn linked_family_bounds_use_declared_input_assumptions_and_check_unused_projections() {
    let interface = applied("Family");
    let member = associated_type_id(&interface.declaration, "Item");
    let parameter = GenericParam {
        owner: member.clone(),
        position: 0,
    };
    let hash = Constraint::Trait(primitive::applied(Protocol::Hash, vec![]));
    let mut declaration = record("Family");
    declaration.associated_types.push(AssociatedTypeDef {
        declaration: member.clone(),
        generic_params: vec![parameter.clone()],
        parameter_bounds: vec![GenericBound {
            ty: parameter.as_type(),
            constraints: vec![hash.clone()],
        }],
        bounds: vec![hash],
    });
    let mut implementation = table("family", interface.clone());
    let input = GenericParam {
        owner: associated_type_id(&implementation.declaration, "Item"),
        position: 0,
    };
    implementation
        .associated_type_families
        .push(AssociatedTypeFamily {
            declaration: member.clone(),
            generic_params: vec![input.clone()],
            bounds: vec![],
            value: input.as_type(),
        });
    let mut module = module(vec![
        PublicItem::Trait(declaration),
        PublicItem::InterfaceTable(Box::new(implementation)),
    ]);
    assert!(with_hash_bounds(&module));
    let PublicItem::InterfaceTable(table) = &mut module.public_items[1] else {
        unreachable!()
    };
    table.associated_type_families[0].value = Ty::Projection {
        receiver: Box::new(table.for_type.clone()),
        interface: Box::new(interface),
        member,
        arguments: vec![Ty::Builtin(BuiltinType::F32)],
    };
    assert!(!with_hash_bounds(&module));
}
