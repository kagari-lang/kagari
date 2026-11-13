use super::source_free_vm;
use kagari_runtime::{
    Runtime,
    native::{
        builder::ModuleBuilder, callable::CallableHandle, context::NativeCall,
        declarations::FunctionDecl, scalar::NativeScalar, types::Type, views::ValueHandle,
    },
};

use kagari_derive::KagariValue;

#[derive(KagariValue)]
struct Player {
    hp: i32,
}

#[test]
fn shared_formal_arguments_cannot_forward_mutable_capabilities() {
    let catalog = crate::declarations::StandardDeclarations::default()
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
                |cx: &mut NativeCall<'_>, value: ValueHandle<'_>, callback: CallableHandle<'_>| {
                    cx.collect_garbage()?;
                    let forwarded = value.value(cx)?;
                    i32::decode(callback.call_values(cx, &[forwarded])?)
                },
            )
            .unwrap();
    }
    let module = builder.finish().unwrap();
    let (mut vm, owner) = source_free_vm(
        include_str!("../../../../../tests/fixtures/native/conversion-scoped-capabilities.kgr"),
        &module,
    );
    let _owner_program = owner;
    let ty = vm.bind_rust_type::<Player>(&_owner_program.root()).unwrap();
    let mut player = Player { hp: 40 };
    vm.with_host_scope(|scope| {
        let player = scope.borrow_mut(&ty, &mut player)?;
        for entry in ["attempt", "via_shared"] {
            assert!(
                Runtime::execute_scoped(
                    scope,
                    &_owner_program,
                    entry,
                    &[player.value()],
                    &Default::default()
                )
                .is_err()
            );
        }
        assert_eq!(
            Runtime::execute_scoped(
                scope,
                &_owner_program,
                "read",
                &[player.value()],
                &Default::default()
            )?
            .as_i32(),
            Some(40)
        );
        assert_eq!(
            Runtime::execute_scoped(
                scope,
                &_owner_program,
                "direct",
                &[player.value()],
                &Default::default()
            )?
            .as_i32(),
            Some(41)
        );
        Ok(())
    })
    .unwrap();
    assert_eq!(player.hp, 41);
}
