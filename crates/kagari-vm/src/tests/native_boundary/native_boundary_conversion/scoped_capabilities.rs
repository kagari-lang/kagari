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
                    let expected = cx.argument_type_argument(0)?;
                    let arguments = cx.arguments().snapshot()?;
                    let mut native = cx.native_context()?;
                    assert!(arguments.retain(native.conversion(), &expected, 0).is_err());
                    // SAFETY: The active call and original host scope cover this read and synchronous forwarding.
                    assert_eq!(unsafe { value.value_transport() }, unsafe {
                        cx.argument_transport(0)
                    }?);
                    // SAFETY: The active call retains the argument and its original host scope.
                    assert_eq!(
                        // SAFETY: The active call and original host scope cover this read and synchronous forwarding.
                        unsafe { value.value_transport() },
                        unsafe { cx.arguments().get_transport(0) }.unwrap()
                    );
                    // SAFETY: The active call and original host scope cover this read and synchronous forwarding.
                    i32::decode(unsafe {
                        callback.call_values_transport(cx, &[value.value_transport()])
                    }?)
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
    let _owner_program = owner;
    let ty = vm
        .runtime()
        .bind_rust_type::<Player>(&_owner_program.root())
        .unwrap();
    let mut player = Player { hp: 40 };
    vm.runtime_mut()
        .with_host_scope(|scope| {
            let player = scope.borrow_mut(&ty, &mut player)?;
            for entry in ["attempt", "via_shared"] {
                assert!(
                    Vm::execute_scoped(
                        scope,
                        &_owner_program,
                        entry,
                        &[player.argument()],
                        Default::default()
                    )
                    .is_err()
                );
            }
            assert_eq!(
                Vm::execute_scoped(
                    scope,
                    &_owner_program,
                    "read",
                    &[player.argument()],
                    Default::default()
                )?
                .inspect(|view| view.as_i32().map(Value::I32))?,
                Value::I32(40)
            );
            assert_eq!(
                Vm::execute_scoped(
                    scope,
                    &_owner_program,
                    "direct",
                    &[player.argument()],
                    Default::default()
                )?
                .inspect(|view| view.as_i32().map(Value::I32))?,
                Value::I32(41)
            );
            Ok(())
        })
        .unwrap();
    assert_eq!(player.hp, 41);
}
