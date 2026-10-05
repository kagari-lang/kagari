//! Prepare field/method bindings once, then work with retained script objects.
use kagari_embed::{context::ExecutionContext, engine::KagariEngine, program::PreparedProgram};
use kagari_runtime::native::objects::Object;
use kagari_source::source::SourceFile;

fn main() {
    let engine = KagariEngine::builder().unwrap().build().unwrap();
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new(
                "player.kgr",
                r#"
            pub struct Player { pub var hp: i32 }
            impl Player {
                pub fn on_damage(self, amount: i32) -> i32 {
                    self.hp = self.hp - amount;
                    self.hp
                }
            }
            pub fn make() -> Player { Player { hp: 100 } }
            pub fn play() -> i32 { make().on_damage(1) }
        "#,
            ),
            Default::default(),
        )
        .unwrap();
    let prepared =
        PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
    let execution = ExecutionContext::default();
    let mut runtime = engine.runtime(execution.clone());
    let loaded = runtime.load_program(&prepared, Default::default()).unwrap();
    let player: Object = runtime
        .execute_typed(&loaded, "make", (), &execution)
        .unwrap();
    let hp = runtime
        .runtime()
        .bind_field::<i32>(player.object_type(), "hp")
        .unwrap();
    let damage = runtime
        .runtime()
        .bind_method::<(i32,), i32>(player.object_type(), "on_damage")
        .unwrap();

    let alias = player.clone();
    runtime.runtime().collect_garbage().unwrap();
    runtime
        .with_context(&loaded, &execution, |cx| {
            assert_eq!(player.get(cx, &hp)?, 100);
            assert_eq!(player.call(cx, &damage, (10,))?, 90);
            alias.set(cx, &hp, 80)?;
            assert_eq!(player.call(cx, &damage, (5,))?, 75);
            Ok(())
        })
        .unwrap();
    drop(alias);
    drop(player);
    assert_eq!(runtime.runtime().collect_garbage().unwrap().live_objects, 0);
}
