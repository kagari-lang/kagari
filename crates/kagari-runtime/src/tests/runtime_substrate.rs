mod ownership;

#[cfg(test)]
use crate::test_support::bytecode::load_bytecode;
use crate::test_support::native::program::ProgramFixture;
use crate::{
    Runtime,
    error::RuntimeErrorKind,
    execution::roots::ExecutionRoots,
    host::HostObjectId,
    metadata::{AbiFingerprint, TypeKind, TypeRegistration},
    test_support::layouts,
    value::{
        aggregates::{structures::StructObject, tuples::TupleObject},
        arrays::ArrayObject,
        carriers::{RootedValue, ScopedValue},
        host_values::{HostRootObject, paths::HostPathObject},
        scalar,
    },
};
use kagari_bytecode::{module::BytecodeModule, program::ModuleRef};
use kagari_common::identity::map::DefinitionContext;

use kagari_gc::{
    error::GcError,
    heap::Phase,
    object::GcObject,
    scan::{EdgeVisitor, ScanBudget, ScanKind, ScanStatus},
};
use kagari_types::{scalar::BuiltinType, ty::Ty};
use std::{
    slice,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

#[test]
fn explicit_roots_trace_script_objects_without_crossing_host_boundaries() {
    let mut runtime = Runtime::default();
    let record_layout = layouts::layout(
        &mut runtime,
        "Record",
        &[(
            "leaf",
            Ty::Array(Box::new(Ty::Builtin(BuiltinType::I32))),
            true,
        )],
    );
    runtime.collect_garbage().unwrap();
    let baseline = runtime.gc_stats();
    let leaf = runtime
        .alloc_array(
            record_layout.module(),
            Ty::Builtin(BuiltinType::I32),
            &[1i32.into()],
        )
        .unwrap();
    let record = runtime
        .alloc_struct(record_layout, slice::from_ref(&leaf))
        .unwrap();
    let tuple = runtime
        .memory
        .scope(&runtime.resources, |cx, _| {
            let record = record.get(cx)?;
            let tuple = TupleObject::alloc(cx, &[record, record])?;
            ScopedValue::managed(tuple).retain(cx)
        })
        .unwrap();
    drop((leaf, record));
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc_stats().objects, baseline.objects + 3);
    assert_eq!(
        runtime.gc_stats().root_registrations,
        baseline.root_registrations + 1
    );
    let record = runtime
        .memory
        .scope(&runtime.resources, |cx, _| {
            let tuple = cx.downcast::<TupleObject>(tuple.get(cx)?.object().unwrap())?;
            TupleObject::get(cx, tuple, 0)?.retain(cx)
        })
        .unwrap();
    let leaf = runtime.reflect_get_field(&record, "leaf").unwrap();
    let changed = runtime
        .reflect_set_index(&leaf, &0i32.into(), &9i32.into())
        .unwrap();
    let (host, path) = layouts::host_values(&mut runtime, 11);
    assert_eq!(
        runtime
            .reflect_set_field(&record, "leaf", &host)
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::InvalidReflectiveWrite
    );
    assert_eq!(
        runtime
            .reflect_set_index(&leaf, &0i32.into(), &path)
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::InvalidReflectiveWrite
    );
    drop((host, path));
    drop((record, leaf, changed));
    runtime.collect_garbage().unwrap();
    runtime
        .memory
        .scope(&runtime.resources, |cx, _| {
            let tuple = cx.downcast::<TupleObject>(tuple.get(cx)?.object().unwrap())?;
            let left = TupleObject::get(cx, tuple, 0)?.object().unwrap();
            let right = TupleObject::get(cx, tuple, 1)?.object().unwrap();
            assert!(left.ptr_eq(right));
            let record = cx.downcast::<StructObject>(right)?;
            let leaf = StructObject::get(cx, record, 0)?.object().unwrap();
            let leaf = cx.downcast::<ArrayObject>(leaf)?;
            assert_eq!(
                scalar::read_integer(BuiltinType::I32, ArrayObject::get(cx, leaf, 0)?).unwrap(),
                9
            );
            Ok(())
        })
        .unwrap();
    drop(tuple);
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc_stats().objects, baseline.objects);
    assert_eq!(
        runtime.gc_stats().root_registrations,
        baseline.root_registrations
    );
}

#[test]
fn same_name_loads_have_independent_installations_and_runtime_instances() {
    let context = DefinitionContext::new().unwrap();
    let mut runtime = Runtime::default();
    let first_program = load_bytecode(
        &mut runtime,
        "game.player",
        ProgramFixture {
            exports: Default::default(),
            root: ModuleRef::new(0),
            modules: vec![BytecodeModule::default()],
        }
        .metadata(context.snapshot(), &Default::default())
        .unwrap(),
    )
    .unwrap();
    let first = runtime.select_program(&first_program).unwrap();
    let second_program = load_bytecode(
        &mut runtime,
        "game.player",
        ProgramFixture {
            exports: Default::default(),
            root: ModuleRef::new(0),
            modules: vec![BytecodeModule::default()],
        }
        .metadata(context.snapshot(), &Default::default())
        .unwrap(),
    )
    .unwrap();
    let second = runtime.select_program(&second_program).unwrap();

    assert_ne!(first.id, second.id);
    assert_ne!(first.installation_id(), second.installation_id());
    assert_eq!(first.epoch, second.epoch);
    assert_eq!(first.epoch.0, 1);
    assert_eq!(second.epoch.0, 1);

    let first_instance = runtime.module_instance_snapshot(&first).unwrap();
    let second_instance = runtime.module_instance_snapshot(&second).unwrap();
    assert_eq!(first_instance.epoch, second_instance.epoch);
    assert_ne!(first_instance.id, second_instance.id);
    assert!(first_instance.module_slots.is_empty());
    assert!(second_instance.module_slots.is_empty());
}

#[test]
fn runtime_type_registry_facade_preserves_registered_metadata() {
    let runtime = Runtime::default();
    let player_id = runtime
        .types()
        .register(TypeRegistration {
            abi_fingerprint: AbiFingerprint(5),
            ..TypeRegistration::new("Player", TypeKind::Struct)
        })
        .unwrap();

    assert_eq!(runtime.types().id_by_name("Player"), Some(player_id));
    assert_eq!(runtime.types().get(player_id).unwrap().name, "Player");
}

#[test]
fn host_objects_are_not_gc_payloads_or_trace_targets() {
    let mut runtime = Runtime::default();
    let record_layout = layouts::layout(
        &mut runtime,
        "HostBacked",
        &[(
            "path",
            Ty::Array(Box::new(Ty::Builtin(BuiltinType::I32))),
            true,
        )],
    );
    let (host, path) = layouts::host_values(&mut runtime, 7);
    runtime.collect_garbage().unwrap();
    let before = runtime.gc_stats();
    for error in [
        runtime
            .alloc_array(
                record_layout.module(),
                Ty::Builtin(BuiltinType::I32),
                slice::from_ref(&host),
            )
            .unwrap_err(),
        runtime
            .alloc_struct(record_layout, slice::from_ref(&path))
            .unwrap_err(),
    ] {
        assert_eq!(error.kind(), RuntimeErrorKind::ScriptTrap);
    }
    let after = runtime.gc_stats();
    assert_eq!(after.objects, before.objects);
    assert_eq!(after.indirect_bytes, before.indirect_bytes);
    let tuple = runtime
        .memory
        .scope(&runtime.resources, |cx, _| {
            let host = host.get(cx)?;
            let path = path.get(cx)?;
            let tuple = TupleObject::alloc(cx, &[host, path])?;
            assert!(!cx.read(tuple, TupleObject::properties).storable);
            ScopedValue::managed(tuple).retain(cx)
        })
        .unwrap();
    drop((host, path));
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc_stats().objects, before.objects + 1);
    runtime
        .memory
        .scope(&runtime.resources, |cx, _| {
            let tuple = cx.downcast::<TupleObject>(tuple.get(cx)?.object().unwrap())?;
            let host =
                cx.downcast::<HostRootObject>(TupleObject::get(cx, tuple, 0)?.object().unwrap())?;
            let path =
                cx.downcast::<HostPathObject>(TupleObject::get(cx, tuple, 1)?.object().unwrap())?;
            let handle = cx.read(host, HostRootObject::handle);
            assert_eq!(handle.object_id(), HostObjectId(7));
            assert_eq!(cx.read(path, |path| path.facts().root), handle);
            Ok(())
        })
        .unwrap();
    drop(tuple);
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc_stats().objects, before.objects - 2);
    assert_eq!(
        runtime.gc_stats().root_registrations,
        before.root_registrations - 2
    );
}

struct ScanFailure(Arc<AtomicBool>);

// SAFETY: no managed edges. The flag injects a scanner error after valid
// construction; no partial pointer or invalid managed representation is exposed.
unsafe impl GcObject for ScanFailure {
    type ScanState = ();
    const SCAN_KIND: ScanKind = ScanKind::Bounded { max_work: 1 };

    fn begin_scan(&self) {}

    fn scan(
        &self,
        _: &mut (),
        _: &mut ScanBudget,
        _: &mut EdgeVisitor<'_>,
    ) -> Result<ScanStatus, GcError> {
        if self.0.load(Ordering::Relaxed) {
            Err(GcError::Storage("injected Runtime scan failure"))
        } else {
            Ok(ScanStatus::Complete)
        }
    }
}

#[test]
fn collection_failure_quarantines_before_any_owner_is_swept() {
    let mut runtime = Runtime::default();
    let fail = Arc::new(AtomicBool::new(false));
    let execution = runtime.memory.enter_execution(&runtime.resources).unwrap();
    let (root, queued) = runtime
        .memory
        .scope(&runtime.resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                let _dead = slots.scope().alloc_leaf(41u64)?;
                // SAFETY: an edge-free initialized failure-injection provider.
                let failing =
                    unsafe { slots.scope().alloc_with(|_| Ok(ScanFailure(fail.clone()))) }?;
                let root = slots.scope().retain(failing)?;
                let queued = slots.begin_queue(execution, 1)?;
                let value = TupleObject::alloc(slots.scope(), &[])?;
                slots.set_queued(&queued, 0, ScopedValue::managed(value))?;
                Ok((root, queued))
            })
        })
        .unwrap();
    let before = runtime.gc_stats();
    let counters = runtime.resources().counters();
    fail.store(true, Ordering::Relaxed);
    let error = runtime.collect_garbage().unwrap_err();
    assert_eq!(error.kind(), RuntimeErrorKind::EngineFault);
    assert!(runtime.is_quarantined());
    assert_eq!(runtime.gc_stats().phase, Phase::Faulted);
    assert!(matches!(
        runtime
            .memory
            .retire_execution(&runtime.resources, execution),
        Err(GcError::Faulted)
    ));
    assert!(runtime.memory.execution_space(execution).is_ok());
    assert!(matches!(
        runtime.memory.scope(&runtime.resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| queued.check_bounds(slots))
        }),
        Err(GcError::Faulted)
    ));
    assert_eq!(runtime.gc_stats().objects, before.objects);
    assert_eq!(
        runtime.gc_stats().root_registrations,
        before.root_registrations
    );
    assert_eq!(runtime.resources().counters(), counters);
    assert_eq!(
        runtime
            .reflect_set_index(&RootedValue::unit(), &0i32.into(), &1i32.into())
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::EngineFault
    );
    assert_eq!(runtime.gc_stats().objects, before.objects);
    drop(root);
}
