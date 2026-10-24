//! Bind once, retain objects, await or submit, and call current implementations.
//! This uses an explicitly finite source environment, not the default foundation's
//! unknown protected callback closure, which can reject otherwise valid body updates.
//! Run with `cargo run -p kagari --example host_objects`.
use kagari::{engine::KagariEngine, program::PreparedProgram};
use kagari_bytecode::artifact::KbcArtifact;
use kagari_compiler::{bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir};
use kagari_hir::analysis::AnalysisDatabase;
use kagari_runtime::native::{interfaces::Interface, objects::Object};
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use kagari_stdlib::{modules, namespaces};
use std::{
    future::{Future, poll_fn},
    pin::Pin,
    sync::Arc,
    task::Poll,
};

use tokio::runtime::Builder;

const SOURCE: &str = r#"
use core::future::Future;
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

fn prepare(hp: i32) -> PreparedProgram {
    // Supply only this script and the complete Future storage provider it needs.
    // No provider is trimmed and no protected evidence is removed or overridden;
    // initial preparation and reload both use the normal artifact validator.
    let mut sources = SourceDatabase::default();
    let root = sources
        .set(
            "player.kgr",
            SOURCE.replace("hp: 100", &format!("hp: {hp}")),
            SourceLayer::Base,
        )
        .unwrap();
    let future = modules()
        .unwrap()
        .iter()
        .map(|module| module.to_declaration())
        .find(|module| module.records().identity == namespaces::module("core", "future"))
        .unwrap();
    let mut analysis = AnalysisDatabase::default();
    analysis.set_native_modules(vec![Arc::new(future)]);
    let snapshot = analysis
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let checked = snapshot.check_program(root, &Default::default()).unwrap();
    let mir = lower_program_to_mir(&checked, &Default::default()).unwrap();
    let bytecode = lower_program_to_bytecode(&mir, &Default::default()).unwrap();
    let artifact =
        KbcArtifact::from_verified(bytecode, Default::default(), &Default::default()).unwrap();
    PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default()).unwrap()
}

pub fn main() {
    let engine = KagariEngine::builder().unwrap().build().unwrap();
    let prepared = prepare(100);
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
    let replacement = prepare(200);
    runtime
        .reload_program(&loaded, &replacement, Default::default())
        .unwrap();
    let current = runtime
        .async_function::<(), Object>(&loaded.root(), "make")
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
    // Retained and newly bound functions both select the current implementation;
    // neither the async factory nor the associated constructor needs rebinding.
    let retained_player = executor
        .block_on(runtime.call_async(&make, ()).unwrap())
        .unwrap();
    assert_eq!(runtime.get(&retained_player, &current_hp).unwrap(), 200);
    let constructed = runtime.call(&new, ()).unwrap();
    assert_eq!(runtime.get(&constructed, &current_hp).unwrap(), 200);
    assert_eq!(runtime.call_method(&player, &damage, (4,)).unwrap(), 80);
    // Field descriptors still validate the object's exact layout, while retained
    // methods select current code and accept compatible receivers.
    assert!(runtime.set(&new_player, &hp, 0).is_err());
    assert_eq!(runtime.get(&new_player, &current_hp).unwrap(), 200);
    let healed_new = executor
        .block_on(runtime.call_async_method(&new_player, &heal, (1,)).unwrap())
        .unwrap();
    assert_eq!(runtime.get(&healed_new, &current_hp).unwrap(), 201);
    runtime.set(&new_player, &current_hp, 99).unwrap();
    assert_eq!(runtime.get(&healed_new, &current_hp).unwrap(), 99);
    drop((
        player,
        alias,
        healed,
        fresh_player,
        view,
        same,
        new_player,
        old_player,
        retained_player,
        constructed,
        healed_new,
    ));
    assert_eq!(runtime.runtime().collect_garbage().unwrap().live_objects, 0);
    runtime.close().unwrap();
}
