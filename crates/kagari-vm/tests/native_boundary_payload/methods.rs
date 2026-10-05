use super::PlayerState;
use crate::{compile_program, native_boundary_functions::fixture};
use kagari_runtime::native::{
    binding::NativeResult,
    builder::ModuleBuilder,
    conversion::context::ConversionContext,
    declarations::{CallableRequirement, MethodDecl},
    module::NativeModule,
    payload::NativeObject,
    storage::NativeStorage,
    typed::NativeContext,
    types::{FunctionRef, Type, TypeRef},
    value_handle::ScriptValue,
};
use kagari_stdlib::declarations::StandardDeclarations;

fn registration() -> (NativeModule, TypeRef, FunctionRef) {
    let mut module = ModuleBuilder::new(
        "example::methods",
        &StandardDeclarations::default().catalog().unwrap(),
    );
    let mut declaration = module.define_type("Player");
    declaration
        .native_storage(NativeStorage::data::<PlayerState>())
        .unwrap();
    let player = declaration.finish().unwrap();
    let damage = module
        .implement(player.clone(), |implementation| {
            let receiver = implementation.receiver();
            implementation.inherent_impl(|methods| {
                let new = methods.define_method(
                    MethodDecl::static_method("new")
                        .parameter("hp", Type::i32())
                        .returns(receiver),
                )?;
                methods.bind_typed(new, |cx: &mut NativeContext<'_>, (hp,): (i32,)| {
                    cx.create_native(PlayerState {
                        hp,
                        position: [0.0; 3],
                    })
                })?;
                let damage = methods.define_method(
                    MethodDecl::instance("damage")
                        .parameter("amount", Type::i32())
                        .returns(Type::i32()),
                )?;
                methods.bind_typed_method(
                    damage.clone(),
                    |cx: &mut NativeContext<'_>,
                     player: NativeObject<PlayerState>,
                     (amount,): (i32,)| {
                        player.edit(cx, |state| {
                            state.hp -= amount;
                            Ok(state.hp)
                        })
                    },
                )?;
                let echo = methods.define_method(MethodDecl::instance("echo"))?;
                methods.method(&echo, |signature| {
                    let output = signature.type_parameter("Output")?;
                    signature.parameter("value", output.ty());
                    signature.returns(output.ty());
                    Ok(())
                })?;
                methods.bind_typed_method(
                    echo,
                    |_: &mut NativeContext<'_>,
                     _: NativeObject<PlayerState>,
                     (value,): (ScriptValue,)|
                     -> NativeResult<ScriptValue> { Ok(value) },
                )?;
                Ok(damage)
            })
        })
        .unwrap();
    (module.finish().unwrap(), player, damage)
}

const SOURCE: &str = r#"
    use example::methods::Player;
    pub fn make() -> Player { Player::new(100) }
    pub fn hit(player: Player, amount: i32) -> i32 { player.damage(amount) }
    pub fn echo(player: Player) -> i32 { player.echo::<i32>(7) }
"#;

#[test]
fn native_inherent_members_support_prepared_calls_and_method_parameters() {
    let (module, declaration, damage_declaration) = registration();
    let (vm, owner) = fixture(SOURCE, Some(&module));
    let runtime = vm.runtime();
    let ty = runtime
        .bind_native_type::<PlayerState>(&owner, &declaration, &[])
        .unwrap();
    let member = ty.method_declaration(damage_declaration.id()).unwrap();
    let damage = runtime
        .bind_method_declaration::<(i32,), i32>(&member)
        .unwrap();
    let again = runtime.bind_method::<(i32,), i32>(&ty, "damage").unwrap();
    let new = runtime
        .bind_associated_function::<(i32,), NativeObject<PlayerState>>(&ty, "new")
        .unwrap();
    let player = vm.call(&new, (100,)).unwrap();
    assert_eq!(
        vm.execute_typed::<_, i32>(&owner, "hit", (player.clone(), 5))
            .unwrap(),
        95
    );
    let mut cx = vm.context(&owner).unwrap();
    assert_eq!(player.call(&mut cx, &damage, (10,)).unwrap(), 85);
    runtime.collect_garbage().unwrap();
    assert_eq!(player.call(&mut cx, &again, (15,)).unwrap(), 70);
    let echo = runtime
        .bind_method_application::<(i32,), i32>(
            &ty,
            "echo",
            &[ConversionContext::new(runtime, &owner)
                .unwrap()
                .type_for::<i32>()
                .unwrap()],
        )
        .unwrap();
    assert_eq!(player.call(&mut cx, &echo, (42,)).unwrap(), 42);
    assert!(runtime.bind_method::<(bool,), i32>(&ty, "damage").is_err());
    assert!(runtime.bind_method::<(i32,), i32>(&ty, "new").is_err());
    assert!(
        runtime
            .bind_associated_function::<(NativeObject<PlayerState>, i32), i32>(&ty, "damage")
            .is_err()
    );
    let mut nested = vm.context(&owner).unwrap();
    player
        .edit(&cx, |_| {
            assert!(player.call(&mut nested, &damage, (1,)).is_err());
            Ok(())
        })
        .unwrap();
    let newer = vm
        .reload_program(&owner, "functions", compile_program(SOURCE, Some(&module)))
        .unwrap();
    let newer_type = runtime
        .bind_native_type::<PlayerState>(&newer, &declaration, &[])
        .unwrap();
    let newer_damage = runtime
        .bind_method::<(i32,), i32>(&newer_type, "damage")
        .unwrap();
    assert!(player.call(&mut cx, &newer_damage, (1,)).is_err());
    assert_eq!(player.call(&mut cx, &damage, (1,)).unwrap(), 69);
    let (foreign, foreign_owner) = fixture(SOURCE, Some(&module));
    assert!(
        player
            .call(&mut foreign.context(&foreign_owner).unwrap(), &damage, (1,))
            .is_err()
    );
    assert_eq!(player.read(&cx, |state| Ok(state.hp)).unwrap(), 69);
}

#[test]
fn generic_native_impl_groups_preserve_receiver_and_local_binders() {
    let mut module = ModuleBuilder::new(
        "example::generic_methods",
        &StandardDeclarations::default().catalog().unwrap(),
    );
    let mut declaration = module.define_type("Holder");
    declaration.type_parameter("Element").unwrap();
    declaration
        .native_storage(NativeStorage::data::<PlayerState>())
        .unwrap();
    let holder = declaration.finish().unwrap();
    module
        .implement(holder.clone(), |implementation| {
            let receiver = implementation.receiver();
            let element = implementation.parameter("Element")?;
            implementation.inherent_impl(|methods| {
                let new =
                    methods.define_method(MethodDecl::static_method("new").returns(receiver))?;
                methods.bind_typed(new, |cx: &mut NativeContext<'_>, (): ()| {
                    cx.create_native(PlayerState {
                        hp: 0,
                        position: [0.0; 3],
                    })
                })
            })?;
            // A second group has a different impl identity; authored receiver parameters
            // must be substituted without overwriting the method's own binders.
            implementation.inherent_impl(|methods| {
                let exchange = methods.define_method(
                    MethodDecl::instance("exchange")
                        .parameter("value", element.ty())
                        .returns(element.ty()),
                )?;
                methods.bind_typed_method(
                    exchange,
                    |_: &mut NativeContext<'_>,
                     _: NativeObject<PlayerState>,
                     (value,): (ScriptValue,)|
                     -> NativeResult<ScriptValue> { Ok(value) },
                )?;
                let choose = methods.define_method(
                    MethodDecl::instance("choose").parameter("input", element.ty()),
                )?;
                methods.method(&choose, |signature| {
                    let output = signature.type_parameter("Output")?;
                    signature.parameter("value", output.ty());
                    signature.returns(output.ty());
                    Ok(())
                })?;
                methods.bind_typed_method(
                    choose,
                    |_: &mut NativeContext<'_>,
                     _: NativeObject<PlayerState>,
                     (_, value): (ScriptValue, ScriptValue)|
                     -> NativeResult<ScriptValue> { Ok(value) },
                )
            })
        })
        .unwrap();
    let module = module.finish().unwrap();
    let (vm, owner) = fixture(
        r#"
        use example::generic_methods::Holder;
        pub fn make() -> Holder<i32> { Holder::new::<i32>() }
        pub fn other() -> Holder<String> { Holder::new::<String>() }
        pub fn evidence() -> String {
            val holder = make();
            val previous = holder.exchange(12);
            holder.choose::<String>(previous, "selected")
        }
    "#,
        Some(&module),
    );
    let runtime = vm.runtime();
    let conversion = ConversionContext::new(runtime, &owner).unwrap();
    let i32_type = conversion.type_for::<i32>().unwrap();
    let string_type = conversion.type_for::<String>().unwrap();
    let ty = runtime
        .bind_native_type::<PlayerState>(&owner, &holder, &[i32_type])
        .unwrap();
    let exchange = runtime.bind_method::<(i32,), i32>(&ty, "exchange").unwrap();
    let choose = runtime
        .bind_method_application::<(i32, String), String>(&ty, "choose", &[string_type])
        .unwrap();
    let value: NativeObject<PlayerState> = vm.execute_typed(&owner, "make", ()).unwrap();
    let other: NativeObject<PlayerState> = vm.execute_typed(&owner, "other", ()).unwrap();
    let mut cx = vm.context(&owner).unwrap();
    assert_eq!(value.call(&mut cx, &exchange, (33,)).unwrap(), 33);
    assert_eq!(
        value.call(&mut cx, &choose, (1, "host".into())).unwrap(),
        "host"
    );
    assert!(other.call(&mut cx, &exchange, (1,)).is_err());
    assert_eq!(
        vm.execute_typed::<_, String>(&owner, "evidence", ())
            .unwrap(),
        "selected"
    );
    assert!(
        runtime
            .bind_method::<(String,), String>(&ty, "exchange")
            .is_err()
    );
    assert!(
        runtime
            .bind_method::<(i32, String), String>(&ty, "choose")
            .is_err()
    );
}

#[test]
fn native_method_requirements_use_selected_generic_evidence() {
    let mut module = ModuleBuilder::new(
        "example::bounded_methods",
        &StandardDeclarations::default().catalog().unwrap(),
    );
    let mut contract = module.define_trait("Read");
    contract
        .define_method(MethodDecl::instance("read").returns(Type::i32()))
        .unwrap();
    let contract = contract.finish().unwrap();
    let mut declaration = module.define_type("Holder");
    declaration.type_parameter("Element").unwrap();
    declaration
        .native_storage(NativeStorage::data::<PlayerState>())
        .unwrap();
    let holder = declaration.finish().unwrap();
    module
        .implement(holder.clone(), |implementation| {
            let receiver = implementation.receiver();
            let element = implementation.parameter("Element")?.ty();
            implementation.inherent_impl(|methods| {
                let new =
                    methods.define_method(MethodDecl::static_method("new").returns(receiver))?;
                methods.bind_typed(new, |cx: &mut NativeContext<'_>, (): ()| {
                    cx.create_native(PlayerState {
                        hp: 0,
                        position: [0.0; 3],
                    })
                })
            })?;
            implementation.inherent_impl(|methods| {
                let read = methods.define_method(
                    MethodDecl::instance("read")
                        .parameter("item", element.clone())
                        .returns(Type::i32()),
                )?;
                let selected = methods.method(&read, |signature| {
                    signature.bound(element.clone(), contract.apply([]));
                    Ok(signature.requires(CallableRequirement::method(
                        element,
                        contract.method("read")?,
                    )))
                })?;
                methods.bind_typed_method(
                    read,
                    move |cx: &mut NativeContext<'_>,
                          _: NativeObject<PlayerState>,
                          (item,): (ScriptValue,)| {
                        let method = cx.selected_method::<(ScriptValue,), i32>(&selected)?;
                        cx.collect_garbage()?;
                        method.call(cx, (item,))
                    },
                )
            })
        })
        .unwrap();
    let module = module.finish().unwrap();
    let (vm, owner) = fixture(
        r#"
        use example::bounded_methods::{Holder, Read};
        pub struct Item { pub val value: i32 }
        impl Read for Item { fn read(self) -> i32 { self.value } }
        pub fn make() -> Holder<Item> { Holder::new::<Item>() }
        pub fn item() -> Item { Item { value: 77 } }
        pub fn evidence() -> i32 { make().read(item()) }
    "#,
        Some(&module),
    );
    let runtime = vm.runtime();
    let item_type = runtime.bind_type(&owner, "Item", &[]).unwrap();
    let ty = runtime
        .bind_native_type::<PlayerState>(&owner, &holder, &[item_type.type_argument().clone()])
        .unwrap();
    let read = runtime
        .bind_method::<(ScriptValue,), i32>(&ty, "read")
        .unwrap();
    let holder: NativeObject<PlayerState> = vm.execute_typed(&owner, "make", ()).unwrap();
    let item: ScriptValue = vm.execute_typed(&owner, "item", ()).unwrap();
    assert_eq!(
        holder
            .call(&mut vm.context(&owner).unwrap(), &read, (item,))
            .unwrap(),
        77
    );
    assert_eq!(
        vm.execute_typed::<_, i32>(&owner, "evidence", ()).unwrap(),
        77
    );
}
