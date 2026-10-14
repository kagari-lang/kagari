use super::MetadataSnapshot;
use crate::{
    metadata::{TypeKind, TypeRegistration, TypeRegistry},
    native::catalog::DeclarationCatalog,
};
use kagari_common::identity::{
    DefinitionKind, ModuleIdentity, PackageId, map::DefinitionContext, metadata::DefinitionMetadata,
};
use kagari_types::{
    declaration::{
        FieldDef, TypeDef, TypeDefKind,
        host::{
            HostFieldPolicy, HostReflectionPolicy, HostTypeOwnership, HostTypePolicy, PathAccess,
        },
        module::ModuleDecl,
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
