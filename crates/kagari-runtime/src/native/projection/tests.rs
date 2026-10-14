use crate::native::{
    binding::NativeResult,
    builder::ModuleBuilder,
    catalog::DeclarationCatalog,
    conversion::KagariType,
    projection::GeneratedField,
    schema::{TypeRegistration, ValueField},
    types::Type,
};
use kagari_derive::KagariValue;
use std::{any::TypeId, ptr};

#[derive(KagariValue)]
struct Stats {
    hp: i32,
}

#[derive(KagariValue)]
struct Inventory {
    gold: i32,
}

#[derive(KagariValue)]
struct Player {
    stats: Stats,
    inventory: Inventory,
}

#[test]
fn generated_inline_fields_preserve_disjoint_pointer_provenance() {
    let mut module = ModuleBuilder::new("test::fields", &DeclarationCatalog::default()).unwrap();
    module.register_type::<Player>().unwrap();
    let module = module.finish().unwrap();
    let catalog = module.catalog();
    let mapping = catalog.rust_types.get(&TypeId::of::<Player>()).unwrap();
    let fields = mapping.fields.as_ref().unwrap();
    assert_eq!(fields[0].name, "stats");
    assert_eq!(fields[1].name, "inventory");
    assert_eq!(fields[0].target, TypeId::of::<Stats>());
    assert_eq!(fields[1].target, TypeId::of::<Inventory>());
    let mut player = Player {
        stats: Stats { hp: 40 },
        inventory: Inventory { gold: 25 },
    };
    let root = ptr::addr_of_mut!(player).cast();
    // SAFETY: The generated layout has been matched against this exact Rust type.
    // Both raw addresses precede references; no mutable ancestor is constructed.
    unsafe {
        let stats = (fields[0].project)(root).cast::<Stats>();
        let inventory = (fields[1].project)(root).cast::<Inventory>();
        let stats = &mut *stats;
        let inventory = &mut *inventory;
        stats.hp += 30;
        inventory.gold -= 5;
        assert_eq!(stats.hp, 70);
        assert_eq!(inventory.gold, 20);
    }
    assert_eq!(player.stats.hp, 70);
    assert_eq!(player.inventory.gold, 20);
}

struct Renamed {
    actual: i32,
}

impl KagariType for Renamed {
    fn kagari_type(catalog: &DeclarationCatalog) -> NativeResult<Type> {
        catalog.rust_type::<Self>()
    }

    fn register_type(registration: &mut TypeRegistration<'_>) -> NativeResult<Type> {
        registration.structure::<Self>("Renamed", |_| {
            Ok(vec![ValueField::new("alias", Type::i32())])
        })
    }

    fn generated_fields() -> Option<Vec<GeneratedField>> {
        // SAFETY: This really is Renamed.actual. Its schema intentionally exports
        // a different identity, which registration must reject before publication.
        Some(vec![unsafe {
            GeneratedField::new::<Self, i32>("actual", |root| {
                ptr::addr_of_mut!((*root.cast::<Self>()).actual).cast()
            })
        }])
    }
}

#[test]
fn mismatched_field_identity_aborts_the_entire_schema_registration() {
    let mut module = ModuleBuilder::new("test::fields", &DeclarationCatalog::default()).unwrap();
    assert!(module.register_type::<Renamed>().is_err());
    assert!(module.providers.rust_type::<Renamed>().is_err());
    let module = module.finish().unwrap();
    assert!(module.declaration().types.is_empty());
}
