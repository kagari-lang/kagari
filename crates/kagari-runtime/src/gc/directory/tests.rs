//! Physical lifecycle contracts, in addition to checked-identity/graph tests in gc.
use super::*;
use crate::{
    Runtime,
    module::ExecutableModule,
    native::{
        payload::{data::NativeData, take::NativeTake},
        storage::{
            NativePayload, NativeStorage,
            scan::{NativeScanKind, ScanBudget, ScanStatus},
            value::StoredValue,
        },
    },
    resource::HeapCharge,
    test_support::layouts::{allocation_owner, layout},
    value::Value,
};
use kagari_types::{scalar::BuiltinType, ty::Ty};
use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

#[derive(Clone, Copy, Debug)]
#[repr(align(64))]
struct Aligned([u8; 3]);

// SAFETY: fixed scalar bytes with no pointers or interior state.
unsafe impl NativeData for Aligned {}

#[derive(Debug)]
struct Probe<const N: usize> {
    drops: Arc<AtomicUsize>,
    panic: bool,
    bytes: [u8; N],
}

impl<const N: usize> Drop for Probe<N> {
    fn drop(&mut self) {
        self.drops.fetch_add(1, Ordering::SeqCst);
        assert!(!self.panic, "payload destructor probe");
    }
}

// SAFETY: only fixed scalar bytes and a Rust-only drop counter; no script edges.
unsafe impl<const N: usize> NativePayload for Probe<N> {
    type ScanState = ();
    const SCAN_KIND: NativeScanKind = NativeScanKind::EdgeFree;

    fn begin_scan(&self) {}

    fn scan<'a>(
        &'a self,
        _: &mut (),
        budget: &mut ScanBudget,
        _: &mut dyn FnMut(&'a StoredValue),
    ) -> ScanStatus {
        budget.charge(1);
        ScanStatus::Complete
    }

    fn charge(&self) -> HeapCharge {
        HeapCharge::default()
    }
}

// SAFETY: all state is Rust-owned and can outlive its Runtime.
unsafe impl<const N: usize> NativeTake for Probe<N> {}

static ZST_DROPS: AtomicUsize = AtomicUsize::new(0);

#[derive(Debug)]
struct Zst;

impl Drop for Zst {
    fn drop(&mut self) {
        ZST_DROPS.fetch_add(1, Ordering::SeqCst);
    }
}

// SAFETY: zero-sized and edge-free, with only a Rust global drop counter.
unsafe impl NativePayload for Zst {
    type ScanState = ();
    const SCAN_KIND: NativeScanKind = NativeScanKind::EdgeFree;

    fn begin_scan(&self) {}

    fn scan<'a>(
        &'a self,
        _: &mut (),
        budget: &mut ScanBudget,
        _: &mut dyn FnMut(&'a StoredValue),
    ) -> ScanStatus {
        budget.charge(1);
        ScanStatus::Complete
    }

    fn charge(&self) -> HeapCharge {
        HeapCharge::default()
    }
}

// SAFETY: zero-sized and edge-free.
unsafe impl NativeTake for Zst {}

fn allocate<T: NativeTake>(runtime: &Runtime, owner: &ExecutableModule, value: T) -> GcRef {
    let object = NativeStorage::takeable::<T>()
        .prepare_payload(runtime.gc(), &Ty::Builtin(BuiltinType::Unit), value, owner)
        .unwrap();
    runtime.gc().alloc_native(object).unwrap()
}

fn inline_address<T: 'static>(heap: &GcHeap, id: GcRef) -> usize {
    let objects = heap.objects.borrow();
    let object = heap.native_ref(&objects, id).unwrap();
    let (_, offset) = object.physical_layout().unwrap();
    let header = (object as *const NativeObject).addr();
    let location = objects.direct_location(id).unwrap();
    let checked = objects.pages.get(location).unwrap();
    let prefix = checked.descriptor.payload_offset();
    assert!(prefix >= ObjectHeader::SIZE);
    assert_eq!(checked.pointer.as_ptr().addr(), header);
    // SAFETY: this occupied allocation and both headers remain under the same
    // storage borrow; the checked descriptor accounts for alignment padding.
    let common = unsafe { ObjectHeader::read(checked.pointer.sub(prefix)) };
    assert_eq!(common.allocation_id, location.page);
    assert_ne!(common.descriptor_id, 0);
    let payload = (object.payload::<T>().unwrap() as *const T).addr();
    assert_eq!(
        payload,
        header + offset,
        "T must be adjacent, never a permanent Box"
    );
    assert_eq!(payload % align_of::<T>(), 0);
    payload
}

#[test]
fn inline_native_small_large_overaligned_and_zero_sized_lifecycles() {
    let mut runtime = Runtime::default();
    let owner = allocation_owner(&mut runtime);
    let drops = Arc::new(AtomicUsize::new(0));
    let small = allocate(
        &runtime,
        &owner,
        Probe {
            drops: drops.clone(),
            panic: false,
            bytes: [7; 3],
        },
    );
    let large = allocate(
        &runtime,
        &owner,
        Probe {
            drops: drops.clone(),
            panic: false,
            bytes: [9; 1200],
        },
    );
    let aligned = allocate(&runtime, &owner, Aligned([1, 2, 3]));
    let zst = allocate(&runtime, &owner, Zst);
    let prior_zst_drops = ZST_DROPS.load(Ordering::SeqCst);
    let roots =
        [small, large, aligned, zst].map(|id| runtime.root_value(Value::GcHandle(id)).unwrap());
    let addresses = [
        inline_address::<Probe<3>>(runtime.gc(), small),
        inline_address::<Probe<1200>>(runtime.gc(), large),
        inline_address::<Aligned>(runtime.gc(), aligned),
        inline_address::<Zst>(runtime.gc(), zst),
    ];
    let expected_backing = {
        let objects = runtime.gc().objects.borrow();
        let large = ObjectHeader::layout(
            runtime
                .gc()
                .native_ref(&objects, large)
                .unwrap()
                .physical_layout()
                .unwrap()
                .0,
        )
        .unwrap()
        .0
        .pad_to_align();
        let aligned = ObjectHeader::layout(
            runtime
                .gc()
                .native_ref(&objects, aligned)
                .unwrap()
                .physical_layout()
                .unwrap()
                .0,
        )
        .unwrap()
        .0
        .pad_to_align();
        assert_eq!(aligned.align(), 64);
        assert!(aligned.size() <= SLAB_BYTES);
        // Probe<3> and Zst may share an ordinary class; Aligned needs its own tier.
        let slabs = objects.pages.usage().slabs;
        assert!(slabs == 2 * SLAB_BYTES || slabs == 3 * SLAB_BYTES);
        assert_eq!(objects.pages.usage().large, large.size());
        slabs + large.size()
    };
    assert_eq!(
        runtime.gc().stats().current_retained_bytes,
        expected_backing as u128
    );
    runtime.collect_garbage().unwrap();
    assert_eq!(
        addresses,
        [
            inline_address::<Probe<3>>(runtime.gc(), small),
            inline_address::<Probe<1200>>(runtime.gc(), large),
            inline_address::<Aligned>(runtime.gc(), aligned),
            inline_address::<Zst>(runtime.gc(), zst)
        ]
    );
    assert_eq!(
        runtime.gc().take_native::<Aligned>(aligned).unwrap().0,
        [1, 2, 3]
    );
    let taken = runtime.gc().take_native::<Probe<1200>>(large).unwrap();
    assert_eq!(taken.bytes, [9; 1200]);
    assert_eq!(drops.load(Ordering::SeqCst), 0);
    assert!(runtime.gc().take_native::<Probe<1200>>(large).is_err());
    drop(taken);
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    let taken_zst = runtime.gc().take_native::<Zst>(zst).unwrap();
    assert_eq!(ZST_DROPS.load(Ordering::SeqCst), prior_zst_drops);
    drop(taken_zst);
    assert_eq!(ZST_DROPS.load(Ordering::SeqCst), prior_zst_drops + 1);
    drop(roots);
    runtime.collect_garbage().unwrap();
    assert_eq!(drops.load(Ordering::SeqCst), 2);
    assert_eq!(runtime.gc().stats().current_retained_bytes, 0);
    assert_eq!(runtime.gc().objects.borrow().backing_bytes(), 0);
}

#[test]
fn destructor_panics_quarantine_without_retry_and_teardown_drains_survivors() {
    let mut runtime = Runtime::default();
    let owner = allocation_owner(&mut runtime);
    let drops = Arc::new(AtomicUsize::new(0));
    // Cross multiple batches. Quarantine finishes only the already detached
    // batch; undiscovered owners remain for teardown, never another collector step.
    for index in 0..70 {
        allocate(
            &runtime,
            &owner,
            Probe {
                drops: drops.clone(),
                panic: index % 32 == 0,
                bytes: [0; 3],
            },
        );
    }
    let live = allocate(
        &runtime,
        &owner,
        Probe {
            drops: drops.clone(),
            panic: true,
            bytes: [0; 1200],
        },
    );
    let root = runtime.root_value(Value::GcHandle(live)).unwrap();
    let result = catch_unwind(AssertUnwindSafe(|| runtime.collect_garbage()));
    assert!(result.is_ok());
    assert!(runtime.is_quarantined());
    let disposed = drops.load(Ordering::SeqCst);
    assert!((1..=32).contains(&disposed));
    assert_eq!(
        runtime.gc().stats().reclaimed_objects,
        disposed,
        "every detached owner is disposed even after the first panic"
    );
    assert_eq!(runtime.gc().stats().current_heap_units, 71 - disposed);
    assert!(runtime.collect_garbage().is_err());
    assert_eq!(
        drops.load(Ordering::SeqCst),
        disposed,
        "quarantine forbids another detach or retry"
    );
    // Root and descriptor provenance may outlive teardown, but cannot keep payloads alive.
    assert!(catch_unwind(AssertUnwindSafe(|| drop(runtime))).is_ok());
    assert_eq!(drops.load(Ordering::SeqCst), 71);
    drop(root);
    drop(owner);
    assert_eq!(drops.load(Ordering::SeqCst), 71);
}

#[test]
fn inline_struct_bytes_do_not_move_and_release_the_last_sparse_page() {
    let mut runtime = Runtime::default();
    let shape = layout(
        &mut runtime,
        "Point",
        &[
            ("x", Ty::Builtin(BuiltinType::I32), true),
            ("y", Ty::Builtin(BuiltinType::I32), true),
        ],
    );
    let heap = runtime.gc();
    let mut ids = Vec::new();
    for i in 0..520 {
        ids.push(
            heap.alloc_struct(shape.clone(), vec![Value::I32(i), Value::I32(-i)])
                .unwrap(),
        );
    }
    let stale = heap.objects.borrow().direct_location(ids[0]).unwrap();
    let id = ids[17];
    let identity = heap.objects.borrow().direct_location(id).unwrap();
    let root = runtime.root_value(Value::Struct(id)).unwrap();
    let address = {
        let objects = heap.objects.borrow();
        let view = objects.structure(identity).unwrap();
        assert_eq!(view.bytes.len(), 8);
        view.bytes.as_ptr().addr()
    };
    assert_eq!(
        heap.objects.borrow().backing_bytes(),
        (520usize * 16).div_ceil(SLAB_BYTES) * SLAB_BYTES
    );
    #[cfg(feature = "execution-diagnostics")]
    {
        let usage = heap.storage_usage().unwrap();
        assert_eq!(usage.object_header_bytes, 520 * 8);
        assert_eq!(usage.initialized_payload_bytes, 520 * 16);
        assert_eq!(
            usage.initialized_payload_bytes + usage.unused_backing_bytes,
            usage.slab_bytes + usage.large_bytes
        );
    }
    runtime.collect_garbage().unwrap();
    assert_eq!(heap.objects.borrow().backing_bytes(), SLAB_BYTES);
    {
        let objects = heap.objects.borrow();
        let view = objects.structure(identity).unwrap();
        assert_eq!(view.bytes.as_ptr().addr(), address);
        assert_eq!(view.get(0), Some(Value::I32(17)));
        assert_eq!(view.get(1), Some(Value::I32(-17)));
    }
    assert!(heap.objects.borrow().structure(stale).is_none());
    drop(root);
    runtime.collect_garbage().unwrap();
    assert_eq!(heap.objects.borrow().backing_bytes(), 0);
}
