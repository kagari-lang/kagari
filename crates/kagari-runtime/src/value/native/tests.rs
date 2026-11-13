//! Native identity revision admission is checked before lending mutable data.
use crate::{
    Runtime,
    error::RuntimeErrorKind,
    module::{ExecutableModule, InstalledProgram},
    native::{
        builder::ModuleBuilder,
        payload::NativeType,
        storage::{NativePayload, NativeStorage},
        typed::NativeContext,
    },
    test_support::update_programs::load_empty,
    value::native::NativeObject,
};
use kagari_gc::{
    error::GcError,
    object::{GcObject, ObjectCharge},
    scan::{EdgeVisitor, ScanBudget, ScanKind, ScanStatus},
};
use std::{
    cell::Cell,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

fn registered_type<T: NativePayload>(
    runtime: &mut Runtime,
    storage: NativeStorage,
) -> (InstalledProgram, ExecutableModule, NativeType<T>) {
    let mut builder = ModuleBuilder::new("provider_fixture", &Default::default()).unwrap();
    let mut payload = builder.define_type("Payload").unwrap();
    payload.native_storage(storage).unwrap();
    let payload = payload.finish().unwrap();
    builder.finish().unwrap().install(runtime).unwrap();
    let program = load_empty(runtime, "native-provider");
    let owner = runtime.select_program(&program).unwrap();
    let native_type = runtime
        .bind_native_type::<T>(&program.root(), &payload, &[])
        .unwrap();
    (program, owner, native_type)
}

#[test]
fn native_revision_survives_collection_and_rejects_edit_exhaustion() {
    let mut runtime = Runtime::default();
    let (_program, owner, native_type) =
        registered_type::<i32>(&mut runtime, NativeStorage::data::<i32>());
    runtime.collect_garbage().unwrap();
    let baseline = runtime.gc_stats().objects;
    let mut context = NativeContext::new(&mut runtime, &owner).unwrap();
    let object = native_type.create(&mut context, 1).unwrap();
    object
        .edit(&mut context, |value| {
            *value = 2;
            Ok(())
        })
        .unwrap();
    let value = context
        .conversion
        .encode_value(native_type.type_argument(), object.clone())
        .unwrap();
    let root = context.conversion.retain(&value).unwrap();
    drop(context);
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc_stats().objects, baseline + 2);
    runtime
        .memory
        .scope(&runtime.resources, |cx, _| {
            let header = cx.downcast::<NativeObject>(root.get(cx)?.object().unwrap())?;
            assert_eq!(cx.read(header, NativeObject::revision), 1);
            // SAFETY: only the scalar revision is changed to exercise the finite
            // exhaustion boundary. No edge, charge or scan position changes.
            unsafe {
                cx.with_object_mut(header, |header, mutation| {
                    header.revision = u64::MAX;
                    mutation.continue_scan();
                    Ok(())
                })
            }
        })
        .unwrap();
    let before = runtime.gc_stats();
    let calls = Cell::new(0);
    let mut context = NativeContext::new(&mut runtime, &owner).unwrap();
    let error = object
        .edit(&mut context, |value| {
            calls.set(calls.get() + 1);
            *value = 3;
            Ok(())
        })
        .unwrap_err();
    assert_eq!(error.kind(), RuntimeErrorKind::ModuleValidation);
    assert!(error.to_string().contains("native revision exhausted"));
    assert_eq!(calls.get(), 0);
    assert_eq!(object.read(&mut context, |value| Ok(*value)).unwrap(), 2);
    drop(context);
    runtime
        .memory
        .scope(&runtime.resources, |cx, _| {
            let header = cx.downcast::<NativeObject>(root.get(cx)?.object().unwrap())?;
            assert_eq!(cx.read(header, NativeObject::revision), u64::MAX);
            Ok::<_, GcError>(())
        })
        .unwrap();
    let after = runtime.gc_stats();
    assert_eq!(after.objects, before.objects);
    assert_eq!(after.indirect_bytes, before.indirect_bytes);
    assert!(!runtime.is_quarantined());
    drop((object, root));
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc_stats().objects, baseline);
}

#[derive(Debug)]
struct ChargedPayload {
    bytes: Vec<u8>,
    overflow: bool,
    queries: Arc<AtomicUsize>,
    drops: Arc<AtomicUsize>,
}

impl Drop for ChargedPayload {
    fn drop(&mut self) {
        self.drops.fetch_add(1, Ordering::Relaxed);
    }
}

impl NativePayload for ChargedPayload {
    fn registration_storage() -> NativeStorage {
        NativeStorage::payload::<Self>()
    }
}

// SAFETY: no edges. The Vec is the only owned backing and is never edited.
// The overflow branch is an explicit arithmetic-containment fault witness;
// neither diagnostic counter affects object contents, accounting or scanning.
unsafe impl GcObject for ChargedPayload {
    type ScanState = ();
    const SCAN_KIND: ScanKind = ScanKind::EdgeFree;

    fn begin_scan(&self) {}

    fn scan(
        &self,
        _: &mut (),
        _: &mut ScanBudget,
        _: &mut EdgeVisitor<'_>,
    ) -> Result<ScanStatus, GcError> {
        Ok(ScanStatus::Complete)
    }

    fn charge(&self) -> ObjectCharge {
        self.queries.fetch_add(1, Ordering::Relaxed);
        ObjectCharge {
            units: 7,
            bytes: if self.overflow {
                usize::MAX
            } else {
                self.bytes.capacity()
            },
        }
    }
}

#[test]
fn native_payload_charge_and_initialization_failure_release_each_owner_once() {
    let mut runtime = Runtime::default();
    let (_program, owner, native_type) =
        registered_type::<ChargedPayload>(&mut runtime, NativeStorage::payload::<ChargedPayload>());
    runtime.collect_garbage().unwrap();
    let baseline = runtime.gc_stats();
    let queries = Arc::new(AtomicUsize::new(0));
    let drops = Arc::new(AtomicUsize::new(0));
    let bytes = Vec::with_capacity(4096);
    let capacity = bytes.capacity();
    let mut context = NativeContext::new(&mut runtime, &owner).unwrap();
    let payload = native_type
        .create_with(&mut context, |init| {
            // SAFETY: edge-free fully initialized payload; no references can escape
            // this branded allocation into a different collector or Native header.
            unsafe {
                init.scope().alloc_with(|_| {
                    Ok(ChargedPayload {
                        bytes,
                        overflow: false,
                        queries: queries.clone(),
                        drops: drops.clone(),
                    })
                })
            }
        })
        .unwrap();
    assert_eq!(
        payload
            .read(&mut context, |payload| Ok(payload.bytes.capacity()))
            .unwrap(),
        capacity
    );
    drop(context);
    let allocated = runtime.gc_stats();
    assert_eq!(allocated.objects, baseline.objects + 2);
    assert_eq!(allocated.indirect_bytes, baseline.indirect_bytes + capacity);
    assert_eq!(allocated.provider_units, baseline.provider_units + 7);
    assert_eq!(queries.load(Ordering::Relaxed), 1);
    assert_eq!(drops.load(Ordering::Relaxed), 0);
    runtime.collect_garbage().unwrap();
    assert_eq!(queries.load(Ordering::Relaxed), 1);
    drop(payload);
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc_stats().objects, baseline.objects);
    assert_eq!(runtime.gc_stats().indirect_bytes, baseline.indirect_bytes);
    assert_eq!(runtime.gc_stats().provider_units, baseline.provider_units);
    assert_eq!(queries.load(Ordering::Relaxed), 1);
    assert_eq!(drops.load(Ordering::Relaxed), 1);
    let mut context = NativeContext::new(&mut runtime, &owner).unwrap();
    let failure = native_type
        .create_with(&mut context, |init| {
            // SAFETY: no edges or false pointer layout. Only the declared byte
            // count is deliberately invalid, exercising checked charge admission.
            unsafe {
                init.scope().alloc_with(|_| {
                    Ok(ChargedPayload {
                        bytes: Vec::new(),
                        overflow: true,
                        queries: queries.clone(),
                        drops: drops.clone(),
                    })
                })
            }
        })
        .unwrap_err();
    assert_eq!(failure.kind(), RuntimeErrorKind::ResourceLimitExceeded);
    drop(context);
    assert_eq!(queries.load(Ordering::Relaxed), 2);
    assert_eq!(drops.load(Ordering::Relaxed), 2);
    let after = runtime.gc_stats();
    assert_eq!(after.objects, baseline.objects);
    assert_eq!(after.indirect_bytes, baseline.indirect_bytes);
    assert_eq!(after.provider_units, baseline.provider_units);
    runtime.collect_garbage().unwrap();
    assert_eq!(queries.load(Ordering::Relaxed), 2);
    assert_eq!(drops.load(Ordering::Relaxed), 2);
    assert!(!runtime.is_quarantined());
}
