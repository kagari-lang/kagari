//! An ordinary DTO schema permits explicit external loans, never GC-to-Rust casts.
use super::source_free_vm;
use crate::vm::Vm;
use kagari_derive::KagariValue;
use kagari_hir::analyze_source;
use kagari_runtime::{
    native::{
        builder::ModuleBuilder, conversion::context::ConversionContext, declarations::MethodDecl,
        module::NativeModule, typed::loans::LoanContext, types::Type,
    },
    value::Value,
};
use kagari_source::source::SourceFile;
use kagari_stdlib::declarations::StandardDeclarations;
use kagari_types::{reference::ReferenceAccess, scalar::BuiltinType};
use std::sync::Arc;

#[derive(KagariValue)]
struct Player {
    hp: i32,
    label: String,
}

fn registration() -> NativeModule {
    let providers = StandardDeclarations::default().catalog().unwrap();
    let mut module = ModuleBuilder::new("example::struct_scope", &providers).unwrap();
    let player = module.register_type::<Player>().unwrap();
    module
        .implement(player, |implementation| {
            implementation.inherent_impl(|methods| {
                let inspect = methods.define_method(
                    MethodDecl::instance("inspect")
                        .receiver_access(ReferenceAccess::Shared)
                        .returns(Type::scalar(BuiltinType::String)),
                )?;
                methods.bind(
                    inspect,
                    |cx: &mut LoanContext<'_>, (player,): (&Player,)| {
                        cx.poll()?;
                        Ok(format!("{} hp={}", player.label, player.hp))
                    },
                )?;
                let damage = methods.define_method(
                    MethodDecl::instance("damage")
                        .receiver_access(ReferenceAccess::Mutable)
                        .parameter("amount", Type::i32())
                        .returns(Type::i32()),
                )?;
                methods.bind(
                    damage,
                    |cx: &mut LoanContext<'_>, (player, amount): (&mut Player, i32)| {
                        cx.poll()?;
                        player.hp -= amount;
                        Ok(player.hp)
                    },
                )?;
                Ok(())
            })
        })
        .unwrap();
    module.finish().unwrap()
}

const SOURCE: &str = r#"
    use example::struct_scope::Player;
    pub fn inspect(player: Ref<Player>) -> String { player.inspect() }
    pub fn damage(player: RefMut<Player>, amount: i32) -> i32 { player.damage(amount) }
    pub fn copy(player: Player) -> Player { player }
"#;

#[test]
fn registered_struct_loans_use_original_host_storage_and_common_methods() {
    let module = registration();
    assert_eq!(module.declaration().types.len(), 1);
    let (mut vm, owner) = source_free_vm(SOURCE, &module);
    let _owner_program = owner;
    let owner = vm.runtime().select_program(&_owner_program).unwrap();
    let descriptor = vm
        .runtime()
        .bind_rust_type::<Player>(&_owner_program.root())
        .unwrap();
    assert!(
        vm.runtime()
            .bind_rust_type::<i32>(&_owner_program.root())
            .is_err()
    );
    let mut external = Player {
        hp: 10,
        label: "external".into(),
    };
    let original = ConversionContext::new(vm.runtime(), &owner)
        .unwrap()
        .encode(Player {
            hp: external.hp,
            label: external.label.clone(),
        })
        .unwrap();
    let retained = vm
        .runtime_mut()
        .with_host_scope(|scope| {
            let snapshot = scope.retain(&original)?;
            let replacement = ConversionContext::new(scope.runtime(), &owner)?.encode(Player {
                hp: 99,
                label: "replacement".into(),
            })?;
            original
                .replace(scope.runtime().gc(), &replacement)
                .unwrap();
            scope.runtime().collect_garbage()?;
            let preserved = snapshot.retain()?;
            assert_eq!(
                ConversionContext::new(scope.runtime(), &owner)?
                    .decode::<Player>(&preserved)?
                    .hp,
                10
            );
            // Restore the fixture's original value from the independently retained snapshot.
            original.replace(scope.runtime().gc(), &preserved).unwrap();
            let view = scope.borrow_mut(&descriptor, &mut external)?;
            assert!(view.argument().retain().is_err());
            let text = Vm::execute_scoped(
                scope,
                &_owner_program,
                "inspect",
                &[view.argument()],
                Default::default(),
            )?;
            assert_eq!(
                Vm::execute_scoped(
                    scope,
                    &_owner_program,
                    "damage",
                    &[view.argument(), scope.scalar(3_i32)],
                    Default::default()
                )?
                .inspect(|view| view.as_i32().map(Value::I32))?,
                Value::I32(7)
            );
            // A later invocation may collect; the earlier heap result is retained
            // for the active scope, independently of the original Rust object.
            scope.runtime().collect_garbage()?;
            assert_eq!(
                text.inspect(|view| Ok(view.as_str()?.to_owned()))?,
                "external hp=10"
            );
            view.read(scope, |player| {
                assert_eq!(player.hp, 7);
                Ok(())
            })?;
            assert!(
                view.read(scope, |_| Vm::execute_scoped(
                    scope,
                    &_owner_program,
                    "damage",
                    &[view.argument(), scope.scalar(1_i32)],
                    Default::default()
                ))
                .is_err()
            );
            text.retain()
        })
        .unwrap();
    vm.runtime().collect_garbage().unwrap();
    assert_eq!(
        retained
            .inspect(vm.runtime(), |view| Ok(view.as_str()?.to_owned()))
            .unwrap(),
        "external hp=10"
    );
    assert_eq!(external.hp, 7);
    let mut cx = ConversionContext::new(vm.runtime(), &owner).unwrap();
    assert_eq!(cx.decode::<Player>(&original).unwrap().hp, 10);
    let copied: Player = vm
        .execute_typed(
            &owner,
            "copy",
            (Player {
                hp: 2,
                label: "DTO".into(),
            },),
        )
        .unwrap();
    assert_eq!(copied.hp, 2);
}

#[test]
fn ordinary_gc_structs_and_shared_views_cannot_call_mutable_rust_loans() {
    let module = registration();
    let mut providers = kagari_stdlib::catalog::shared().unwrap();
    providers.push(Arc::new(module.to_declaration()));
    for source in [
        "fn bad(player: Player) -> String { player.inspect() }",
        "fn bad(player: Player) -> i32 { player.damage(1) }",
        "fn bad(player: Ref<Player>) -> i32 { player.damage(1) }",
    ] {
        let analysis = analyze_source(
            &SourceFile::new(
                "reject.kgr",
                format!("use example::struct_scope::Player; {source}"),
            ),
            providers.clone(),
        )
        .unwrap();
        assert!(!analysis.diagnostics().is_empty(), "{source}");
    }
}
