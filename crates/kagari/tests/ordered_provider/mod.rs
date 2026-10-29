mod allocation;
mod provider;
mod registration;

use kagari_compiler::{bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir};
use kagari_hir::analysis::AnalysisDatabase;
use kagari_runtime::{
    Runtime, RuntimeConfig,
    error::{RuntimeError, RuntimeErrorKind},
    gc::GcHeapConfig,
    module::Program,
    native::{
        conversion::context::ConversionContext, function_handle::Function, module::NativeModule,
        payload::Opaque, storage::access::EditPlan, storage_type::StorageType,
        typed::NativeContext, types::TypeRef, value_handle::ScriptValue,
        views::NativeOperationKind,
    },
    session::ExecutionOptions,
    value::Value,
};
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use kagari_stdlib::collections::vector::ScriptVec;
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
pub fn pipeline(map: OrderedMap<String>) -> Iterator<Item = (u64, String)> {
    map.map(|entry| entry[1]).filter(|text| text != "skip").take(3).enumerate()
}
pub fn advance(cursor: Iterator<Item = (u64, String)>) -> Option<(u64, String)> {
    cursor.next()
}
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

// Allocate without a conversion safepoint so a test can publish between two
// observed scanner slices. The public conversion trait still validates and roots
// the new value; no raw value survives an intervening collection boundary.
fn retained_text(cx: &NativeContext<'_>, text: &str) -> ScriptValue {
    let mut conversion = ConversionContext::new(cx.runtime(), cx.owner()).unwrap();
    let argument = conversion.type_for::<String>().unwrap();
    let value = unsafe { cx.runtime().gc().alloc_string_transport(text.into()) }.unwrap();
    let root = unsafe { cx.runtime().gc().root_value_transport(value) }.unwrap();
    conversion.retain(&argument, root).unwrap()
}

#[track_caller]
fn automatic_until(runtime: &Runtime, mut reached: impl FnMut() -> bool) {
    for _ in 0..512 {
        if reached() {
            return;
        }
        runtime.gc_safepoint().unwrap();
    }
    assert!(
        reached(),
        "automatic collection did not reach the observed boundary"
    );
}

// Cross the adaptive start threshold without servicing it yet. The returned
// unrooted identity is only used for checked rejection, never dereferenced.
fn collection_pressure(runtime: &Runtime) -> Value {
    const TEXT: &str = "discarded pressure";
    // String pressure includes its bytes, not just its object header. Keep the
    // workload finite while leaving enough doomed objects to observe mid-sweep.
    let count = runtime
        .gc()
        .stats()
        .current_heap_units
        .div_ceil(TEXT.len() + 1)
        + 2048;
    let mut last = Value::Unit;
    for _ in 0..count {
        last = unsafe { runtime.gc().alloc_string_transport(TEXT.into()) }.unwrap();
    }
    last
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
    let cursor: Opaque<Cursor> = vm.execute_typed(&owner, "cursor", (map.clone(),)).unwrap();
    let contract = StorageType::prepare_scoped(map.type_argument().clone(), map.owner()).unwrap();
    let source = {
        let mut conversion = cx.conversion_scope(map.owner()).unwrap();
        conversion
            .encode_value(map.type_argument(), map.clone())
            .unwrap()
    };
    runtime.collect_garbage().unwrap();
    let mut nested = vm.context(&owner).unwrap();
    // SAFETY: The existing map root protects source across this explicit
    // collection and every immediate read. Only scalars leave these test loans.
    unsafe {
        cx.read_payload::<OrderedMap, _>(
            source.value_transport(runtime).unwrap(),
            &contract,
            |_, _| {
                assert!(runtime.collect_garbage().is_err());
                assert!(vm.execute_typed::<_, i32>(&owner, "number", ()).is_err());
                assert!(provider::next(&mut nested, (cursor.clone(),)).is_err());
                assert!(provider::clear(&mut nested, (map.clone(),)).is_err());
                Ok(())
            },
        )
        .unwrap();
        assert!(
            cx.read_payload::<Cursor, ()>(
                source.value_transport(runtime).unwrap(),
                &contract,
                |_, _| { panic!("wrong payload mapping must reject before entering the reader") }
            )
            .is_err()
        );
    }
    drop(source);
    runtime.collect_garbage().unwrap();
    let (key, first) = provider::next(&mut cx, (cursor.clone(),)).unwrap().unwrap();
    assert_eq!(key, 7, "rejected reads must not advance the cursor");
    assert_eq!(first.decode::<String>(&mut cx).unwrap(), "seven");
    drop(first);
    // SAFETY: No payload mutation or recovery occurs; the guard ends outside loans.
    unsafe {
        let guard = map
            .try_enter_operation(&cx, NativeOperationKind::ExclusiveSlots, None)
            .unwrap()
            .unwrap();
        assert!(provider::next(&mut cx, (cursor.clone(),)).is_err());
        drop(guard);
    }
    let error = provider::next(&mut cx, (cursor,)).unwrap_err();
    assert!(error.to_string().contains("structure changed"), "{error}");
    let cursor: Opaque<Cursor> = vm.execute_typed(&owner, "cursor", (map.clone(),)).unwrap();
    let (key, first) = provider::next(&mut cx, (cursor.clone(),)).unwrap().unwrap();
    assert_eq!(key, 7);
    drop(first);
    let (key, cursor_value) = provider::next(&mut cx, (cursor.clone(),)).unwrap().unwrap();
    assert_eq!(key, 8);
    let retained = provider::remove(&mut cx, (map.clone(), 8))
        .unwrap()
        .unwrap();
    drop(cursor);
    drop(map);
    runtime.collect_garbage().unwrap();
    assert_eq!(cursor_value.decode::<String>(&mut cx).unwrap(), "eight");
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
    let filler = retained_text(&cx, "callback scan tail");
    for key in 100..4196 {
        provider::insert(&mut cx, (map.clone(), key, filler.clone())).unwrap();
    }
    drop(filler);
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

    for failure in ["error", "cancel", "unwind"] {
        runtime.collect_garbage().unwrap();
        collection_pressure(runtime);
        observations.scan_position.store(0, Ordering::SeqCst);
        let options = ExecutionOptions::default();
        let cancellation = options.cancellation.clone();
        let session = runtime.begin_execution(&owner, options).unwrap();
        let mut callback_error = Some(RuntimeError::module_validation("external callback fails"));
        let before = runtime.gc().stats();
        let before_shape = provider::shape(&cx, &map).unwrap();
        let revision = provider::revision(&cx, &map).unwrap();
        let result = catch_unwind(AssertUnwindSafe(|| {
            provider::with_scratch::<()>(&mut cx, &map, |cx| {
                automatic_until(runtime, || {
                    let position = observations.scan_position.load(Ordering::SeqCst);
                    position > 0 && position < original.0
                });
                assert_eq!(runtime.gc().stats().collections, before.collections);
                assert!(provider::get(cx, (map.clone(), 7)).is_err());
                assert!(provider::clear(cx, (map.clone(),)).is_err());
                if failure == "unwind" {
                    let _probe = allocation::BeginOnUnwind;
                    panic!("external provider callback unwinds")
                }
                let error = if failure == "cancel" {
                    cancellation.cancel();
                    cx.poll().unwrap_err()
                } else {
                    callback_error.take().unwrap()
                };
                allocation::begin_cleanup_probe();
                Err(error)
            })
        }));
        allocation::end_cleanup_probe();
        drop(session);
        if failure == "unwind" {
            assert!(result.is_err());
        } else {
            let error = result.unwrap().unwrap_err();
            if failure == "cancel" {
                assert_eq!(error.kind(), RuntimeErrorKind::Cancelled);
            } else {
                assert!(error.to_string().contains("external callback fails"));
            }
        }
        let after_shape = provider::shape(&cx, &map)
            .unwrap_or_else(|error| panic!("post-{failure} receiver: {error:?}"));
        let after = runtime.gc().stats();
        assert_eq!(after_shape.0, original.0);
        assert_eq!(after_shape.1.logical_units, original.1.logical_units);
        assert_eq!(after.current_heap_units, before.current_heap_units);
        assert_eq!(
            after.current_retained_bytes - before.current_retained_bytes,
            (after_shape.1.retained_bytes - before_shape.1.retained_bytes) as u128
        );
        assert_eq!(provider::revision(&cx, &map).unwrap(), revision + 1);
        automatic_until(runtime, || {
            runtime.gc().stats().collections > before.collections
        });
        assert_eq!(
            provider::get(&mut cx, (map.clone(), 7))
                .unwrap()
                .unwrap()
                .decode::<String>(&mut cx)
                .unwrap(),
            "seven"
        );
    }
    assert_eq!(observations.recoveries.load(Ordering::SeqCst), 5);
    let guard = provider::scratch_operation(&mut cx, &map).unwrap();
    cx.collect_garbage().unwrap();
    allocation::assert_no_allocations(|| drop(guard));
    assert_eq!(observations.recoveries.load(Ordering::SeqCst), 6);
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
    for key in 0..4096_i32 {
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
        "installed erased scanner must resume across 4096 slots, including holes"
    );
    for key in [0, 255, 256, 511, 4095] {
        assert_eq!(
            provider::get(&mut cx, (map.clone(), key))
                .unwrap()
                .unwrap()
                .decode::<String>(&mut cx)
                .unwrap(),
            format!("value-{key}")
        );
    }
    // Unlike the synchronous assertion above, these observations straddle real
    // automatic service returns. A single 1024-unit invocation cannot scan this
    // provider, and the mutator edits both an already inspected slot and the tail.
    let completed = runtime.gc().stats().collections;
    let scans = observations.completed_scans.load(Ordering::SeqCst);
    observations.scan_position.store(0, Ordering::SeqCst);
    let condemned = collection_pressure(runtime);
    automatic_until(runtime, || {
        let position = observations.scan_position.load(Ordering::SeqCst);
        position > 0 && position < 4096
    });
    assert_eq!(runtime.gc().stats().collections, completed);
    assert_eq!(observations.completed_scans.load(Ordering::SeqCst), scans);
    let later = provider::get(&mut cx, (map.clone(), 4095))
        .unwrap()
        .unwrap();
    provider::insert(&mut cx, (map.clone(), 7, later)).unwrap();
    let appended = retained_text(&cx, "partial-append");
    provider::insert(&mut cx, (map.clone(), 4096, appended)).unwrap();
    automatic_until(runtime, || {
        observations.completed_scans.load(Ordering::SeqCst) > scans
    });
    assert_eq!(runtime.gc().stats().collections, completed);
    assert_eq!(observations.scan_limit.load(Ordering::SeqCst), 4096);
    // The provider is fully scanned, but the same cycle has not completed. Both
    // replacement and a new slot must publish without restarting its old cursor.
    let replacement = retained_text(&cx, "black-replacement");
    provider::insert(&mut cx, (map.clone(), 0, replacement)).unwrap();
    let appended = retained_text(&cx, "black-append");
    provider::insert(&mut cx, (map.clone(), 4097, appended)).unwrap();
    let reclaimed = runtime.gc().stats().reclaimed_objects;
    automatic_until(runtime, || {
        runtime.gc().stats().reclaimed_objects > reclaimed
    });
    assert_eq!(runtime.gc().stats().collections, completed);
    assert!(
        unsafe { runtime.gc().root_value_transport(condemned) }.is_none(),
        "sweep cannot resurrect a condemned identity"
    );
    let newborn = retained_text(&cx, "sweep-newborn");
    provider::insert(&mut cx, (map.clone(), 4098, newborn)).unwrap();
    automatic_until(runtime, || runtime.gc().stats().collections > completed);
    for (key, expected) in [
        (7, "value-4095"),
        (4096, "partial-append"),
        (0, "black-replacement"),
        (4097, "black-append"),
        (4098, "sweep-newborn"),
    ] {
        assert_eq!(
            provider::get(&mut cx, (map.clone(), key))
                .unwrap()
                .unwrap()
                .decode::<String>(&mut cx)
                .unwrap(),
            expected
        );
    }
    assert_eq!(
        vm.execute_typed::<_, u64>(&owner, "first_two", (map.clone(),))
            .unwrap(),
        2
    );
    let cursor: Opaque<Cursor> = vm.execute_typed(&owner, "cursor", (map.clone(),)).unwrap();
    let pipeline: ScriptValue = vm
        .execute_typed(&owner, "pipeline", (map.clone(),))
        .unwrap();
    let completed = runtime.gc().stats().collections;
    observations.scan_position.store(0, Ordering::SeqCst);
    collection_pressure(runtime);
    automatic_until(runtime, || {
        let position = observations.scan_position.load(Ordering::SeqCst);
        position > 0 && position < 4096
    });
    assert_eq!(runtime.gc().stats().collections, completed);
    // Typed script calls, custom next and nested adapter callbacks now continue
    // the active automatic cycle while both retained cursors remain usable.
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
    assert_eq!(
        vm.execute_typed::<_, Option<(u64, String)>>(&owner, "advance", (pipeline.clone(),))
            .unwrap(),
        Some((0, "replacement".into()))
    );
    automatic_until(runtime, || runtime.gc().stats().collections > completed);
    vm.execute_typed::<_, ()>(&owner, "put", (map.clone(), 4099_i32, "new".to_owned()))
        .unwrap();
    let error = provider::next(&mut cx, (cursor.clone(),)).unwrap_err();
    assert!(error.to_string().contains("structure changed"), "{error}");
    let error = vm
        .execute_typed::<_, Option<(u64, String)>>(&owner, "advance", (pipeline.clone(),))
        .unwrap_err();
    assert!(
        format!("{error:?}").contains("structure changed"),
        "{error:?}"
    );
    // Removal and clear must invalidate the same retained custom-next chain,
    // not just direct calls to the provider cursor.
    for clear in [false, true] {
        let nested: ScriptValue = vm
            .execute_typed(&owner, "pipeline", (map.clone(),))
            .unwrap();
        if clear {
            provider::clear(&mut cx, (map.clone(),)).unwrap();
        } else {
            drop(provider::remove(&mut cx, (map.clone(), 1)).unwrap());
        }
        let error = vm
            .execute_typed::<_, Option<(u64, String)>>(&owner, "advance", (nested,))
            .unwrap_err();
        assert!(
            format!("{error:?}").contains("structure changed"),
            "{error:?}"
        );
    }
    drop(map);
    runtime.collect_garbage().unwrap();
    assert_eq!(
        observations.drops.load(Ordering::SeqCst),
        0,
        "cursor traces its source without a payload-owned root"
    );
    drop(cursor);
    runtime.collect_garbage().unwrap();
    assert_eq!(
        observations.drops.load(Ordering::SeqCst),
        0,
        "nested adapters retain their custom source"
    );
    drop(pipeline);
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
    let retained = provider::get(&mut cx, (map.clone(), 1)).unwrap().unwrap();
    for key in 2..4097 {
        provider::insert(&mut cx, (map.clone(), key, retained.clone())).unwrap();
    }
    runtime.collect_garbage().unwrap();
    let completed = runtime.gc().stats().collections;
    observations.scan_position.store(0, Ordering::SeqCst);
    collection_pressure(runtime);
    automatic_until(runtime, || {
        let position = observations.scan_position.load(Ordering::SeqCst);
        position > 0 && position < 4096
    });
    assert_eq!(runtime.gc().stats().collections, completed);
    drop(map);
    automatic_until(runtime, || runtime.gc().stats().collections > completed);
    let callback = retained.decode::<Function<(), i32>>(&mut cx).unwrap();
    assert_eq!(callback.call(&mut cx, ()).unwrap(), 42);
    assert_eq!(observations.drops.load(Ordering::SeqCst), 0);
    drop(callback);
    drop(retained);
    let completed = runtime.gc().stats().collections;
    collection_pressure(runtime);
    automatic_until(runtime, || runtime.gc().stats().collections > completed);
    assert_eq!(
        observations.drops.load(Ordering::SeqCst),
        1,
        "an unreachable map/closure cycle must not contain hidden permanent roots"
    );
}

#[test]
fn standard_vector_error_and_unwind_recovery_allocate_nothing() {
    let (vm, program, _, _) = fixture();
    let runtime = vm.runtime();
    let owner = runtime.select_program(&program).unwrap();
    let mut cx = vm.context(&owner).unwrap();
    let original = ["delta", "beta", "alpha", "gamma"];
    for unwind in [false, true] {
        let vector = ScriptVec::create(&mut cx, original.map(str::to_owned).to_vec()).unwrap();
        let mut error = Some(RuntimeError::module_validation("comparator failure"));
        let mut comparisons = 0;
        let result = catch_unwind(AssertUnwindSafe(|| {
            vector.sort_by(&mut cx, |cx, a, b| {
                cx.collect_garbage()?;
                comparisons += 1;
                if comparisons != 2 {
                    return Ok(a.cmp(&b));
                }
                if unwind {
                    let _probe = allocation::BeginOnUnwind;
                    panic!("standard comparator unwind");
                }
                allocation::begin_cleanup_probe();
                Err(error.take().unwrap())
            })
        }));
        allocation::end_cleanup_probe();
        assert_eq!(comparisons, 2);
        if unwind {
            assert!(result.is_err());
        } else {
            assert!(result.unwrap().is_err());
        }
        runtime.collect_garbage().unwrap();
        let mut actual = Vec::new();
        vector
            .for_each(&mut cx, |_, value| {
                actual.push(value);
                Ok(())
            })
            .unwrap();
        actual.sort();
        let mut expected = original.map(str::to_owned);
        expected.sort();
        assert_eq!(actual, expected);
        drop(vector);
        assert_eq!(runtime.collect_garbage().unwrap().live_objects, 0);
    }
}
