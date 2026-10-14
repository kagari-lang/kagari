mod scoped;
use super::PlayerState;
use crate::{compile_program, native_boundary_functions::fixture};
use kagari_bytecode::program::verify_program;
use kagari_contract::types::PublicItem;
use kagari_hir::analyze_source;
use kagari_runtime::native::{
    binding::NativeResult,
    builder::ModuleBuilder,
    conversion::context::ConversionContext,
    declarations::{CallableRequirement, MethodDecl},
    module::NativeModule,
    payload::Opaque,
    storage::NativeStorage,
    typed::{NativeContext, loans::LoanContext},
    types::{FunctionRef, Type, TypeRef},
    value_handle::ScriptValue,
};
use kagari_source::source::SourceFile;
use kagari_stdlib::declarations::StandardDeclarations;
use kagari_types::{reference::ReferenceAccess, scalar::BuiltinType, ty::Ty};
use std::sync::Arc;

trait HostDescribe {
    fn describe(&self) -> String;
}

impl HostDescribe for PlayerState {
    fn describe(&self) -> String {
        format!("trait hp={}", self.hp)
    }
}

fn registration() -> (NativeModule, TypeRef, FunctionRef) {
    let mut module = ModuleBuilder::new(
        "example::methods",
        &StandardDeclarations::default().catalog().unwrap(),
    )
    .unwrap();
    let mut declaration = module.define_type("Player").unwrap();
    declaration
        .native_storage(NativeStorage::data::<PlayerState>())
        .unwrap();
    let player = declaration.finish().unwrap();
    let mut other = module.define_type("Other").unwrap();
    other
        .native_storage(NativeStorage::data::<PlayerState>())
        .unwrap();
    other.finish().unwrap();
    let mut contract = module.define_trait("Describe").unwrap();
    contract
        .define_method(MethodDecl::instance("describe").returns(Type::scalar(BuiltinType::String)))
        .unwrap();
    let describe = contract.finish().unwrap();
    module
        .implement(player.clone(), |implementation| {
            implementation.trait_impl(describe.apply([])?, |methods| {
                methods.receiver_access("describe", ReferenceAccess::Shared)?;
                methods.bind(
                    "describe",
                    |_: &mut LoanContext<'_>, (state,): (&PlayerState,)| {
                        Ok(HostDescribe::describe(state))
                    },
                )?;
                assert!(
                    methods
                        .receiver_access("describe", ReferenceAccess::Mutable)
                        .is_err()
                );
                Ok(())
            })
        })
        .unwrap();
    let damage = module
        .implement(player.clone(), |implementation| {
            let receiver = implementation.receiver();
            implementation.inherent_impl(|methods| {
                let new = methods.define_method(
                    MethodDecl::static_method("new")
                        .parameter("hp", Type::i32())
                        .returns(receiver),
                )?;
                methods.bind(new, |cx: &mut NativeContext<'_>, (hp,): (i32,)| {
                    cx.create_opaque(PlayerState {
                        hp,
                        position: [0.0; 3],
                    })
                })?;
                let damage = methods.define_method(
                    MethodDecl::instance("damage")
                        .receiver_access(ReferenceAccess::Mutable)
                        .parameter("amount", Type::i32())
                        .returns(Type::i32()),
                )?;
                methods.bind(
                    damage.clone(),
                    |cx: &mut LoanContext<'_>, (state, amount): (&mut PlayerState, i32)| {
                        cx.poll()?;
                        state.hp -= amount;
                        Ok(state.hp)
                    },
                )?;
                let inspect = methods.define_method(
                    MethodDecl::instance("inspect")
                        .receiver_access(ReferenceAccess::Shared)
                        .returns(Type::scalar(BuiltinType::String)),
                )?;
                // An incompatible callback cannot consume the declaration; the
                // correctly shared callback may be bound after this rejection.
                assert!(
                    methods
                        .bind(
                            inspect.clone(),
                            |_: &mut LoanContext<'_>, (_state,): (&mut PlayerState,)| Ok(
                                String::new()
                            )
                        )
                        .is_err()
                );
                assert!(
                    methods
                        .bind(
                            inspect.clone(),
                            |_: &mut NativeContext<'_>, _: Opaque<PlayerState>, (): ()| Ok(
                                String::new()
                            )
                        )
                        .is_err()
                );
                methods.bind(
                    inspect,
                    |cx: &mut LoanContext<'_>, (state,): (&PlayerState,)| {
                        cx.poll()?;
                        Ok(format!("hp={}", state.hp))
                    },
                )?;
                let echo = methods.define_method(MethodDecl::instance("echo"))?;
                methods.method(&echo, |signature| {
                    let output = signature.type_parameter("Output")?;
                    signature.parameter("value", output.ty())?;
                    signature.returns(output.ty())?;
                    Ok(())
                })?;
                methods.bind(
                    echo,
                    |_: &mut NativeContext<'_>,
                     _: Opaque<PlayerState>,
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
    use example::methods::{Player, Describe};
    pub fn make() -> Player { Player::new(100) }
    pub fn hit(player: Player, amount: i32) -> i32 { player.damage(amount) }
    pub fn echo(player: Player) -> i32 { player.echo::<i32>(7) }
    pub fn inspect(player: Player) -> String { player.inspect() }
    pub fn describe(player: Player) -> String { player.describe() }
    struct ScriptPlayer {}
    impl Describe for ScriptPlayer { fn describe(self) -> String { "script" } }
    pub fn ordinary() -> String { ScriptPlayer {}.describe() }
    pub fn scoped_inspect(player: Ref<Player>) -> String { player.inspect() }
    pub fn scoped_damage(player: RefMut<Player>) -> i32 { player.damage(1) }
    pub fn scoped_read_mut(player: RefMut<Player>) -> String { player.inspect() }
    pub fn scoped_recurse(player: RefMut<Player>) -> i32 {
        player.damage(1);
        scoped_recurse(player)
    }
"#;

#[test]
fn scoped_members_reject_capability_forgery_and_unregistered_rust_traits() {
    let (module, _, _) = registration();
    let mut providers = kagari_stdlib::catalog::shared().unwrap();
    providers.push(Arc::new(module.to_declaration()));
    for source in [
        "fn reject(value: Ref<Player>) -> i32 { value.damage(1) }",
        "fn reject(value: RefMut<Player>) -> i32 { value.echo::<i32>(1) }",
        "fn need<T: Describe>(value: T) -> String { value.describe() } fn reject(value: Other) -> String { need(value) }",
    ] {
        let analysis = analyze_source(
            &SourceFile::new(
                "reject.kgr",
                format!("use example::methods::{{Player, Other, Describe}}; {source}"),
            ),
            providers.clone(),
        )
        .unwrap();
        assert!(!analysis.diagnostics().is_empty(), "{source}");
    }

    let program = compile_program(SOURCE, Some(&module));
    let root = program.records().root.index();
    let mut forged = program.records().clone();
    let inspect = forged.modules[root]
        .native_imports
        .iter_mut()
        .find(|import| {
            program
                .definitions()
                .resolve(import.instance.declaration)
                .unwrap()
                .last()
                .is_some_and(|part| part.name == "inspect")
        })
        .unwrap();
    inspect.receiver_access = Some(ReferenceAccess::Mutable);
    assert!(verify_program(&forged, program.definitions()).is_err());

    let mut forged = program.records().clone();
    let module = &mut forged.modules[root];
    let damage = module
        .functions
        .iter_mut()
        .find(|function| function.name == "scoped_damage")
        .unwrap();
    for ty in damage
        .metadata
        .semantic
        .params
        .values_mut()
        .chain(damage.metadata.semantic.locals.values_mut())
        .chain(damage.metadata.semantic.registers.values_mut())
    {
        if let Ty::Reference { access, .. } = ty {
            *access = ReferenceAccess::Shared;
        }
    }
    let abi = module
        .public_items
        .iter_mut()
        .find_map(|item| match item {
            PublicItem::Function(abi) if abi.name.ends_with("scoped_damage") => Some(abi),
            _ => None,
        })
        .unwrap();
    let Ty::Reference { access, .. } = &mut abi.params[0].ty else {
        panic!("view parameter")
    };
    *access = ReferenceAccess::Shared;
    assert!(verify_program(&forged, program.definitions()).is_err());
}

#[test]
fn native_inherent_members_support_prepared_calls_and_method_parameters() {
    let (module, declaration, damage_declaration) = registration();
    let (vm, owner) = fixture(SOURCE, Some(&module));
    let runtime = vm.runtime();
    let ty = runtime
        .bind_native_type::<PlayerState>(&owner, &declaration, &[])
        .unwrap();
    let member = ty
        .method_declaration(
            &damage_declaration
                .definitions()
                .resolve(damage_declaration.id())
                .unwrap()
                .to_path(),
        )
        .unwrap();
    let damage = runtime
        .bind_method_declaration::<(i32,), i32>(&member)
        .unwrap();
    let again = runtime.bind_method::<(i32,), i32>(&ty, "damage").unwrap();
    let new = runtime
        .bind_associated_function::<(i32,), Opaque<PlayerState>>(&ty, "new")
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
            .bind_associated_function::<(Opaque<PlayerState>, i32), i32>(&ty, "damage")
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
    assert_eq!(
        vm.execute_typed::<_, String>(&owner, "inspect", (player.clone(),))
            .unwrap(),
        "hp=69"
    );
    assert_eq!(
        vm.execute_typed::<_, String>(&owner, "describe", (player.clone(),))
            .unwrap(),
        "trait hp=69"
    );
    assert_eq!(
        vm.execute_typed::<_, String>(&owner, "ordinary", ())
            .unwrap(),
        "script"
    );
}

#[test]
fn generic_native_impl_groups_preserve_receiver_and_local_binders() {
    let mut module = ModuleBuilder::new(
        "example::generic_methods",
        &StandardDeclarations::default().catalog().unwrap(),
    )
    .unwrap();
    let mut declaration = module.define_type("Holder").unwrap();
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
                methods.bind(new, |cx: &mut NativeContext<'_>, (): ()| {
                    cx.create_opaque(PlayerState {
                        hp: 0,
                        position: [0.0; 3],
                    })
                })
            })?;
            // A second group has a different impl identity; authored receiver parameters
            // must be substituted without overwriting the method's own binders.
            implementation.inherent_impl(|methods| {
                let state = methods.define_method(
                    MethodDecl::instance("state")
                        .receiver_access(ReferenceAccess::Shared)
                        .returns(Type::i32()),
                )?;
                methods.bind(
                    state,
                    |_: &mut LoanContext<'_>, (state,): (&PlayerState,)| Ok(state.hp),
                )?;
                let exchange = methods.define_method(
                    MethodDecl::instance("exchange")
                        .parameter("value", element.ty())
                        .returns(element.ty()),
                )?;
                methods.bind(
                    exchange,
                    |_: &mut NativeContext<'_>,
                     _: Opaque<PlayerState>,
                     (value,): (ScriptValue,)|
                     -> NativeResult<ScriptValue> { Ok(value) },
                )?;
                let choose = methods.define_method(
                    MethodDecl::instance("choose").parameter("input", element.ty()),
                )?;
                methods.method(&choose, |signature| {
                    let output = signature.type_parameter("Output")?;
                    signature.parameter("value", output.ty())?;
                    signature.returns(output.ty())?;
                    Ok(())
                })?;
                methods.bind(
                    choose,
                    |_: &mut NativeContext<'_>,
                     _: Opaque<PlayerState>,
                     (_, value): (ScriptValue, ScriptValue)|
                     -> NativeResult<ScriptValue> { Ok(value) },
                )
            })
        })
        .unwrap();
    let module = module.finish().unwrap();
    let (mut vm, owner) = fixture(
        r#"
        use example::generic_methods::Holder;
        pub fn make() -> Holder<i32> { Holder::new::<i32>() }
        pub fn other() -> Holder<String> { Holder::new::<String>() }
        pub fn scoped<T>(holder: Ref<Holder<T>>) -> i32 { holder.state() }
        pub fn external(holder: Ref<Holder<i32>>) -> i32 { scoped(holder) }
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
    let value: Opaque<PlayerState> = vm.execute_typed(&owner, "make", ()).unwrap();
    let other: Opaque<PlayerState> = vm.execute_typed(&owner, "other", ()).unwrap();
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
    drop(cx);
    drop(conversion);
    let external = PlayerState {
        hp: 17,
        position: [0.0; 3],
    };
    vm.runtime_mut()
        .with_host_scope(|scope| {
            let view = scope.borrow(&ty, &external)?;
            assert_eq!(
                kagari_vm::vm::Vm::execute_scoped(scope, &owner, "external", &[view.value()])?,
                kagari_runtime::value::Value::I32(17)
            );
            Ok(())
        })
        .unwrap();
}

#[test]
fn native_method_requirements_use_selected_generic_evidence() {
    let mut module = ModuleBuilder::new(
        "example::bounded_methods",
        &StandardDeclarations::default().catalog().unwrap(),
    )
    .unwrap();
    let mut contract = module.define_trait("Read").unwrap();
    contract
        .define_method(MethodDecl::instance("read").returns(Type::i32()))
        .unwrap();
    let contract = contract.finish().unwrap();
    let mut declaration = module.define_type("Holder").unwrap();
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
                methods.bind(new, |cx: &mut NativeContext<'_>, (): ()| {
                    cx.create_opaque(PlayerState {
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
                    signature.bound(element.clone(), contract.apply([])?)?;
                    signature.requires(CallableRequirement::method(
                        element,
                        contract.method("read")?,
                    )?)
                })?;
                methods.bind(
                    read,
                    move |cx: &mut NativeContext<'_>,
                          _: Opaque<PlayerState>,
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
    let holder: Opaque<PlayerState> = vm.execute_typed(&owner, "make", ()).unwrap();
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

#[test]
fn taken_opaque_aliases_reject_edits_and_pinned_method_calls_after_reinsertion() {
    let (module, declaration, _) = registration();
    let (vm, owner) = fixture(SOURCE, Some(&module));
    let ty = vm
        .runtime()
        .bind_native_type::<PlayerState>(&owner, &declaration, &[])
        .unwrap();
    let damage = vm
        .runtime()
        .bind_method::<(i32,), i32>(&ty, "damage")
        .unwrap();
    let player: Opaque<PlayerState> = vm.execute_typed(&owner, "make", ()).unwrap();
    let alias = player.clone();
    let mut cx = vm.context(&owner).unwrap();
    let state = player.take(&cx).unwrap();
    assert_eq!(state.hp, 100);
    assert!(alias.edit(&cx, |_| Ok(())).is_err());
    assert!(alias.call(&mut cx, &damage, (1,)).is_err());
    assert!(
        vm.execute_typed::<_, i32>(&owner, "hit", (alias.clone(), 1))
            .is_err()
    );
    let fresh = ty.create(&mut cx, state).unwrap();
    assert_eq!(fresh.call(&mut cx, &damage, (1,)).unwrap(), 99);
    assert!(alias.call(&mut cx, &damage, (1,)).is_err());
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 1);
    drop((player, alias, fresh));
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
    assert_eq!(vm.runtime().gc().stats().current_heap_units, 0);
}
