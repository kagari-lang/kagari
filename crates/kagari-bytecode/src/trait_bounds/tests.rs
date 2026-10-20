use super::*;
use kagari_common::identity::{ModuleIdentity, map::DefinitionContext};
use kagari_contract::{
    language::primitive,
    types::{InterfaceTable, identity::collect_language},
};
use kagari_types::{
    declaration::{AssociatedTypeDef, AssociatedTypeFamily},
    language,
    scalar::BuiltinType,
};

fn id(context: &DefinitionContext, kind: DefinitionKind, name: &str) -> DefinitionId {
    let root = context
        .intern_root(&ModuleIdentity::single_file("linked.kgr"))
        .unwrap();
    context.intern_child(root, kind, name, 0).unwrap()
}

fn applied(context: &DefinitionContext, name: &str) -> NominalTy {
    NominalTy {
        declaration: id(context, DefinitionKind::Trait, name),
        arguments: vec![],
        associated_types: Default::default(),
    }
}

fn record(name: &str) -> TraitDef {
    TraitDef {
        conversion_adapter: None,
        name: name.into(),
        generic_params: vec![],
        bounds: vec![],
        methods: vec![],
        associated_consts: vec![],
        associated_types: vec![],
        supertraits: vec![],
    }
}

fn table(context: &DefinitionContext, name: &str, interface: NominalTy) -> InterfaceTable {
    InterfaceTable {
        declaration: id(context, DefinitionKind::Impl, name),
        name: name.into(),
        generic_params: vec![],
        bounds: vec![],
        trait_type: Ty::Trait(interface),
        for_type: Ty::Builtin(BuiltinType::I32),
        methods: vec![],
        associated_consts: vec![],
        associated_type_families: vec![],
        bridge: None,
    }
}

fn module(items: Vec<PublicItem>) -> BytecodeModule {
    BytecodeModule {
        identity: ModuleIdentity::single_file("linked.kgr"),
        public_items: items,
        ..Default::default()
    }
}

fn with_hash_bounds(context: &DefinitionContext, module: &BytecodeModule) -> bool {
    let protocol = BytecodeModule {
        identity: language::identity(Protocol::Hash).module,
        public_items: vec![PublicItem::Trait(record("Hash"))],
        ..Default::default()
    };
    let cancel = CancellationToken::default();
    let Ok(resources) =
        VerificationResources::new(vec![module, &protocol], &context.snapshot(), &cancel)
    else {
        return false;
    };
    let Ok(scope) = resources.scope(&[0, 1], resources.receiver_owners(&[0, 1]), &cancel) else {
        return false;
    };
    verify_trait_bounds(0, None, &resources, &scope, &cancel).is_ok()
}

#[test]
fn linked_associated_bounds_reject_corrupted_outputs_and_missing_parent_implementations() {
    let context = DefinitionContext::new().unwrap();
    collect_language(&context, &Default::default()).unwrap();
    let mut declaration = record("Read");
    let mut interface = applied(&context, "Read");
    let member = context
        .intern_child(
            interface.declaration,
            DefinitionKind::AssociatedType,
            "Item",
            0,
        )
        .unwrap();
    declaration.associated_types.push(AssociatedTypeDef {
        declaration: member,
        generic_params: vec![],
        parameter_bounds: vec![],
        bounds: vec![primitive::applied(Protocol::Hash, vec![], &context.snapshot()).unwrap()],
    });
    interface
        .associated_types
        .insert(member, Ty::Builtin(BuiltinType::I32));
    let implementation = table(&context, "read", interface);
    let mut module = module(vec![
        PublicItem::Trait(declaration.clone()),
        PublicItem::InterfaceTable(Box::new(implementation.clone())),
    ]);
    assert!(with_hash_bounds(&context, &module));
    let PublicItem::InterfaceTable(corrupt) = &mut module.public_items[1] else {
        unreachable!()
    };
    let Ty::Trait(corrupt_interface) = &mut corrupt.trait_type else {
        unreachable!()
    };
    corrupt_interface
        .associated_types
        .insert(member, Ty::Builtin(BuiltinType::F32));
    assert!(!with_hash_bounds(&context, &module));
    declaration.supertraits.push(applied(&context, "Parent"));
    module.public_items = vec![
        PublicItem::Trait(declaration),
        PublicItem::Trait(record("Parent")),
        PublicItem::InterfaceTable(Box::new(implementation)),
    ];
    assert!(!with_hash_bounds(&context, &module));
    module
        .public_items
        .push(PublicItem::InterfaceTable(Box::new(table(
            &context,
            "parent",
            applied(&context, "Parent"),
        ))));
    assert!(with_hash_bounds(&context, &module));
}

#[test]
fn linked_family_bounds_use_declared_input_assumptions_and_check_unused_projections() {
    let context = DefinitionContext::new().unwrap();
    collect_language(&context, &Default::default()).unwrap();
    let interface = applied(&context, "Family");
    let member = context
        .intern_child(
            interface.declaration,
            DefinitionKind::AssociatedType,
            "Item",
            0,
        )
        .unwrap();
    let parameter = GenericParam {
        owner: member,
        position: 0,
    };
    let hash = primitive::applied(Protocol::Hash, vec![], &context.snapshot()).unwrap();
    let mut declaration = record("Family");
    declaration.associated_types.push(AssociatedTypeDef {
        declaration: member,
        generic_params: vec![parameter.clone()],
        parameter_bounds: vec![GenericBound {
            ty: parameter.as_type(),
            constraints: vec![hash.clone()],
        }],
        bounds: vec![hash],
    });
    let mut implementation = table(&context, "family", interface.clone());
    let input = GenericParam {
        owner: context
            .intern_child(
                implementation.declaration,
                DefinitionKind::AssociatedType,
                "Item",
                0,
            )
            .unwrap(),
        position: 0,
    };
    implementation
        .associated_type_families
        .push(AssociatedTypeFamily {
            declaration: member,
            generic_params: vec![input.clone()],
            bounds: vec![],
            value: input.as_type(),
        });
    let mut module = module(vec![
        PublicItem::Trait(declaration),
        PublicItem::InterfaceTable(Box::new(implementation)),
    ]);
    assert!(with_hash_bounds(&context, &module));
    let PublicItem::InterfaceTable(table) = &mut module.public_items[1] else {
        unreachable!()
    };
    table.associated_type_families[0].value = Ty::Projection {
        receiver: Box::new(table.for_type.clone()),
        interface: Box::new(interface),
        member,
        arguments: vec![Ty::Builtin(BuiltinType::F32)],
    };
    assert!(!with_hash_bounds(&context, &module));
}
