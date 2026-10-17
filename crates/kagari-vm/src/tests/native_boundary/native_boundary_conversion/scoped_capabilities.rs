use super::source_free_vm;
use crate::vm::Vm;
use kagari_derive::KagariValue;
use kagari_runtime::{
    native::{
        builder::ModuleBuilder, callable::CallableHandle, context::CallContext,
        declarations::FunctionDecl, scalar::NativeScalar, types::Type, views::ValueHandle,
    },
    value::Value,
};

#[derive(KagariValue)]
struct Player {
    hp: i32,
}

#[test]
fn shared_formal_arguments_cannot_forward_mutable_capabilities() {
    let catalog = kagari_stdlib::declarations::StandardDeclarations::default()
        .catalog()
        .unwrap();
    let mut builder = ModuleBuilder::new("example::capabilities", &catalog).unwrap();
    let player = builder.register_type::<Player>().unwrap();
    for (name, callback_parameter) in [
        ("forward", Type::ref_mut(player.clone())),
        ("inspect", Type::ref_(player.clone())),
    ] {
        let callback = Type::function([callback_parameter], Type::i32()).unwrap();
        let forward = builder
            .define_function(
                FunctionDecl::new(name)
                    .parameter("player", Type::ref_(player.clone()))
                    .parameter("callback", callback)
                    .returns(Type::i32()),
            )
            .unwrap();
        builder
            .bind(
                forward,
                |cx: &mut CallContext<'_>, value: ValueHandle<'_>, callback: CallableHandle<'_>| {
                    cx.collect_garbage()?;
                    assert_eq!(value.value(), cx.argument(0)?);
                    assert_eq!(value.value(), cx.arguments().get(0).unwrap());
                    i32::decode(callback.call_values(cx, &[value.value()])?)
                },
            )
            .unwrap();
    }
    let module = builder.finish().unwrap();
    let (mut vm, owner) = source_free_vm(
        r#"
        use example::capabilities::{Player, forward, inspect};
        fn bump(player: RefMut<Player>) -> i32 { player.hp += 1; player.hp }
        pub fn attempt(player: RefMut<Player>) -> i32 { forward(player, |p: RefMut<Player>| bump(p)) }
        fn relay(player: Ref<Player>) -> i32 { forward(player, |p: RefMut<Player>| bump(p)) }
        pub fn via_shared(player: RefMut<Player>) -> i32 { relay(player) }
        pub fn read(player: Ref<Player>) -> i32 { inspect(player, |p: Ref<Player>| p.hp) }
        pub fn direct(player: RefMut<Player>) -> i32 { bump(player) }
        "#,
        &module,
    );
    let ty = vm.runtime().bind_rust_type::<Player>(&owner).unwrap();
    let mut player = Player { hp: 40 };
    vm.runtime_mut()
        .with_host_scope(|scope| {
            let player = scope.borrow_mut(&ty, &mut player)?;
            for entry in ["attempt", "via_shared"] {
                assert!(
                    Vm::execute_scoped(scope, &owner, entry, &[player.value()], Default::default())
                        .is_err()
                );
            }
            assert_eq!(
                Vm::execute_scoped(scope, &owner, "read", &[player.value()], Default::default())?,
                Value::I32(40)
            );
            assert_eq!(
                Vm::execute_scoped(
                    scope,
                    &owner,
                    "direct",
                    &[player.value()],
                    Default::default()
                )?,
                Value::I32(41)
            );
            Ok(())
        })
        .unwrap();
    assert_eq!(player.hp, 41);
}
