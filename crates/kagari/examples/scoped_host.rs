//! One registered method set serves runtime-owned and externally borrowed players.
use kagari::{context::ExecutionContext, engine::KagariEngine, program::PreparedProgram};
use kagari_runtime::{
    native::{
        builder::ModuleBuilder,
        declarations::MethodDecl,
        payload::Opaque,
        storage::NativeStorage,
        typed::{NativeContext, loans::LoanContext},
        types::Type,
    },
    value::Value,
};
use kagari_source::source::SourceFile;
use kagari_types::reference::ReferenceAccess;
use kagari_vm::vm::Vm;

kagari_runtime::native_data! { struct Player { hp: i32 } }

fn main() {
    let mut builder = KagariEngine::builder().unwrap();
    let mut module = ModuleBuilder::new("game::actors", builder.declarations()).unwrap();
    let mut declaration = module.define_type("Player").unwrap();
    declaration
        .native_storage(NativeStorage::data::<Player>())
        .unwrap();
    let player = declaration.finish().unwrap();
    module
        .implement(player.clone(), |implementation| {
            implementation.inherent_impl(|methods| {
                let heal = methods.define_method(
                    MethodDecl::instance("heal")
                        .receiver_access(ReferenceAccess::Mutable)
                        .parameter("amount", Type::i32())
                        .returns(Type::i32()),
                )?;
                methods.bind(
                    heal,
                    |cx: &mut LoanContext<'_>, (player, amount): (&mut Player, i32)| {
                        cx.poll()?;
                        player.hp += amount;
                        Ok(player.hp)
                    },
                )?;
                let inspect = methods.define_method(
                    MethodDecl::instance("inspect")
                        .receiver_access(ReferenceAccess::Shared)
                        .returns(Type::i32()),
                )?;
                methods.bind(inspect, |_: &mut LoanContext<'_>, (player,): (&Player,)| {
                    Ok(player.hp)
                })
            })
        })
        .unwrap();
    builder.install(module.finish().unwrap()).unwrap();
    let engine = builder.build().unwrap();
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new(
                "scoped.kgr",
                r#"
        use game::actors::Player;
        pub fn owned(player: Player) -> i32 { player.heal(10) }
        pub fn borrowed(player: RefMut<Player>) -> i32 { player.heal(10) }
        pub fn inspect(player: Ref<Player>) -> i32 { player.inspect() }
    "#,
            ),
            Default::default(),
        )
        .unwrap();
    let program =
        PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime.load_program(&program, Default::default()).unwrap();
    let descriptor = runtime
        .runtime()
        .bind_native_type::<Player>(&loaded, &player, &[])
        .unwrap();
    let owned: Opaque<Player> = descriptor
        .create(
            &mut NativeContext::new(runtime.runtime(), &loaded).unwrap(),
            Player { hp: 60 },
        )
        .unwrap();
    let owned_hp: i32 = runtime
        .execute_typed(&loaded, "owned", (owned,), &context)
        .unwrap();
    let mut external = Player { hp: 60 };
    let borrowed_hp = runtime
        .runtime_mut()
        .with_host_scope(|scope| {
            let view = scope.borrow_mut(&descriptor, &mut external)?;
            let Value::I32(hp) = Vm::execute_scoped(scope, &loaded, "borrowed", &[view.value()])?
            else {
                unreachable!()
            };
            assert_eq!(
                Vm::execute_scoped(scope, &loaded, "inspect", &[view.value()])?,
                Value::I32(hp)
            );
            Ok(hp)
        })
        .unwrap();
    assert_eq!(owned_hp, 70);
    assert_eq!(borrowed_hp, 70);
    assert_eq!(external.hp, 70);
    println!("owned hp={owned_hp}, borrowed hp={borrowed_hp}");
}
