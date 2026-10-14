use super::source_free_vm;
use kagari_derive::KagariValue;
use kagari_runtime::{
    frame::types::arguments::TypeArgument,
    native::{
        binding::NativeResult,
        builder::ModuleBuilder,
        catalog::DeclarationCatalog,
        conversion::{FromKagari, KagariType, context::ConversionContext},
        declarations::{FunctionDecl, MethodDecl},
        storage::NativeStorage,
        typed::{NativeContext, loans::LoanContext},
        types::Type,
    },
    value::Value,
};
use kagari_types::reference::ReferenceAccess;
use kagari_vm::vm::Vm;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

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

kagari_runtime::native_data! { struct Treasury { hp: i32 } }

static CONVERSIONS: AtomicUsize = AtomicUsize::new(0);

struct ConversionEffect(i32);

impl KagariType for ConversionEffect {
    fn kagari_type(_: &DeclarationCatalog) -> NativeResult<Type> {
        Ok(Type::i32())
    }
}

impl FromKagari for ConversionEffect {
    fn from_kagari(
        cx: &mut ConversionContext<'_>,
        expected: &TypeArgument,
        value: &Value,
    ) -> NativeResult<Self> {
        let value = i32::from_kagari(cx, expected, value)?;
        CONVERSIONS.fetch_add(1, Ordering::SeqCst);
        Ok(Self(value))
    }
}

#[test]
fn one_registered_pair_method_serves_opaque_and_external_receivers() {
    let providers = kagari_stdlib::declarations::StandardDeclarations::default()
        .catalog()
        .unwrap();
    let mut builder = ModuleBuilder::new("example::mixed_packs", &providers).unwrap();
    let inventory = builder.register_type::<Inventory>().unwrap();
    let mut treasury = builder.define_type("Treasury").unwrap();
    treasury
        .native_storage(NativeStorage::data::<Treasury>())
        .unwrap();
    let treasury = treasury.finish().unwrap();
    builder.implement(treasury.clone(), |implementation| implementation.inherent_impl(|methods| {
        let transfer = methods.define_method(MethodDecl::instance("transfer")
            .receiver_access(ReferenceAccess::Mutable)
            .parameter("inventory", Type::ref_mut(inventory))
            .parameter("amount", Type::i32()).returns(Type::i32()))?;
        methods.bind(transfer, |_: &mut LoanContext<'_>, (treasury, inventory, amount): (&mut Treasury, &mut Inventory, i32)| {
            treasury.hp += amount; inventory.gold -= 5; Ok(treasury.hp)
        })
    })).unwrap();
    let module = builder.finish().unwrap();
    let (mut vm, owner) = source_free_vm(
        r#"
        use example::mixed_packs::{Treasury, Inventory};
        pub fn owned(treasury: Treasury, inventory: RefMut<Inventory>) -> i32 { treasury.transfer(inventory, 30) }
        pub fn external(treasury: RefMut<Treasury>, inventory: RefMut<Inventory>) -> i32 { treasury.transfer(inventory, 30) }
    "#,
        &module,
    );
    let treasury_type = vm
        .runtime()
        .bind_native_type::<Treasury>(&owner, &treasury, &[])
        .unwrap();
    let inventory_type = vm.runtime().bind_rust_type::<Inventory>(&owner).unwrap();
    let opaque = treasury_type
        .create(
            &mut NativeContext::new(vm.runtime(), &owner).unwrap(),
            Treasury { hp: 40 },
        )
        .unwrap();
    let raw = ConversionContext::new(vm.runtime(), &owner)
        .unwrap()
        .encode_value(treasury_type.type_argument(), opaque.clone())
        .unwrap();
    let mut external = Treasury { hp: 40 };
    let mut inventory = Inventory { gold: 25 };
    vm.runtime_mut()
        .with_host_scope(|scope| {
            let treasury = scope.borrow_mut(treasury_type.rust_type(), &mut external)?;
            let inventory = scope.borrow_mut(&inventory_type, &mut inventory)?;
            assert_eq!(
                Vm::execute_scoped(scope, &owner, "owned", &[raw, inventory.value()])?,
                Value::I32(70)
            );
            assert_eq!(
                Vm::execute_scoped(
                    scope,
                    &owner,
                    "external",
                    &[treasury.value(), inventory.value()]
                )?,
                Value::I32(70)
            );
            Ok(())
        })
        .unwrap();
    assert_eq!(external.hp, 70);
    assert_eq!(inventory.gold, 15);
    assert_eq!(
        opaque
            .read(
                &NativeContext::new(vm.runtime(), &owner).unwrap(),
                |value| Ok(value.hp)
            )
            .unwrap(),
        70
    );
}

#[test]
fn common_bind_accepts_free_borrowed_packs_and_rejects_overlap_before_callbacks() {
    CONVERSIONS.store(0, Ordering::SeqCst);
    let providers = kagari_stdlib::declarations::StandardDeclarations::default()
        .catalog()
        .unwrap();
    let mut builder = ModuleBuilder::new("example::packs", &providers).unwrap();
    builder.register_type::<Player>().unwrap();
    let stats = builder.declarations().rust_type::<Stats>().unwrap();
    let inventory = builder.declarations().rust_type::<Inventory>().unwrap();
    let read = builder
        .define_function(
            FunctionDecl::new("read_hp")
                .parameter("stats", Type::ref_(stats.clone()))
                .returns(Type::i32()),
        )
        .unwrap();
    builder
        .bind(read, |_: &mut LoanContext<'_>, (stats,): (&Stats,)| {
            Ok(stats.hp)
        })
        .unwrap();
    let heal = builder
        .define_function(
            FunctionDecl::new("heal")
                .parameter("stats", Type::ref_mut(stats.clone()))
                .parameter("inventory", Type::ref_mut(inventory.clone()))
                .parameter("amount", Type::i32())
                .returns(Type::i32()),
        )
        .unwrap();
    builder
        .bind(
            heal,
            |cx: &mut LoanContext<'_>,
             (stats, inventory, amount): (&mut Stats, &mut Inventory, i32)| {
                cx.poll()?;
                stats.hp += amount;
                inventory.gold -= 5;
                Ok(stats.hp)
            },
        )
        .unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let called = calls.clone();
    let same = builder
        .define_function(
            FunctionDecl::new("both")
                .parameter("first", Type::ref_mut(stats.clone()))
                .parameter("second", Type::ref_mut(stats))
                .parameter("effect", Type::i32())
                .returns(Type::i32()),
        )
        .unwrap();
    builder
        .bind(
            same,
            move |_: &mut LoanContext<'_>,
                  (first, second, effect): (&mut Stats, &mut Stats, ConversionEffect)| {
                called.fetch_add(1, Ordering::SeqCst);
                first.hp += second.hp + effect.0;
                Ok(first.hp)
            },
        )
        .unwrap();
    let module = builder.finish().unwrap();
    let (mut vm, owner) = source_free_vm(
        r#"
        use example::packs::{Stats, Inventory, heal, read_hp, both};
        pub fn tick(stats: RefMut<Stats>, inventory: RefMut<Inventory>) -> i32 { heal(stats, inventory, 30) }
        pub fn inspect(stats: Ref<Stats>) -> i32 { read_hp(stats) }
        pub fn duplicate(stats: RefMut<Stats>) -> i32 { both(stats, stats, 1) }
    "#,
        &module,
    );
    let player_type = vm.runtime().bind_rust_type::<Player>(&owner).unwrap();
    let stats_field = vm
        .runtime()
        .bind_rust_field::<Player, Stats>(&player_type, "stats")
        .unwrap();
    let inventory_field = vm
        .runtime()
        .bind_rust_field::<Player, Inventory>(&player_type, "inventory")
        .unwrap();
    let mut player = Player {
        stats: Stats { hp: 40 },
        inventory: Inventory { gold: 25 },
    };
    vm.runtime_mut()
        .with_host_scope(|scope| {
            let whole = scope.borrow_mut(&player_type, &mut player)?;
            let stats = whole.field(scope, &stats_field)?;
            let inventory = whole.field(scope, &inventory_field)?;
            assert_eq!(
                Vm::execute_scoped(scope, &owner, "tick", &[stats.value(), inventory.value()])?,
                Value::I32(70)
            );
            assert_eq!(
                Vm::execute_scoped(scope, &owner, "inspect", &[stats.value()])?,
                Value::I32(70)
            );
            assert!(Vm::execute_scoped(scope, &owner, "duplicate", &[stats.value()]).is_err());
            assert_eq!(calls.load(Ordering::SeqCst), 0);
            assert_eq!(CONVERSIONS.load(Ordering::SeqCst), 1);
            stats.edit(scope, |stats| {
                stats.hp += 1;
                Ok(())
            })?;
            Ok(())
        })
        .unwrap();
    assert_eq!(player.stats.hp, 71);
    assert_eq!(player.inventory.gold, 20);
}
