use kagari_bytecode::{artifact::KbcArtifact, program::BytecodeProgram};
use kagari_compiler::{bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir};
use kagari_hir::analysis::AnalysisDatabase;
use kagari_runtime::{
    Runtime, RuntimeConfig,
    error::RuntimeError,
    frame::types::arguments::TypeArgument,
    gc::GcHeapConfig,
    module::LoadedModule,
    native::{
        binding::NativeResult,
        catalog::DeclarationCatalog,
        collections::vector::ScriptVec,
        conversion::{IntoKagari, KagariType, context::ConversionContext},
        module::NativeModule,
        objects::{Object, ObjectType},
        typed::NativeContext,
        types::Type,
    },
    value::Value,
};
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
};

const SOURCE: &str = r#"
pub struct Player { pub var hp: i32, pub val name: String }
pub struct Other { pub var hp: i32, pub val name: String }
pub struct Pair { pub var first: Player, pub var second: Player }
pub struct Hidden { val secret: i32, pub(super) var internal: i32, pub var visible: i32 }
struct Private { pub var hp: i32 }
pub struct Cell<T> { pub var value: T }
pub struct Node { pub var next: Option<Node> }
pub struct Bag { pub var items: Vec<Option<Node>>, pub var pair: (Node, i32) }
fn layouts() {
    val p = Player { hp: 100, name: "Ada" };
    val o = Other { hp: 100, name: "Ada" };
    val pair = Pair { first: p, second: p };
    val hidden = Hidden { secret: 1, internal: 2, visible: 3 };
    val private = Private { hp: 100 };
    val c = Cell { value: p };
    val n = Node { next: None };
    val bag = Bag { items: Vec::from([Some(n)]), pair: (n, 1) };
}
"#;

fn program(source: &str) -> BytecodeProgram {
    let mut analysis = AnalysisDatabase::default();
    analysis.set_native_modules(kagari_stdlib::catalog::shared());
    let mut sources = SourceDatabase::default();
    let root = sources
        .set("objects.kgr", source.into(), SourceLayer::Base)
        .unwrap();
    let snapshot = analysis
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let checked = snapshot.check_program(root, &Default::default()).unwrap();
    let mir = lower_program_to_mir(&checked, &Default::default()).unwrap();
    let artifact =
        KbcArtifact::from_program(lower_program_to_bytecode(&mir).unwrap(), Default::default())
            .unwrap();
    let bytes = artifact.to_bytes().unwrap();
    KbcArtifact::from_bytes(&bytes)
        .unwrap()
        .into_verified(&Default::default())
        .unwrap()
        .into_bytecode()
        .into_unverified()
}

fn runtime() -> Runtime {
    let mut runtime = Runtime::new(RuntimeConfig {
        gc: GcHeapConfig {
            collection_threshold: Some(1),
        },
        ..Default::default()
    });
    NativeModule::install_all(&kagari_stdlib::modules().unwrap(), &mut runtime).unwrap();
    runtime
}

fn fixture() -> (Runtime, LoadedModule) {
    let mut runtime = runtime();
    let owner = runtime.load_program("objects", program(SOURCE)).unwrap();
    (runtime, owner)
}

fn player(cx: &mut NativeContext<'_>, ty: &ObjectType, hp: i32) -> Object {
    let health = cx.runtime().bind_field::<i32>(ty, "hp").unwrap();
    let name = cx.runtime().bind_field::<String>(ty, "name").unwrap();
    let mut builder = ty.builder().unwrap();
    builder.set(cx, &health, hp).unwrap();
    builder.set(cx, &name, "Ada".to_owned()).unwrap();
    builder.build(cx).unwrap()
}

#[test]
fn prepared_fields_preserve_aliases_and_public_access_after_gc() {
    let (runtime, owner) = fixture();
    let ty = runtime.bind_type(&owner, "Player", &[]).unwrap();
    let hp = runtime.bind_field::<i32>(&ty, "hp").unwrap();
    let name = runtime
        .bind_field_declaration::<String>(&ty.field("name").unwrap())
        .unwrap();
    assert!(runtime.bind_field::<u32>(&ty, "hp").is_err());
    let mut cx = NativeContext::new(&runtime, &owner).unwrap();
    let object = player(&mut cx, &ty, 100);
    let alias = object.clone();
    for remaining in [90, 80, 70] {
        object.set(&mut cx, &hp, remaining).unwrap();
        runtime.collect_garbage().unwrap();
        assert_eq!(alias.get(&mut cx, &hp).unwrap(), remaining);
    }
    assert_eq!(object.get(&mut cx, &name).unwrap(), "Ada");
    assert!(object.set(&mut cx, &name, "Grace".into()).is_err());
    assert_eq!(object.get(&mut cx, &name).unwrap(), "Ada");
    drop(object);
    runtime.collect_garbage().unwrap();
    assert_eq!(alias.get(&mut cx, &hp).unwrap(), 70);
    drop(alias);
    assert_eq!(runtime.collect_garbage().unwrap().live_objects, 0);

    let hidden = runtime.bind_type(&owner, "Hidden", &[]).unwrap();
    assert!(runtime.bind_field::<i32>(&hidden, "secret").is_err());
    assert!(runtime.bind_field::<i32>(&hidden, "internal").is_err());
    assert!(runtime.bind_field::<i32>(&hidden, "visible").is_ok());
    assert!(hidden.builder().is_err());
    assert!(runtime.bind_type(&owner, "Private", &[]).is_err());
}

#[test]
fn object_reads_retain_children_and_failed_construction_releases_temporaries() {
    let (runtime, owner) = fixture();
    let player_type = runtime.bind_type(&owner, "Player", &[]).unwrap();
    let pair_type = runtime.bind_type(&owner, "Pair", &[]).unwrap();
    let first = runtime.bind_field::<Object>(&pair_type, "first").unwrap();
    let second = runtime.bind_field::<Object>(&pair_type, "second").unwrap();
    let hp = runtime.bind_field::<i32>(&player_type, "hp").unwrap();
    let name = runtime.bind_field::<String>(&player_type, "name").unwrap();
    let mut cx = NativeContext::new(&runtime, &owner).unwrap();
    let child = player(&mut cx, &player_type, 42);
    let mut incomplete = pair_type.builder().unwrap();
    incomplete.set(&mut cx, &first, child.clone()).unwrap();
    assert!(incomplete.set(&mut cx, &first, child.clone()).is_err());
    assert!(incomplete.build(&mut cx).is_err());
    // The retained player also owns its separately traced String.
    assert_eq!(runtime.collect_garbage().unwrap().live_objects, 2);
    let mut complete = pair_type.builder().unwrap();
    complete.set(&mut cx, &first, child.clone()).unwrap();
    complete.set(&mut cx, &second, child.clone()).unwrap();
    let pair = complete.build(&mut cx).unwrap();
    let read = pair.get(&mut cx, &first).unwrap();
    drop(child);
    drop(pair);
    // The retained player also owns its separately traced String.
    assert_eq!(runtime.collect_garbage().unwrap().live_objects, 2);
    assert_eq!(read.get(&mut cx, &hp).unwrap(), 42);
    assert_eq!(read.get(&mut cx, &name).unwrap(), "Ada");
    drop(read);
    assert_eq!(runtime.collect_garbage().unwrap().live_objects, 0);
}

#[test]
fn fields_reject_foreign_runtimes_and_unrelated_nominal_types() {
    let (runtime, owner) = fixture();
    let ty = runtime.bind_type(&owner, "Player", &[]).unwrap();
    let other = runtime.bind_type(&owner, "Other", &[]).unwrap();
    let hp = runtime.bind_field::<i32>(&ty, "hp").unwrap();
    let mut cx = NativeContext::new(&runtime, &owner).unwrap();
    let object = player(&mut cx, &other, 10);
    assert!(object.get(&mut cx, &hp).is_err());
    assert!(object.set(&mut cx, &hp, 20).is_err());
    let object = player(&mut cx, &ty, 10);
    let (foreign, foreign_owner) = fixture();
    assert!(foreign.bind_field::<i32>(&ty, "hp").is_err());
    let mut foreign_cx = NativeContext::new(&foreign, &foreign_owner).unwrap();
    assert!(object.get(&mut foreign_cx, &hp).is_err());
    assert!(object.set(&mut foreign_cx, &hp, 20).is_err());
    assert_eq!(object.get(&mut cx, &hp).unwrap(), 10);
}

struct FailAfterGc(Arc<AtomicBool>);

impl KagariType for FailAfterGc {
    fn kagari_type(catalog: &DeclarationCatalog) -> NativeResult<Type> {
        i32::kagari_type(catalog)
    }
}

impl IntoKagari for FailAfterGc {
    fn into_kagari(self, cx: &mut ConversionContext<'_>, _: &TypeArgument) -> NativeResult<Value> {
        self.0.store(true, Ordering::Relaxed);
        cx.runtime().collect_garbage()?;
        Err(RuntimeError::module_validation("converter failure"))
    }
}

#[test]
fn a_failed_conversion_preserves_earlier_commits_and_never_publishes_partial_writes() {
    let (runtime, owner) = fixture();
    let ty = runtime.bind_type(&owner, "Player", &[]).unwrap();
    let hp = runtime.bind_field::<i32>(&ty, "hp").unwrap();
    let failing = runtime.bind_field::<FailAfterGc>(&ty, "hp").unwrap();
    let mut cx = NativeContext::new(&runtime, &owner).unwrap();
    let object = player(&mut cx, &ty, 100);
    object.set(&mut cx, &hp, 90).unwrap();
    let attempted = Arc::new(AtomicBool::new(false));
    assert!(
        object
            .set(&mut cx, &failing, FailAfterGc(attempted.clone()))
            .is_err()
    );
    assert!(attempted.load(Ordering::Relaxed));
    assert_eq!(object.get(&mut cx, &hp).unwrap(), 90);
}

#[test]
fn applied_fields_preserve_nominal_arguments() {
    let (runtime, owner) = fixture();
    let player_type = runtime.bind_type(&owner, "Player", &[]).unwrap();
    let cell_type = runtime
        .bind_type(&owner, "Cell", &[player_type.type_argument().clone()])
        .unwrap();
    let value = runtime.bind_field::<Object>(&cell_type, "value").unwrap();
    let hp = runtime.bind_field::<i32>(&player_type, "hp").unwrap();
    let mut cx = NativeContext::new(&runtime, &owner).unwrap();
    let child = player(&mut cx, &player_type, 11);
    let mut builder = cell_type.builder().unwrap();
    builder.set(&mut cx, &value, child).unwrap();
    let cell = builder.build(&mut cx).unwrap();
    let child = cell.get(&mut cx, &value).unwrap();
    drop(cell);
    runtime.collect_garbage().unwrap();
    assert_eq!(child.get(&mut cx, &hp).unwrap(), 11);
}

#[test]
fn binding_handles_pin_old_versions_but_the_cache_does_not() {
    let (runtime, old) = fixture();
    let ty = runtime.bind_type(&old, "Player", &[]).unwrap();
    let hp = runtime.bind_field::<i32>(&ty, "hp").unwrap();
    let duplicate = runtime.bind_field::<i32>(&ty, "hp").unwrap();
    let object = player(&mut NativeContext::new(&runtime, &old).unwrap(), &ty, 50);
    let candidate = runtime
        .stage_reload_program(&old, "objects", program(SOURCE))
        .unwrap();
    let new = candidate.module().clone();
    runtime.publish_staged_reload(candidate).unwrap();
    let new_ty = runtime.bind_type(&new, "Player", &[]).unwrap();
    let new_hp = runtime.bind_field::<i32>(&new_ty, "hp").unwrap();
    let mut cx = NativeContext::new(&runtime, &new).unwrap();
    runtime.collect_garbage().unwrap();
    assert_eq!(object.get(&mut cx, &hp).unwrap(), 50);
    assert!(object.get(&mut cx, &new_hp).is_err());
    drop(object);
    drop(ty);
    drop(hp);
    assert!(
        !runtime
            .collect_garbage()
            .unwrap()
            .reclaimed_modules
            .contains(&old.key())
    );
    drop(duplicate);
    assert!(
        runtime
            .collect_garbage()
            .unwrap()
            .reclaimed_modules
            .contains(&old.key())
    );
    assert!(runtime.bind_type(&old, "Player", &[]).is_err());
}

#[test]
fn nested_dynamic_handles_store_heap_edges_and_cycles_are_collectible() {
    let (runtime, owner) = fixture();
    let node_type = runtime.bind_type(&owner, "Node", &[]).unwrap();
    let next = runtime
        .bind_field::<Option<Object>>(&node_type, "next")
        .unwrap();
    let bag_type = runtime.bind_type(&owner, "Bag", &[]).unwrap();
    let items = runtime
        .bind_field::<Vec<Option<Object>>>(&bag_type, "items")
        .unwrap();
    let pair = runtime
        .bind_field::<(Object, i32)>(&bag_type, "pair")
        .unwrap();
    assert!(
        runtime
            .bind_field::<Result<Object, String>>(&node_type, "next")
            .is_err()
    );
    assert!(
        runtime
            .bind_field::<(Object, u32)>(&bag_type, "pair")
            .is_err()
    );
    let mut cx = NativeContext::new(&runtime, &owner).unwrap();
    let mut builder = node_type.builder().unwrap();
    builder.set(&mut cx, &next, None).unwrap();
    let node = builder.build(&mut cx).unwrap();
    node.set(&mut cx, &next, Some(node.clone())).unwrap();
    let mut builder = bag_type.builder().unwrap();
    builder
        .set(&mut cx, &items, vec![Some(node.clone()), None])
        .unwrap();
    builder.set(&mut cx, &pair, (node.clone(), 42)).unwrap();
    let bag = builder.build(&mut cx).unwrap();
    let read = bag.get(&mut cx, &items).unwrap();
    let (pair_read, number) = bag.get(&mut cx, &pair).unwrap();
    assert_eq!(number, 42);
    assert!(read[1].is_none());
    drop(node);
    drop(bag);
    runtime.collect_garbage().unwrap();
    assert!(pair_read.get(&mut cx, &next).unwrap().is_some());
    assert!(
        read[0]
            .as_ref()
            .unwrap()
            .get(&mut cx, &next)
            .unwrap()
            .is_some()
    );
    drop(pair_read);
    drop(read);
    assert_eq!(runtime.collect_garbage().unwrap().live_objects, 0);
}

#[test]
fn collection_handles_preserve_identity_access_and_iteration_cleanup() {
    let (runtime, owner) = fixture();
    let mut cx = NativeContext::new(&runtime, &owner).unwrap();
    let values = cx.create_vec(vec![1i32, 2, 3]).unwrap();
    let alias = values.clone();
    let readonly = values.read_only();
    values.set(&mut cx, 0, 10).unwrap();
    values.push(&mut cx, 4).unwrap();
    values.insert(&mut cx, 1, 20).unwrap();
    assert_eq!(alias.remove(&mut cx, 2).unwrap(), Some(2));
    assert_eq!(alias.pop(&mut cx).unwrap(), Some(4));
    assert_eq!(readonly.get(&mut cx, 0).unwrap(), Some(10));
    assert!(readonly.push(&mut cx, 5).is_err());
    assert!(readonly.set(&mut cx, 0, 5).is_err());
    assert!(readonly.clear(&cx).is_err());
    assert_eq!(values.get(&mut cx, 99).unwrap(), None);
    let mut seen = Vec::new();
    values
        .for_each(&mut cx, |cx, value| {
            cx.collect_garbage()?;
            assert!(alias.push(cx, 99).is_err());
            seen.push(value);
            Ok(())
        })
        .unwrap();
    assert_eq!(seen, [10, 20, 3]);
    assert!(
        values
            .for_each(&mut cx, |_, _| Err(RuntimeError::module_validation("stop")))
            .is_err()
    );
    // The failed callback released its structural-mutation exclusion.
    alias.push(&mut cx, 4).unwrap();
    values.truncate(&cx, 1).unwrap();
    assert_eq!(alias.len(&cx).unwrap(), 1);
    drop(alias);
    drop(values);
    runtime.collect_garbage().unwrap();
    assert_eq!(readonly.get(&mut cx, 0).unwrap(), Some(10));
    drop(readonly);
    assert_eq!(runtime.collect_garbage().unwrap().live_objects, 0);
}

#[test]
fn collection_removals_return_retained_objects_and_reject_readonly_widening() {
    let (runtime, owner) = fixture();
    let node_type = runtime.bind_type(&owner, "Node", &[]).unwrap();
    let next = runtime
        .bind_field::<Option<Object>>(&node_type, "next")
        .unwrap();
    let mut cx = NativeContext::new(&runtime, &owner).unwrap();
    let mut builder = node_type.builder().unwrap();
    builder.set(&mut cx, &next, None).unwrap();
    let node = builder.build(&mut cx).unwrap();
    let values = cx
        .create_vec_with_type(node_type.type_argument().clone(), vec![node])
        .unwrap();
    let read = values.pop(&mut cx).unwrap().unwrap();
    runtime.collect_garbage().unwrap();
    assert!(read.get(&mut cx, &next).unwrap().is_none());
    let bag_type = runtime.bind_type(&owner, "Bag", &[]).unwrap();
    let items = runtime
        .bind_field::<ScriptVec<Option<Object>>>(&bag_type, "items")
        .unwrap();
    let pair = runtime
        .bind_field::<(Object, i32)>(&bag_type, "pair")
        .unwrap();
    let option_node = runtime
        .bind_field::<Vec<Option<Object>>>(&bag_type, "items")
        .unwrap();
    let mut builder = bag_type.builder().unwrap();
    builder
        .set(&mut cx, &option_node, vec![Some(read.clone())])
        .unwrap();
    builder.set(&mut cx, &pair, (read.clone(), 1)).unwrap();
    let bag = builder.build(&mut cx).unwrap();
    let shared = bag.get(&mut cx, &items).unwrap();
    let readonly = shared.read_only();
    assert!(bag.set(&mut cx, &items, readonly.clone()).is_err());
    shared.push(&mut cx, None).unwrap();
    assert_eq!(bag.get(&mut cx, &items).unwrap().len(&cx).unwrap(), 2);
    let (foreign, foreign_owner) = fixture();
    let mut foreign_cx = NativeContext::new(&foreign, &foreign_owner).unwrap();
    assert!(shared.get(&mut foreign_cx, 0).is_err());
    assert!(shared.push(&mut foreign_cx, None).is_err());
    drop(bag);
    drop(shared);
    drop(readonly);
    drop(values);
    drop(read);
    assert_eq!(runtime.collect_garbage().unwrap().live_objects, 0);
}

#[test]
fn typed_handles_and_bindings_move_with_exclusive_runtime_ownership() {
    let (runtime, owner) = fixture();
    let ty = runtime.bind_type(&owner, "Player", &[]).unwrap();
    let hp = runtime.bind_field::<i32>(&ty, "hp").unwrap();
    let (object, values) = {
        let mut cx = NativeContext::new(&runtime, &owner).unwrap();
        (
            player(&mut cx, &ty, 42),
            cx.create_vec(vec![1i32, 2]).unwrap(),
        )
    };
    thread::spawn(move || {
        let mut cx = NativeContext::new(&runtime, &owner).unwrap();
        runtime.collect_garbage().unwrap();
        object.set(&mut cx, &hp, 43).unwrap();
        values.push(&mut cx, 3).unwrap();
        assert_eq!(object.get(&mut cx, &hp).unwrap(), 43);
        assert_eq!(values.pop(&mut cx).unwrap(), Some(3));
        drop(object);
        drop(values);
        assert_eq!(runtime.collect_garbage().unwrap().live_objects, 0);
        (ty, hp)
        // The receiving thread destroys storage; immutable bindings may outlive it.
    })
    .join()
    .unwrap();
}
