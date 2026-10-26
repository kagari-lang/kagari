mod allocation;
mod provider;
mod registration;

use kagari_compiler::{bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir};
use kagari_hir::analysis::AnalysisDatabase;
use kagari_runtime::{
    Runtime, RuntimeConfig,
    error::RuntimeError,
    gc::GcHeapConfig,
    module::Program,
    native::{
        conversion::context::ConversionContext, function_handle::Function, module::NativeModule,
        payload::Opaque, storage::access::EditPlan, types::TypeRef, value_handle::ScriptValue,
        views::NativeOperationKind,
    },
};
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use kagari_vm::vm::Vm;
use provider::{Cursor, Observations, OrderedMap};
use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{Arc, atomic::Ordering},
};

const SOURCE: &str = r#"
use external::ordered::{OrderedMap, Cursor, collect};
use std::collections::{Map, MutableMap};

pub fn make() -> OrderedMap<String> {
    val map: OrderedMap<String> = OrderedMap::new();
    map.insert(7, "seven");
    map
}
pub fn put(map: OrderedMap<String>, key: i32, value: String) {
    val view: MutableMap<i32, String> = map;
    view.insert(key, value);
}
pub fn lookup(map: OrderedMap<String>, key: i32) -> Option<String> {
    val view: Map<i32, String> = map;
    view.get(key)
}
pub fn take(map: OrderedMap<String>, key: i32) -> Option<String> {
    val view: MutableMap<i32, String> = map;
    view.remove(key)
}
pub fn text() -> String { "host-created" }
pub fn number() -> i32 { 42 }
pub fn aggregate() -> OrderedMap<(String, Option<String>)> {
    val map: OrderedMap<(String, Option<String>)> = OrderedMap::new();
    map.insert(1, ("nested", Some("enum")));
    map
}
pub fn good_callback(map: OrderedMap<String>) {
    map.with_scratch(|| { collect(); });
}
pub fn bad_callback(map: OrderedMap<String>) {
    map.with_scratch(|| { collect(); map.insert(99, "forbidden"); });
}
pub fn cursor(map: OrderedMap<String>) -> Cursor<String> { map.iter() }
pub fn first_two(map: OrderedMap<String>) -> u64 { map.take(2).count() }
pub fn cycle() -> OrderedMap<fn() -> i32> {
    val map: OrderedMap<fn() -> i32> = OrderedMap::new();
    map.insert(1, || { map.len(); 42 });
    map
}
"#;

fn fixture() -> (Vm, Program, TypeRef, Arc<Observations>) {
    let observations = Arc::new(Observations::default());
    let (module, map) = registration::module(observations.clone()).unwrap();
    let mut modules = kagari_stdlib::modules().unwrap();
    modules.push(module);
    let mut analysis = AnalysisDatabase::default();
    analysis.set_native_modules(
        modules
            .iter()
            .map(|m| Arc::new(m.to_declaration()))
            .collect(),
    );
    let mut sources = SourceDatabase::default();
    let file = sources
        .set("provider.kgr", SOURCE.into(), SourceLayer::Base)
        .unwrap();
    let snapshot = analysis
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let checked = snapshot.check_program(file, &Default::default()).unwrap();
    let bytecode = lower_program_to_bytecode(
        &lower_program_to_mir(&checked, &Default::default()).unwrap(),
        &Default::default(),
    )
    .unwrap()
    .into_unverified();
    let mut runtime = Runtime::new(RuntimeConfig {
        gc: GcHeapConfig {
            collection_threshold: Some(1),
            ..Default::default()
        },
        ..Default::default()
    });
    NativeModule::install_all(&modules, &mut runtime).unwrap();
    let program = runtime.load_program("provider", bytecode).unwrap();
    (Vm::new(runtime), program, map, observations)
}

#[test]
fn aggregate_validation_remains_available_inside_storage_loans() {
    let (vm, program, _, _) = fixture();
    let runtime = vm.runtime();
    let owner = runtime.select_program(&program).unwrap();
    let mut cx = vm.context(&owner).unwrap();
    let map: Opaque<OrderedMap> = vm.execute_typed(&owner, "aggregate", ()).unwrap();
    let retained = provider::remove(&mut cx, (map.clone(), 1))
        .unwrap()
        .unwrap();
    runtime.collect_garbage().unwrap();
    provider::insert(&mut cx, (map.clone(), 2, retained)).unwrap();
    let retained = provider::get(&mut cx, (map.clone(), 2)).unwrap().unwrap();
    drop(map);
    runtime.collect_garbage().unwrap();
    assert_eq!(
        retained
            .decode::<(String, Option<String>)>(&mut cx)
            .unwrap(),
        ("nested".into(), Some("enum".into()))
    );
}

#[test]
fn public_constructor_host_creation_and_retained_extraction() {
    let (vm, program, declaration, observations) = fixture();
    let runtime = vm.runtime();
    let owner = runtime.select_program(&program).unwrap();
    let mut cx = vm.context(&owner).unwrap();
    let map: Opaque<OrderedMap> = vm.execute_typed(&owner, "make", ()).unwrap();
    vm.execute_typed::<_, ()>(&owner, "put", (map.clone(), 8_i32, "eight".to_owned()))
        .unwrap();
    assert_eq!(
        vm.execute_typed::<_, Option<String>>(&owner, "lookup", (map.clone(), 8_i32))
            .unwrap(),
        Some("eight".into())
    );
    let retained = provider::remove(&mut cx, (map.clone(), 8))
        .unwrap()
        .unwrap();
    drop(map);
    runtime.collect_garbage().unwrap();
    assert_eq!(retained.decode::<String>(&mut cx).unwrap(), "eight");
    assert_eq!(observations.drops.load(Ordering::SeqCst), 1);

    let conversion = ConversionContext::new(runtime, &owner).unwrap();
    let argument = conversion.type_for::<String>().unwrap();
    let ty = runtime
        .bind_native_type::<OrderedMap>(&program.root(), &declaration, &[argument])
        .unwrap();
    let payload = OrderedMap::empty(&cx, &ty, observations.clone()).unwrap();
    let map = ty.create(&mut cx, payload, &[]).unwrap();
    let text: ScriptValue = vm.execute_typed(&owner, "text", ()).unwrap();
    provider::insert(&mut cx, (map.clone(), 9, text)).unwrap();
    runtime.collect_garbage().unwrap();
    assert_eq!(
        vm.execute_typed::<_, Option<String>>(&owner, "take", (map.clone(), 9_i32))
            .unwrap(),
        Some("host-created".into())
    );
    assert_eq!(provider::len(&mut cx, (map,)).unwrap(), 0);
}

#[test]
fn publication_checks_and_committed_edits_survive_errors_and_unwinds() {
    let (vm, program, _, _) = fixture();
    let runtime = vm.runtime();
    let owner = runtime.select_program(&program).unwrap();
    let mut cx = vm.context(&owner).unwrap();
    let map: Opaque<OrderedMap> = vm.execute_typed(&owner, "make", ()).unwrap();
    let before = provider::shape(&cx, &map).unwrap();
    let revision = provider::revision(&cx, &map).unwrap();
    let wrong_type: ScriptValue = vm.execute_typed(&owner, "number", ()).unwrap();
    assert!(provider::insert(&mut cx, (map.clone(), 1, wrong_type)).is_err());
    let (foreign_vm, foreign_program, _, _) = fixture();
    let foreign_owner = foreign_vm
        .runtime()
        .select_program(&foreign_program)
        .unwrap();
    let foreign_value: ScriptValue = foreign_vm
        .execute_typed(&foreign_owner, "text", ())
        .unwrap();
    assert!(provider::insert(&mut cx, (map.clone(), 1, foreign_value)).is_err());
    assert_eq!(provider::shape(&cx, &map).unwrap(), before);
    assert_eq!(provider::revision(&cx, &map).unwrap(), revision);

    for (key, unwind) in [(10, false), (11, true)] {
        let text: ScriptValue = vm.execute_typed(&owner, "text", ()).unwrap();
        runtime.collect_garbage().unwrap();
        let before = runtime.gc().stats();
        let shape = provider::shape(&cx, &map).unwrap();
        let revision = provider::revision(&cx, &map).unwrap();
        let result = catch_unwind(AssertUnwindSafe(|| {
            provider::insert_with_completion(&mut cx, (map.clone(), key, text), || {
                assert!(
                    runtime.collect_garbage().is_err(),
                    "short loan must forbid runtime reentry"
                );
                if unwind {
                    panic!("external provider completed edit unwinds")
                }
                Err(RuntimeError::module_validation("completed insertion fails"))
            })
        }));
        if unwind {
            assert!(result.is_err());
        } else {
            assert!(result.unwrap().is_err());
        }
        let after = runtime.gc().stats();
        let after_shape = provider::shape(&cx, &map).unwrap();
        assert_eq!(after.current_heap_units, before.current_heap_units + 2);
        assert_eq!(
            after.current_retained_bytes - before.current_retained_bytes,
            (after_shape.1.retained_bytes - shape.1.retained_bytes) as u128
        );
        assert_eq!(provider::revision(&cx, &map).unwrap(), revision + 1);
        runtime.collect_garbage().unwrap();
        assert_eq!(
            provider::get(&mut cx, (map.clone(), key))
                .unwrap()
                .unwrap()
                .decode::<String>(&mut cx)
                .unwrap(),
            "host-created"
        );
    }
}

#[test]
fn callbacks_alias_protection_and_allocation_free_scratch_recovery() {
    let (vm, program, _, observations) = fixture();
    let runtime = vm.runtime();
    let owner = runtime.select_program(&program).unwrap();
    let mut cx = vm.context(&owner).unwrap();
    let map: Opaque<OrderedMap> = vm.execute_typed(&owner, "make", ()).unwrap();
    // A host-allocated string avoids accidental retention by executable constants.
    vm.execute_typed::<_, ()>(
        &owner,
        "put",
        (map.clone(), -1_i32, "scratch-only-edge".to_owned()),
    )
    .unwrap();
    let original = provider::shape(&cx, &map).unwrap();
    let initial_revision = provider::revision(&cx, &map).unwrap();
    vm.execute_typed::<_, ()>(&owner, "good_callback", (map.clone(),))
        .unwrap();
    let error = vm
        .execute_typed::<_, ()>(&owner, "bad_callback", (map.clone(),))
        .unwrap_err();
    assert!(
        format!("{error:?}").contains("operation protects receiver slots"),
        "{error:?}"
    );
    assert!(!provider::contains(&mut cx, (map.clone(), 99)).unwrap());
    assert_eq!(provider::revision(&cx, &map).unwrap(), initial_revision + 2);

    for unwind in [false, true] {
        runtime.collect_garbage().unwrap();
        let before = runtime.gc().stats();
        let before_shape = provider::shape(&cx, &map).unwrap();
        let revision = provider::revision(&cx, &map).unwrap();
        let result = catch_unwind(AssertUnwindSafe(|| {
            provider::with_scratch::<()>(&mut cx, &map, |cx| {
                cx.collect_garbage()?;
                assert!(provider::get(cx, (map.clone(), 7)).is_err());
                assert!(provider::clear(cx, (map.clone(),)).is_err());
                if unwind {
                    panic!("external provider callback unwinds")
                }
                Err(RuntimeError::module_validation("external callback fails"))
            })
        }));
        if unwind {
            assert!(result.is_err());
        } else {
            assert!(result.unwrap().is_err());
        }
        let after_shape = provider::shape(&cx, &map).unwrap();
        let after = runtime.gc().stats();
        assert_eq!(after_shape.0, original.0);
        assert_eq!(after_shape.1.logical_units, original.1.logical_units);
        assert_eq!(after.current_heap_units, before.current_heap_units);
        assert_eq!(
            after.current_retained_bytes - before.current_retained_bytes,
            (after_shape.1.retained_bytes - before_shape.1.retained_bytes) as u128
        );
        assert_eq!(provider::revision(&cx, &map).unwrap(), revision + 1);
        assert_eq!(
            provider::get(&mut cx, (map.clone(), 7))
                .unwrap()
                .unwrap()
                .decode::<String>(&mut cx)
                .unwrap(),
            "seven"
        );
    }
    assert_eq!(observations.recoveries.load(Ordering::SeqCst), 4);
    let guard = provider::scratch_operation(&mut cx, &map).unwrap();
    cx.collect_garbage().unwrap();
    allocation::assert_no_allocations(|| drop(guard));
    assert_eq!(observations.recoveries.load(Ordering::SeqCst), 5);
    assert_eq!(provider::shape(&cx, &map).unwrap().0, original.0);
    assert_eq!(
        provider::get(&mut cx, (map.clone(), -1))
            .unwrap()
            .unwrap()
            .decode::<String>(&mut cx)
            .unwrap(),
        "scratch-only-edge"
    );

    // SAFETY: Read-only guards carry no recovery; all loans end before guard drop.
    unsafe {
        let guard = map
            .try_enter_operation(&cx, NativeOperationKind::ReadOnlyCallbacks, None)
            .unwrap()
            .unwrap();
        let nested = map
            .try_enter_operation(&cx, NativeOperationKind::ReadOnlyCallbacks, None)
            .unwrap()
            .unwrap();
        assert!(provider::get(&mut cx, (map.clone(), 7)).unwrap().is_some());
        assert!(
            map.edit(
                &mut cx,
                Some(&guard),
                EditPlan {
                    max_logical_units: original.1.logical_units,
                    structural: false
                },
                |_, _| Ok(())
            )
            .is_err()
        );
        assert!(provider::clear(&mut cx, (map.clone(),)).is_err());
        cx.collect_garbage().unwrap();
        drop(nested);
        drop(guard);
    }
    provider::clear(&mut cx, (map,)).unwrap();
}

#[test]
fn indexed_scanner_continuation_and_existing_iteration_adapters() {
    let (vm, program, _, observations) = fixture();
    let runtime = vm.runtime();
    let owner = runtime.select_program(&program).unwrap();
    let mut cx = vm.context(&owner).unwrap();
    let map: Opaque<OrderedMap> = vm.execute_typed(&owner, "make", ()).unwrap();
    for key in 0..600_i32 {
        vm.execute_typed::<_, ()>(&owner, "put", (map.clone(), key, format!("value-{key}")))
            .unwrap();
    }
    for key in 100..200_i32 {
        drop(provider::remove(&mut cx, (map.clone(), key)).unwrap());
    }
    observations.pending.store(0, Ordering::SeqCst);
    runtime.collect_garbage().unwrap();
    assert!(
        observations.pending.load(Ordering::SeqCst) >= 2,
        "installed erased scanner must resume across 600 slots, including holes"
    );
    for key in [0, 255, 256, 511, 599] {
        assert_eq!(
            provider::get(&mut cx, (map.clone(), key))
                .unwrap()
                .unwrap()
                .decode::<String>(&mut cx)
                .unwrap(),
            format!("value-{key}")
        );
    }
    assert_eq!(
        vm.execute_typed::<_, u64>(&owner, "first_two", (map.clone(),))
            .unwrap(),
        2
    );
    let cursor: Opaque<Cursor> = vm.execute_typed(&owner, "cursor", (map.clone(),)).unwrap();
    let revision = provider::revision(&cx, &map).unwrap();
    vm.execute_typed::<_, ()>(
        &owner,
        "put",
        (map.clone(), 0_i32, "replacement".to_owned()),
    )
    .unwrap();
    assert_eq!(provider::revision(&cx, &map).unwrap(), revision);
    let (key, value) = provider::next(&mut cx, (cursor.clone(),)).unwrap().unwrap();
    assert_eq!(
        (key, value.decode::<String>(&mut cx).unwrap()),
        (0, "replacement".into())
    );
    drop(value);
    vm.execute_typed::<_, ()>(&owner, "put", (map.clone(), 601_i32, "new".to_owned()))
        .unwrap();
    let error = provider::next(&mut cx, (cursor.clone(),)).unwrap_err();
    assert!(error.to_string().contains("structure changed"), "{error}");
    drop(map);
    runtime.collect_garbage().unwrap();
    assert_eq!(
        observations.drops.load(Ordering::SeqCst),
        0,
        "cursor traces its source without a payload-owned root"
    );
    drop(cursor);
    runtime.collect_garbage().unwrap();
    assert_eq!(observations.drops.load(Ordering::SeqCst), 1);
}

#[test]
fn traced_closure_cycle_and_retained_get_release_without_per_element_roots() {
    let (vm, program, _, observations) = fixture();
    let runtime = vm.runtime();
    let owner = runtime.select_program(&program).unwrap();
    let mut cx = vm.context(&owner).unwrap();
    let map: Opaque<OrderedMap> = vm.execute_typed(&owner, "cycle", ()).unwrap();
    runtime.collect_garbage().unwrap();
    let retained = provider::get(&mut cx, (map.clone(), 1)).unwrap().unwrap();
    drop(map);
    runtime.collect_garbage().unwrap();
    let callback = retained.decode::<Function<(), i32>>(&mut cx).unwrap();
    assert_eq!(callback.call(&mut cx, ()).unwrap(), 42);
    assert_eq!(observations.drops.load(Ordering::SeqCst), 0);
    drop(callback);
    drop(retained);
    runtime.collect_garbage().unwrap();
    assert_eq!(
        observations.drops.load(Ordering::SeqCst),
        1,
        "an unreachable map/closure cycle must not contain hidden permanent roots"
    );
}
