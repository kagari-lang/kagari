//! One registered method set serves runtime-owned and externally borrowed players.
use kagari::runtime::Runtime;
use kagari::{context::ExecutionContext, engine::KagariEngine};
use kagari_runtime::native::{
    builder::ModuleBuilder,
    declarations::MethodDecl,
    payload::Opaque,
    storage::NativeStorage,
    typed::{NativeContext, loans::LoanContext},
    types::Type,
};
use kagari_source::source::SourceFile;
use kagari_stdlib as stdlib;
use kagari_types::reference::ReferenceAccess;

kagari_runtime::native_data! { struct Player { hp: i32 } }

fn main() {
    let mut builder = KagariEngine::builder().unwrap();
    builder.install_all(stdlib::modules().unwrap()).unwrap();
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
    let program = engine
        .compile_to_program(
            SourceFile::new(
                "scoped.kgr",
                include_str!("../../../examples/scoped-host.kgr"),
            ),
            Default::default(),
        )
        .unwrap();

    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime.load_program(&program, Default::default()).unwrap();
    let selected = runtime.select_program(&loaded).unwrap();
    let descriptor = runtime
        .bind_native_type::<Player>(&loaded.root(), &player, &[])
        .unwrap();
    let owned: Opaque<Player> = descriptor
        .create(
            &mut NativeContext::new(&mut runtime, &selected).unwrap(),
            Player { hp: 60 },
        )
        .unwrap();
    let owned_hp: i32 = runtime
        .execute_typed(&loaded, "owned", (owned.clone(),), &context)
        .unwrap();
    let mut external = Player { hp: 60 };
    let borrowed_hp = runtime
        .with_host_scope(|scope| {
            let view = scope.borrow_mut(descriptor.rust_type(), &mut external)?;
            let hp =
                Runtime::execute_scoped(scope, &loaded, "borrowed", &[view.value()], &context)?
                    .as_i32()
                    .expect("checked hp result");
            assert_eq!(
                Runtime::execute_scoped(scope, &loaded, "inspect", &[view.value()], &context)?
                    .as_i32(),
                Some(hp)
            );
            Ok(hp)
        })
        .unwrap();
    assert_eq!(owned_hp, 70);
    assert_eq!(borrowed_hp, 70);
    assert_eq!(external.hp, 70);
    runtime
        .with_host_scope(|scope| {
            let view = scope.borrow(descriptor.rust_type(), &external)?;
            assert_eq!(
                Runtime::execute_scoped(scope, &loaded, "inspect", &[view.value()], &context)?
                    .as_i32(),
                Some(70)
            );
            Ok(())
        })
        .unwrap();
    let mut cx = NativeContext::new(&mut runtime, &selected).unwrap();
    assert_eq!(owned.read(&mut cx, |player| Ok(player.hp)).unwrap(), 70);
    owned
        .edit(&mut cx, |player| {
            player.hp += 1;
            Ok(())
        })
        .unwrap();
    let alias = owned.clone();
    let alias_hp = alias.read(&mut cx, |player| Ok(player.hp)).unwrap();
    assert_eq!(alias_hp, 71);
    println!("owned hp={owned_hp}, borrowed hp={borrowed_hp}");
    println!("shared opaque alias hp={alias_hp}");
}
