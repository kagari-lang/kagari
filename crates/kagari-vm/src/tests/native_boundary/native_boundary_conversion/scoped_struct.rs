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
    let descriptor = vm.runtime().bind_rust_type::<Player>(&owner).unwrap();
    assert!(vm.runtime().bind_rust_type::<i32>(&owner).is_err());
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
    vm.runtime_mut()
        .with_host_scope(|scope| {
            let view = scope.borrow_mut(&descriptor, &mut external)?;
            let text = Vm::execute_scoped(
                scope,
                &owner,
                "inspect",
                &[view.value()],
                Default::default(),
            )?;
            assert_eq!(
                Vm::execute_scoped(
                    scope,
                    &owner,
                    "damage",
                    &[view.value(), Value::I32(3)],
                    Default::default()
                )?,
                Value::I32(7)
            );
            // A later invocation may collect; the earlier heap result is retained
            // for the active scope, independently of the original Rust object.
            scope.execute(&[], |runtime, _| {
                runtime.collect_garbage()?;
                let Value::Str(id) = text else {
                    panic!("String");
                };
                assert_eq!(&*runtime.gc().string(id).unwrap(), "external hp=10");
                Ok(Value::Unit)
            })?;
            view.read(scope, |player| {
                assert_eq!(player.hp, 7);
                Ok(())
            })?;
            assert!(
                view.read(scope, |_| Vm::execute_scoped(
                    scope,
                    &owner,
                    "damage",
                    &[view.value(), Value::I32(1)],
                    Default::default()
                ))
                .is_err()
            );
            Ok(())
        })
        .unwrap();
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
