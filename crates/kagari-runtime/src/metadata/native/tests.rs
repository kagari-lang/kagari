use super::MetadataSnapshot;
use crate::{
    metadata::{MethodOrigin, TypeKind, TypeRegistration, TypeRegistry},
    native::catalog::{DeclarationCatalog, import::CatalogImports},
};
use kagari_common::identity::{
    DefinitionKind, ModuleIdentity, PackageId, map::DefinitionContext, metadata::DefinitionMetadata,
};
use kagari_types::{
    callable::CallableImplementation,
    declaration::{
        FieldDef, FnDecl, Param, TraitDef, TypeDef, TypeDefKind,
        host::{
            HostCallPolicy, HostFieldPolicy, HostFunctionEffects, HostPassingStyle,
            HostReflectionPolicy, HostTypeOwnership, HostTypePolicy, PathAccess,
        },
        module::{ImplDecl, ModuleDecl},
    },
    scalar::BuiltinType,
    ty::{NominalTy, Ty},
    visibility::Visibility,
};

fn declarations(dto_as_external: bool) -> DeclarationCatalog {
    declarations_in("example", dto_as_external)
}

fn declarations_in(package: &str, dto_as_external: bool) -> DeclarationCatalog {
    let context = DefinitionContext::new().unwrap();
    let mut module = ModuleDecl::new(ModuleIdentity {
        package: PackageId(package.into()),
        path: vec!["roots".into()],
    });
    let kind = TypeDefKind::Host(HostTypePolicy {
        ownership: HostTypeOwnership::HostRoot,
        reflection: HostReflectionPolicy::Metadata,
        path_access: PathAccess::ReadWrite,
    });
    for (name, kind) in [
        ("Player", kind),
        ("Stats", kind),
        ("Reward", TypeDefKind::Struct),
    ] {
        module.types.push(TypeDef {
            name: name.into(),
            kind,
            generic_params: vec![],
            bounds: vec![],
            fields: vec![],
            variants: vec![],
        });
    }
    module.collect_definitions(&context).unwrap();
    let id = |name| {
        module
            .definition(DefinitionKind::Struct, name, &context.snapshot())
            .unwrap()
    };
    let player = id("Player");
    let stats = id("Stats");
    let reward = id("Reward");
    for (name, ty, visibility, mutable, access) in [
        (
            "parent",
            Ty::Host(player),
            Visibility::Private,
            false,
            PathAccess::ReadOnly,
        ),
        (
            "stats",
            Ty::Host(stats),
            Visibility::Public,
            true,
            PathAccess::ReadWrite,
        ),
        (
            "reward",
            if dto_as_external {
                Ty::Host(reward)
            } else {
                Ty::Struct(NominalTy {
                    declaration: reward,
                    arguments: vec![],
                    associated_types: Default::default(),
                })
            },
            Visibility::Public,
            false,
            PathAccess::ReadOnly,
        ),
    ] {
        module.types[0].fields.push(FieldDef {
            name: name.into(),
            ty,
            visibility,
            mutable,
            host_policy: Some(HostFieldPolicy {
                readable: true,
                path_access: access,
            }),
        });
    }
    for (slot, name) in [(1, "hp"), (2, "amount")] {
        module.types[slot].fields.push(FieldDef {
            name: name.into(),
            ty: Ty::Builtin(BuiltinType::I32),
            visibility: Visibility::Public,
            mutable: true,
            host_policy: (slot == 1).then_some(HostFieldPolicy {
                readable: true,
                path_access: PathAccess::ReadWrite,
            }),
        });
    }
    module.collect_definitions(&context).unwrap();
    let metadata =
        DefinitionMetadata::checked(context.snapshot(), module, &Default::default()).unwrap();
    DeclarationCatalog::from_declarations([&metadata]).unwrap()
}

#[test]
fn prepared_external_metadata_preserves_schema_ownership_and_runtime_isolation() {
    let catalog = declarations(false);
    let registry = TypeRegistry::default();
    let existing = registry
        .register(TypeRegistration::new(
            "application-config",
            TypeKind::Primitive,
        ))
        .unwrap();
    let prepared = registry.snapshot().prepare(&catalog).unwrap();
    assert!(registry.get_by_name("example::roots::Player").is_none());
    let first = prepared.runtime_registry();
    let second = prepared.runtime_registry();
    assert_eq!(first.get(existing), registry.get(existing));
    let player = first.get_by_name("example::roots::Player").unwrap();
    let stats = first.get_by_name("example::roots::Stats").unwrap();
    assert_eq!(player.kind, TypeKind::HostObject);
    assert_eq!(player.fields[0].ty, player.id);
    assert_eq!(player.fields[0].visibility, Visibility::Private);
    assert!(!player.fields[0].writable);
    assert_eq!(player.fields[1].ty, stats.id);
    assert_eq!(player.fields[1].path_access, PathAccess::ReadWrite);
    let reward = first.get(player.fields[2].ty).unwrap();
    assert_eq!(reward.kind, TypeKind::Struct);
    assert_eq!(reward.fields[0].path_access, PathAccess::None);
    assert!(reward.methods.is_empty() && reward.traits.is_empty());
    assert_eq!(first.get(stats.fields[0].ty).unwrap().name, "i32");
    let other = MetadataSnapshot::default()
        .prepare(&declarations_in("other", false))
        .unwrap()
        .runtime_registry();
    let other_stats = other.get_by_name("other::roots::Stats").unwrap();
    assert_ne!(stats.abi_fingerprint, other_stats.abi_fingerprint);
    assert_ne!(
        stats.fields[0].abi_fingerprint,
        other_stats.fields[0].abi_fingerprint
    );
    first
        .register(TypeRegistration::new("first-only", TypeKind::Primitive))
        .unwrap();
    assert!(second.get_by_name("first-only").is_none());
    assert!(
        prepared
            .runtime_registry()
            .get_by_name("first-only")
            .is_none()
    );
}

#[test]
fn rejected_external_metadata_preparation_publishes_no_partial_rows() {
    let registry = TypeRegistry::default();
    let before = registry.len();
    assert!(registry.snapshot().prepare(&declarations(true)).is_err());
    assert_eq!(registry.len(), before);
    let collision = registry
        .register(TypeRegistration::new(
            "example::roots::Stats",
            TypeKind::Primitive,
        ))
        .unwrap();
    assert!(registry.snapshot().prepare(&declarations(false)).is_err());
    assert_eq!(registry.len(), before + 1);
    assert_eq!(registry.get(collision).unwrap().kind, TypeKind::Primitive);
    assert!(registry.get_by_name("example::roots::Player").is_none());
    assert!(
        MetadataSnapshot::default()
            .prepare(&declarations(false))
            .is_ok()
    );
}

#[test]
fn external_members_require_explicit_impl_and_keep_installed_slots_stable() {
    let catalog = declarations(false);
    let context = catalog.types.context().clone();
    let mut module = ModuleDecl::new(ModuleIdentity {
        package: PackageId("example".into()),
        path: vec!["roots".into()],
    });
    module.types.extend(catalog.types.values().cloned());
    let player = module
        .definition(DefinitionKind::Struct, "Player", &context.snapshot())
        .unwrap();
    module.traits.push(TraitDef {
        name: "HasHp".into(),
        associated_consts: vec![],
        associated_types: vec![],
        supertraits: vec![],
        generic_params: vec![],
        bounds: vec![],
        methods: vec![],
        conversion_adapter: None,
    });
    module.collect_definitions(&context).unwrap();
    let interface = module
        .definition(DefinitionKind::Trait, "HasHp", &context.snapshot())
        .unwrap();
    let method = |name: &str, implementation, receiver| FnDecl {
        name: name.into(),
        host_policy: None,
        method_policy: Default::default(),
        implementation,
        generic_params: vec![],
        bounds: vec![],
        params: vec![Param {
            name: "self".into(),
            ty: receiver,
            mutable: false,
        }],
        return_type: Ty::Builtin(BuiltinType::I32),
    };
    module.traits[0].methods.push(method(
        "hp",
        CallableImplementation::Required,
        Ty::SelfType(interface),
    ));
    module.implementations.push(ImplDecl {
        generic_params: vec![],
        bounds: vec![],
        for_type: Ty::Host(player),
        trait_type: None,
        methods: vec![],
    });
    module.collect_definitions(&context).unwrap();
    let owner = module.implementation_id(0, &context.snapshot()).unwrap();
    let heal = context
        .intern_child(owner, DefinitionKind::Method, "heal", 0)
        .unwrap();
    let policy = |passing| HostCallPolicy {
        passing: vec![passing],
        effects: HostFunctionEffects {
            may_mutate_host_state: passing == HostPassingStyle::UniqueBorrow,
            ..Default::default()
        },
    };
    let mut heal_method = method(
        "heal",
        CallableImplementation::Native(heal),
        Ty::Host(player),
    );
    heal_method.host_policy = Some(policy(HostPassingStyle::UniqueBorrow));
    module.implementations[0].methods.push(heal_method);
    let root = context.snapshot().lookup_root(&module.identity).unwrap();
    let trait_impl = context
        .intern_child(root, DefinitionKind::Impl, "", 1)
        .unwrap();
    let read = context
        .intern_child(trait_impl, DefinitionKind::Method, "hp", 0)
        .unwrap();
    let contract = module.traits[0].clone();
    module
        .implement_trait(
            &contract,
            ImplDecl {
                generic_params: vec![],
                bounds: vec![],
                for_type: Ty::Host(player),
                trait_type: Some(NominalTy {
                    declaration: interface,
                    arguments: vec![],
                    associated_types: Default::default(),
                }),
                methods: vec![],
            },
            &[("hp", read)],
            &context,
        )
        .unwrap();
    module.implementations[1].methods[0].host_policy = Some(policy(HostPassingStyle::SharedBorrow));
    let checked = |module| {
        let metadata =
            DefinitionMetadata::checked(context.snapshot(), module, &Default::default()).unwrap();
        DeclarationCatalog::from_declarations([&metadata]).unwrap()
    };
    let catalog = checked(module.clone());
    let prepared = MetadataSnapshot::default().prepare(&catalog).unwrap();
    let rows = prepared.runtime_registry();
    let player = rows.get_by_name("example::roots::Player").unwrap();
    assert_eq!(player.methods.len(), 2);
    let heal = player
        .methods
        .iter()
        .find(|method| method.name == "heal")
        .unwrap();
    assert_eq!(heal.origin, MethodOrigin::Inherent);
    assert_eq!(heal.params[0].ty, player.id);
    let hp = player
        .methods
        .iter()
        .find(|method| method.name == "hp")
        .unwrap();
    assert_eq!(player.traits.len(), 1);
    assert_eq!(hp.origin, MethodOrigin::Trait(player.traits[0].trait_type));
    assert_eq!(player.traits[0].name, "HasHp");
    let mut extra = ModuleDecl::new(ModuleIdentity {
        package: PackageId("aaa".into()),
        path: vec!["extra".into()],
    });
    let mut bonus = contract.clone();
    bonus.name = "Bonus".into();
    bonus.methods.clear();
    extra.traits.push(bonus);
    extra.collect_definitions(&context).unwrap();
    let bonus_id = extra
        .definition(DefinitionKind::Trait, "Bonus", &context.snapshot())
        .unwrap();
    extra.traits[0].methods.push(method(
        "bonus",
        CallableImplementation::Required,
        Ty::SelfType(bonus_id),
    ));
    let extra_root = context.snapshot().lookup_root(&extra.identity).unwrap();
    let extra_impl = context
        .intern_child(extra_root, DefinitionKind::Impl, "", 0)
        .unwrap();
    let added = context
        .intern_child(extra_impl, DefinitionKind::Method, "bonus", 0)
        .unwrap();
    let extra_contract = extra.traits[0].clone();
    extra
        .implement_trait(
            &extra_contract,
            ImplDecl {
                generic_params: vec![],
                bounds: vec![],
                for_type: module.implementations[0].for_type.clone(),
                trait_type: Some(NominalTy {
                    declaration: bonus_id,
                    arguments: vec![],
                    associated_types: Default::default(),
                }),
                methods: vec![],
            },
            &[("bonus", added)],
            &context,
        )
        .unwrap();
    extra.implementations[0].methods[0].host_policy = Some(policy(HostPassingStyle::SharedBorrow));
    let original =
        DefinitionMetadata::checked(context.snapshot(), module, &Default::default()).unwrap();
    let extra =
        DefinitionMetadata::checked(context.snapshot(), extra, &Default::default()).unwrap();
    let complete = DeclarationCatalog::from_declarations([&original, &extra]).unwrap();
    let complete = CatalogImports::new(catalog.types.context().clone())
        .import(&complete)
        .unwrap();
    // The new module sorts before the existing module. Its method receives a
    // fresh physical slot while existing member IDs and fingerprints remain fixed.
    let grown = prepared.prepare(&complete).unwrap().runtime_registry();
    let grown = grown.get_by_name("example::roots::Player").unwrap();
    assert_eq!(
        grown
            .methods
            .iter()
            .find(|method| method.name == "heal")
            .unwrap(),
        heal
    );
    assert_eq!(
        grown
            .methods
            .iter()
            .find(|method| method.name == "hp")
            .unwrap(),
        hp
    );
    assert_eq!(grown.methods.len(), 3);
    assert!(rows.get(player.fields[2].ty).unwrap().methods.is_empty());
}
