mod managed;
mod methods;
use crate::{compile_program, native_boundary_functions::fixture};
use kagari_runtime::{
    error::RuntimeError,
    native::{
        binding::NativeResult,
        builder::ModuleBuilder,
        conversion::context::ConversionContext,
        declarations::FunctionDecl,
        module::NativeModule,
        payload::NativeObject,
        storage::{NativePayload, NativeStorage},
        typed::NativeContext,
        types::{Type, TypeRef},
    },
    value::Value,
};
use kagari_stdlib::declarations::StandardDeclarations;
use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    slice,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
};

kagari_runtime::native_data! {
    struct PlayerState { hp: i32, position: [f32; 3] }
}

#[test]
fn checked_data_edits_preserve_writes_and_release_exclusive_borrows_on_failure() {
    let mut module = ModuleBuilder::new(
        "example::data",
        &StandardDeclarations::default().catalog().unwrap(),
    );
    let mut declaration = module.define_type("Player");
    declaration
        .native_storage(NativeStorage::data::<PlayerState>())
        .unwrap();
    let player_type = declaration.finish().unwrap();
    let mut declaration = module.define_type("ReadOnlyPayload");
    declaration
        .native_storage(NativeStorage::payload::<PlayerState>())
        .unwrap();
    let no_edit = declaration.finish().unwrap();
    let new = module
        .define_function(FunctionDecl::new("new").returns(player_type.apply([]).unwrap()))
        .unwrap();
    module
        .bind_typed(
            new,
            |cx: &mut NativeContext<'_>, (): ()| -> NativeResult<NativeObject<PlayerState>> {
                cx.create_native(PlayerState {
                    hp: 100,
                    position: [0.0; 3],
                })
            },
        )
        .unwrap();
    let damage = module
        .define_function(
            FunctionDecl::new("damage")
                .parameter("player", player_type.apply([]).unwrap())
                .parameter("amount", Type::i32())
                .returns(Type::i32()),
        )
        .unwrap();
    module
        .bind_typed(
            damage,
            |cx: &mut NativeContext<'_>,
             (player, damage): (NativeObject<PlayerState>, i32)|
             -> NativeResult<i32> {
                player.edit(cx, |state| {
                    state.hp = state.hp.saturating_sub(damage);
                    Ok(())
                })?;
                cx.collect_garbage()?;
                player.read(cx, |state| Ok(state.hp))
            },
        )
        .unwrap();
    let (vm, owner) = fixture(
        r#"
        use example::data::{Player, new, damage};
        pub fn make() -> Player { new() }
        pub fn hit(player: Player, amount: i32) -> i32 { damage(player, amount) }
    "#,
        Some(&module.finish().unwrap()),
    );
    let object: NativeObject<PlayerState> = vm.execute_typed(&owner, "make", ()).unwrap();
    let alias = object.clone();
    let mut cx = vm.context(&owner).unwrap();
    let before = vm.runtime().gc().stats().current_heap_units;
    object
        .edit(&cx, |state| {
            state.position[0] = 2.0;
            assert!(alias.read(&cx, |_| Ok(())).is_err());
            assert!(alias.edit(&cx, |_| Ok(())).is_err());
            assert!(cx.collect_garbage().is_err());
            assert!(
                vm.execute_typed::<_, i32>(&owner, "hit", (alias.clone(), 1))
                    .is_err()
            );
            Ok(())
        })
        .unwrap();
    assert!(
        object
            .edit::<()>(&cx, |state| {
                state.hp = 90;
                Err(RuntimeError::module_validation("after write"))
            })
            .is_err()
    );
    assert_eq!(alias.read(&cx, |state| Ok(state.hp)).unwrap(), 90);
    assert!(
        catch_unwind(AssertUnwindSafe(|| {
            let _ = object.edit::<()>(&cx, |state| {
                state.hp = 80;
                panic!("after write")
            });
        }))
        .is_err()
    );
    assert_eq!(
        alias
            .read(&cx, |state| Ok((state.hp, state.position[0])))
            .unwrap(),
        (80, 2.0)
    );
    assert_eq!(
        vm.execute_typed::<_, i32>(&owner, "hit", (alias.clone(), 38))
            .unwrap(),
        42
    );
    assert_eq!(vm.runtime().gc().stats().current_heap_units, before);
    let no_edit = vm
        .runtime()
        .bind_native_type::<PlayerState>(&owner, &no_edit, &[])
        .unwrap()
        .create(
            &mut cx,
            PlayerState {
                hp: 1,
                position: [0.0; 3],
            },
        )
        .unwrap();
    assert!(no_edit.edit(&cx, |_| Ok(())).is_err());
    assert_eq!(no_edit.read(&cx, |state| Ok(state.hp)).unwrap(), 1);
    drop((object, alias, no_edit, cx));
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
    assert_eq!(vm.runtime().gc().active_roots(), 0);
}

#[derive(Debug)]
struct Counter {
    value: i32,
    dropped: Arc<AtomicUsize>,
}

impl Drop for Counter {
    fn drop(&mut self) {
        self.dropped.fetch_add(1, Ordering::Relaxed);
    }
}

impl NativePayload for Counter {
    fn trace<'payload>(&'payload self, _: &mut dyn FnMut(&'payload Value)) {}

    fn units(&self) -> usize {
        1
    }
}

#[derive(Debug)]
struct Edge(Value);

impl NativePayload for Edge {
    fn trace<'payload>(&'payload self, visit: &mut dyn FnMut(&'payload Value)) {
        visit(&self.0);
    }

    fn units(&self) -> usize {
        1
    }
}

fn module(dropped: Arc<AtomicUsize>) -> (NativeModule, TypeRef, TypeRef, TypeRef) {
    let mut module = ModuleBuilder::new(
        "example::payload",
        &StandardDeclarations::default().catalog().unwrap(),
    );
    let mut declaration = module.define_type("Counter");
    declaration
        .native_storage(NativeStorage::payload::<Counter>())
        .unwrap();
    let counter = declaration.finish().unwrap();
    let mut declaration = module.define_type("Other");
    declaration
        .native_storage(NativeStorage::payload::<Counter>())
        .unwrap();
    let other = declaration.finish().unwrap();
    let mut declaration = module.define_type("Edge");
    declaration.type_parameter("T").unwrap();
    declaration
        .native_storage(NativeStorage::payload::<Edge>())
        .unwrap();
    let edge = declaration.finish().unwrap();
    let new = module
        .define_function(
            FunctionDecl::new("new")
                .parameter("value", Type::i32())
                .returns(counter.apply([]).unwrap()),
        )
        .unwrap();
    module
        .bind_typed(
            new,
            move |cx: &mut NativeContext<'_>,
                  (value,): (i32,)|
                  -> NativeResult<NativeObject<Counter>> {
                cx.create_native(Counter {
                    value,
                    dropped: dropped.clone(),
                })
            },
        )
        .unwrap();
    let read = module
        .define_function(
            FunctionDecl::new("read")
                .parameter("counter", counter.apply([]).unwrap())
                .returns(Type::i32()),
        )
        .unwrap();
    module
        .bind_typed(
            read,
            |cx: &mut NativeContext<'_>,
             (counter,): (NativeObject<Counter>,)|
             -> NativeResult<i32> {
                cx.collect_garbage()?;
                counter.read(cx, |counter| Ok(counter.value))
            },
        )
        .unwrap();
    (module.finish().unwrap(), counter, other, edge)
}

const SOURCE: &str = r#"
    use example::payload::{Counter, new, read};
    pub fn make(value: i32) -> Counter { new(value) }
    pub fn value(counter: Counter) -> i32 { read(counter) }
"#;

#[test]
fn retained_native_factories_share_aliases_and_check_nominal_and_runtime_identity() {
    let dropped = Arc::new(AtomicUsize::new(0));
    let (module, declaration, other, _) = module(dropped.clone());
    let (vm, owner) = fixture(SOURCE, Some(&module));
    let runtime = vm.runtime();
    let native = runtime
        .bind_native_type::<Counter>(&owner, &declaration, &[])
        .unwrap();
    assert!(
        runtime
            .bind_native_type::<Edge>(&owner, &declaration, &[])
            .is_err()
    );
    let read = runtime
        .bind_function::<(NativeObject<Counter>,), i32>(&owner, "value")
        .unwrap();
    assert!(
        runtime
            .bind_function::<(NativeObject<Edge>,), i32>(&owner, "value")
            .is_err()
    );
    let mut cx = vm.context(&owner).unwrap();
    let object = native
        .create(
            &mut cx,
            Counter {
                value: 42,
                dropped: dropped.clone(),
            },
        )
        .unwrap();
    let alias = object.clone();
    drop(object);
    cx.collect_garbage().unwrap();
    assert_eq!(alias.read(&cx, |state| Ok(state.value)).unwrap(), 42);
    assert_eq!(vm.call(&read, (alias.clone(),)).unwrap(), 42);
    let returned: NativeObject<Counter> = vm.execute_typed(&owner, "make", (43,)).unwrap();
    assert_eq!(vm.call(&read, (returned,)).unwrap(), 43);
    let wrong = runtime
        .bind_native_type::<Counter>(&owner, &other, &[])
        .unwrap()
        .create(
            &mut cx,
            Counter {
                value: 0,
                dropped: dropped.clone(),
            },
        )
        .unwrap();
    assert!(vm.call(&read, (wrong,)).is_err());
    let (foreign, foreign_owner) = fixture(SOURCE, Some(&module));
    let mut foreign_cx = foreign.context(&foreign_owner).unwrap();
    assert!(alias.read(&foreign_cx, |_| Ok(())).is_err());
    assert!(foreign.call(&read, (alias.clone(),)).is_err());
    assert!(
        native
            .create(
                &mut foreign_cx,
                Counter {
                    value: 0,
                    dropped: dropped.clone()
                }
            )
            .is_err()
    );
    drop((cx, alias));
    assert_eq!(runtime.collect_garbage().unwrap().live_objects, 0);
    assert_eq!(runtime.gc().active_roots(), 0);
    assert_eq!(dropped.load(Ordering::Relaxed), 4);
}

#[test]
fn native_borrows_reject_reentry_and_release_on_error_and_unwind() {
    let (module, _, _, _) = module(Arc::new(AtomicUsize::new(0)));
    let (vm, owner) = fixture(SOURCE, Some(&module));
    let object: NativeObject<Counter> = vm.execute_typed(&owner, "make", (42,)).unwrap();
    let mut cx = vm.context(&owner).unwrap();
    assert!(
        object
            .read(&cx, |_| {
                assert!(cx.collect_garbage().is_err());
                assert!(object.read(&cx, |_| Ok(())).is_err());
                assert!(
                    vm.execute_typed::<_, NativeObject<Counter>>(&owner, "make", (0,))
                        .is_err()
                );
                Err::<(), _>(RuntimeError::module_validation("after inspection"))
            })
            .is_err()
    );
    assert!(
        catch_unwind(AssertUnwindSafe(|| {
            let _ = object.read::<()>(&cx, |_| panic!("borrow unwind"));
        }))
        .is_err()
    );
    cx.collect_garbage().unwrap();
    assert_eq!(object.read(&cx, |state| Ok(state.value)).unwrap(), 42);
    assert!(
        cx.create_native(Counter {
            value: 0,
            dropped: Arc::new(AtomicUsize::new(0))
        })
        .is_err()
    );
    drop((object, cx));
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
}

#[test]
fn payload_factories_trace_children_and_reject_foreign_edges_before_publication() {
    let (module, _, _, edge) = module(Arc::new(AtomicUsize::new(0)));
    let (vm, owner) = fixture(SOURCE, Some(&module));
    let element = ConversionContext::new(vm.runtime(), &owner)
        .unwrap()
        .type_for::<i32>()
        .unwrap();
    assert!(
        vm.runtime()
            .bind_native_type::<Edge>(&owner, &edge, &[])
            .is_err()
    );
    let native = vm
        .runtime()
        .bind_native_type_declaration::<Edge>(&owner, edge.id(), &[element])
        .unwrap();
    let mut cx = vm.context(&owner).unwrap();
    let child: NativeObject<Counter> = vm.execute_typed(&owner, "make", (42,)).unwrap();
    // Handwritten NativePayload is an advanced tracing boundary. The typed
    // native handle API itself never exposes a mutable traced field.
    let root = ConversionContext::new(vm.runtime(), &owner)
        .unwrap()
        .encode_value(child.native_type().type_argument(), child.clone())
        .unwrap();
    let parent = native.create(&mut cx, Edge(root)).unwrap();
    drop(child);
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 2);
    let (foreign, foreign_owner) = fixture(SOURCE, Some(&module));
    let child: NativeObject<Counter> = foreign.execute_typed(&foreign_owner, "make", (0,)).unwrap();
    let foreign_value = ConversionContext::new(foreign.runtime(), &foreign_owner)
        .unwrap()
        .encode_value(child.native_type().type_argument(), child.clone())
        .unwrap();
    assert!(native.create(&mut cx, Edge(foreign_value)).is_err());
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 2);
    drop(parent);
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
}

#[test]
fn native_handles_pin_old_scopes_and_transfer_with_their_runtime() {
    let dropped = Arc::new(AtomicUsize::new(0));
    let (module, declaration, _, _) = module(dropped.clone());
    let (vm, old) = fixture(SOURCE, Some(&module));
    let native = vm
        .runtime()
        .bind_native_type::<Counter>(&old, &declaration, &[])
        .unwrap();
    let object = native
        .create(
            &mut vm.context(&old).unwrap(),
            Counter {
                value: 42,
                dropped: dropped.clone(),
            },
        )
        .unwrap();
    let new = vm
        .reload_program(&old, "functions", compile_program(SOURCE, Some(&module)))
        .unwrap();
    assert!(
        !vm.runtime()
            .collect_garbage()
            .unwrap()
            .reclaimed_modules
            .contains(&old.key())
    );
    thread::spawn(move || {
        assert_eq!(
            vm.execute_typed::<_, i32>(&new, "value", (object.clone(),))
                .unwrap(),
            42
        );
        drop(object);
        let created = native
            .clone()
            .create(
                &mut vm.context(&new).unwrap(),
                Counter { value: 43, dropped },
            )
            .unwrap();
        assert_eq!(
            created
                .read(&vm.context(&new).unwrap(), |state| Ok(state.value))
                .unwrap(),
            43
        );
        drop((created, native));
        let collected = vm.runtime().collect_garbage().unwrap();
        assert_eq!(collected.live_objects, 0);
        assert!(collected.reclaimed_modules.contains(&old.key()));
    })
    .join()
    .unwrap();
}

#[test]
fn applied_native_handles_preserve_the_supplied_nominal_layout_scope() {
    let (module, _, _, edge) = module(Arc::new(AtomicUsize::new(0)));
    let source = r#"
        use example::payload::Edge;
        pub struct Item { pub val value: i32 }
        pub fn item() -> Item { Item { value: 42 } }
        pub fn accept(value: Edge<Item>) -> i32 { 42 }
    "#;
    let (mut vm, old) = fixture(source, Some(&module));
    let old_item = vm.runtime().bind_type(&old, "Item", &[]).unwrap();
    let new = vm
        .runtime_mut()
        .load_program(
            "independent",
            compile_program(&source.replace("value: i32", "value: i64"), Some(&module)),
        )
        .unwrap();
    let runtime = vm.runtime();
    let new_item = runtime.bind_type(&new, "Item", &[]).unwrap();
    assert_eq!(old_item.type_argument().ty(), new_item.type_argument().ty());
    let old_type = runtime
        .bind_native_type::<Edge>(&old, &edge, slice::from_ref(old_item.type_argument()))
        .unwrap();
    // The caller's selected owner must not flatten an argument supplied by a
    // different version. Opaque generic storage may carry either checked scope.
    let new_type = runtime
        .bind_native_type::<Edge>(&old, &edge, slice::from_ref(new_item.type_argument()))
        .unwrap();
    let mut cx = vm.context(&old).unwrap();
    let old_value = old_type.create(&mut cx, Edge(Value::Unit)).unwrap();
    let new_value = new_type.create(&mut cx, Edge(Value::Unit)).unwrap();
    let old_call = runtime
        .bind_function::<(NativeObject<Edge>,), i32>(&old, "accept")
        .unwrap();
    let new_call = runtime
        .bind_function::<(NativeObject<Edge>,), i32>(&new, "accept")
        .unwrap();
    assert_eq!(vm.call(&old_call, (old_value.clone(),)).unwrap(), 42);
    assert!(vm.call(&new_call, (old_value,)).is_err());
    assert!(vm.call(&old_call, (new_value.clone(),)).is_err());
    assert_eq!(vm.call(&new_call, (new_value,)).unwrap(), 42);
    assert_eq!(runtime.collect_garbage().unwrap().live_objects, 0);
}
