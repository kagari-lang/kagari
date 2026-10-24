//! Bind once, retain objects, await directly or submit, and keep old generations pinned.
//! Run with `cargo run -p kagari --example host_objects`.
use kagari::{engine::KagariEngine, program::PreparedProgram};
use kagari_runtime::native::{interfaces::Interface, objects::Object};
use kagari_source::source::SourceFile;
use std::{
    future::{Future, poll_fn},
    pin::Pin,
    task::Poll,
};

use tokio::runtime::Builder;

const SOURCE: &str = r#"
pub struct Player { pub var hp: i32 }
impl Player {
    pub fn new() -> Player { Player { hp: 100 } }
    pub fn damage(self, amount: i32) -> i32 { self.hp -= amount; self.hp }
    pub async fn heal(self, amount: i32) -> Player { self.hp += amount; self }
    pub async fn fresh() -> Player { Player::new() }
}
pub trait Reader {
    type Item;
    fn read(self) -> i32;
    fn later(self) -> Future<Self::Item>;
}
impl Reader for Player {
    type Item = Player;
    fn read(self) -> i32 { self.hp }
    fn later(self) -> Future<Player> { async { self } }
}
pub fn reader(player: Player) -> Reader<Item = Player> { player }
pub fn roundtrip(player: Player) -> Player { player }
pub async fn roundtrip_async(player: Player) -> Player { player }
pub async fn make() -> Player { Player::new() }
// Concrete member applications remain the compiler's responsibility.
pub async fn evidence() -> i32 {
    val p = Player::fresh().await;
    val q = p.heal(1).await;
    q.damage(1)
}
"#;

fn prepare(engine: &KagariEngine, hp: i32) -> PreparedProgram {
    engine
        .compile_to_program(
            SourceFile::new(
                "player.kgr",
                SOURCE.replace("hp: 100", &format!("hp: {hp}")),
            ),
            Default::default(),
        )
        .unwrap()
}

pub fn main() {
    let engine = KagariEngine::builder().unwrap().build().unwrap();
    let prepared = prepare(&engine, 100);
    let mut runtime = engine.runtime(Default::default());
    let loaded = runtime.load_program(&prepared, Default::default()).unwrap();
    let player_type = runtime.object_type(&loaded.root(), "Player").unwrap();
    let new = runtime
        .associated_function::<(), Object>(&player_type, "new")
        .unwrap();
    let make = runtime
        .async_function::<(), Object>(&loaded.root(), "make")
        .unwrap();
    let hp = runtime.field::<i32>(&player_type, "hp").unwrap();
    let damage = runtime
        .method::<(i32,), i32>(&player_type, "damage")
        .unwrap();
    let heal = runtime
        .async_method::<(i32,), Object>(&player_type, "heal")
        .unwrap();
    let fresh = runtime
        .async_associated_function::<(), Object>(&player_type, "fresh")
        .unwrap();
    let roundtrip = runtime
        .function::<(Object,), Object>(&loaded.root(), "roundtrip")
        .unwrap();
    let reader = runtime
        .function::<(Object,), Interface>(&loaded.root(), "reader")
        .unwrap();
    let player = runtime.call(&new, ()).unwrap();
    let alias = runtime.call(&roundtrip, (player.clone(),)).unwrap();
    assert_eq!(runtime.get(&player, &hp).unwrap(), 100);
    runtime.set(&alias, &hp, 90).unwrap();
    assert_eq!(runtime.call_method(&player, &damage, (10,)).unwrap(), 80);

    let executor = Builder::new_current_thread().build().unwrap();
    let healed = executor
        .block_on(runtime.call_async_method(&player, &heal, (5,)).unwrap())
        .unwrap();
    assert_eq!(runtime.get(&healed, &hp).unwrap(), 85);
    let fresh_player = executor
        .block_on(runtime.call_async(&fresh, ()).unwrap())
        .unwrap();
    assert_eq!(runtime.get(&fresh_player, &hp).unwrap(), 100);
    // Script supplies the Interface; there is no arbitrary Rust Object-to-trait cast.
    let view = runtime.call(&reader, (player.clone(),)).unwrap();
    let read = runtime.interface_method::<(), i32>(&view, "read").unwrap();
    let later = runtime
        .async_interface_method::<(), Object>(&view, "later")
        .unwrap();
    assert_eq!(runtime.call_interface_method(&view, &read, ()).unwrap(), 85);
    let same = executor
        .block_on(
            runtime
                .call_async_interface_method(&view, &later, ())
                .unwrap(),
        )
        .unwrap();
    runtime.set(&same, &hp, 84).unwrap();
    assert_eq!(runtime.get(&alias, &hp).unwrap(), 84);

    // Submitted arguments and callable authority are retained before publication.
    let echo = runtime
        .async_function::<(Object,), Object>(&loaded.root(), "roundtrip_async")
        .unwrap();
    let argument = runtime.call(&new, ()).unwrap();
    let mut pending = runtime.submit_async(&echo, (argument.clone(),)).unwrap();
    runtime.set(&argument, &hp, 123).unwrap();
    drop(argument);
    runtime.runtime().collect_garbage().unwrap();
    let replacement = prepare(&engine, 200);
    runtime
        .reload_program(&loaded, &replacement, Default::default())
        .unwrap();
    let reloaded = loaded.clone();
    let current = runtime
        .async_function::<(), Object>(&reloaded.root(), "make")
        .unwrap();
    let new_player = executor
        .block_on(runtime.call_async(&current, ()).unwrap())
        .unwrap();
    let current_hp = runtime
        .field::<i32>(new_player.object_type(), "hp")
        .unwrap();
    assert_eq!(runtime.get(&new_player, &current_hp).unwrap(), 200);
    // PendingCall is result-only. An application driver must poll Runtime; direct
    // Call above already made shared progress, but this also handles unfinished work.
    let old_player = executor
        .block_on(poll_fn(|cx| {
            if let Poll::Ready(Err(error)) = runtime.poll(cx) {
                return Poll::Ready(Err(error));
            }
            Pin::new(&mut pending).poll(cx)
        }))
        .unwrap();
    assert_eq!(runtime.get(&old_player, &hp).unwrap(), 123);
    let pinned = executor
        .block_on(runtime.call_async(&make, ()).unwrap())
        .unwrap();
    assert_eq!(runtime.get(&pinned, &hp).unwrap(), 100);
    assert_eq!(runtime.call_method(&player, &damage, (4,)).unwrap(), 80);
    assert!(runtime.call_async_method(&new_player, &heal, (1,)).is_err());
    assert!(runtime.set(&new_player, &hp, 0).is_err());
    assert_eq!(runtime.get(&new_player, &current_hp).unwrap(), 200);
    drop((
        player,
        alias,
        healed,
        fresh_player,
        view,
        same,
        new_player,
        old_player,
        pinned,
    ));
    assert_eq!(runtime.runtime().collect_garbage().unwrap().live_objects, 0);
    runtime.close().unwrap();
}
