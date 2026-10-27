use super::*;
use crate::collections::vector::{ScriptVec, VecValues};
use kagari_common::cancellation::CancellationToken;
use kagari_compiler::{bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir};
use kagari_hir::analysis::AnalysisDatabase;
use kagari_runtime::{
    module::ExecutableModule,
    native::{module::NativeModule, typed::NativeContext},
};
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use std::{cell::Cell, sync::Arc};

#[test]
fn scalar_partial_order_preserves_signed_zero_and_leaf_permutation() {
    let runtime = Runtime::new(Default::default());
    let mut values = vec![1.0_f64, -0.0, 0.0, -1.0];
    sort_range(&mut values, &mut Vec::new(), &mut None, &runtime, 0, 4).unwrap();
    assert_eq!(
        values.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
        [-1.0_f64, -0.0, 0.0, 1.0].map(f64::to_bits)
    );
    let mut values = (0..32).rev().map(|i| i as f64).collect::<Vec<_>>();
    values[17] = f64::NAN;
    let mut expected = values.iter().map(|v| v.to_bits()).collect::<Vec<_>>();
    expected.sort_unstable();
    assert!(sort_range(&mut values, &mut Vec::new(), &mut None, &runtime, 0, 32).is_err());
    let mut actual = values.iter().map(|v| v.to_bits()).collect::<Vec<_>>();
    actual.sort_unstable();
    assert_eq!(actual, expected);
}

fn fixture() -> (Runtime, ExecutableModule) {
    let modules = crate::modules().unwrap();
    let mut analysis = AnalysisDatabase::default();
    analysis.set_native_modules(
        modules
            .iter()
            .map(|module| Arc::new(module.to_declaration()))
            .collect(),
    );
    let mut sources = SourceDatabase::default();
    let root = sources
        .set(
            "scalar_merge.kgr",
            "fn signature(value: Vec<f64>) {}".into(),
            SourceLayer::Base,
        )
        .unwrap();
    let snapshot = analysis
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let checked = snapshot.check_program(root, &Default::default()).unwrap();
    let mir = lower_program_to_mir(&checked, &Default::default()).unwrap();
    let program = lower_program_to_bytecode(&mir, &Default::default())
        .unwrap()
        .into_unverified();
    let mut runtime = Runtime::new(Default::default());
    NativeModule::install_all(&modules, &mut runtime).unwrap();
    let loaded = runtime.load_program("scalar_merge", program).unwrap();
    let owner = runtime.select_program(&loaded).unwrap();
    (runtime, owner)
}

#[test]
fn fallible_sort_fallback_preserves_unordered_values_and_allows_reuse() {
    let (runtime, owner) = fixture();
    let mut cx = NativeContext::new(&runtime, &owner).unwrap();
    let values = vec![3.0_f64, f64::NAN, -0.0, 0.0, 1.0];
    let mut expected = values
        .iter()
        .map(|value| value.to_bits())
        .collect::<Vec<_>>();
    expected.sort_unstable();
    let root = cx.conversion().encode(VecValues(values)).unwrap();
    let vector: ScriptVec<f64> = cx.conversion().decode(&root).unwrap();
    // The ordinary fallible kernel is also the configured abort-target fallback.
    // PartialOrd failure returns through its public guard, without any unwind.
    let error = vector
        .sort_by(&mut cx, |_, a, b| {
            a.partial_cmp(&b).ok_or_else(|| {
                RuntimeError::new(
                    RuntimeErrorKind::ScriptTrap,
                    "total comparison cannot be unordered",
                )
            })
        })
        .unwrap_err();
    assert_eq!(error.kind(), RuntimeErrorKind::ScriptTrap);
    let mut actual = vector
        .with_slice::<f64, _>(&cx, |values| {
            Ok(values
                .iter()
                .map(|value| value.to_bits())
                .collect::<Vec<_>>())
        })
        .unwrap();
    actual.sort_unstable();
    assert_eq!(actual, expected);
    vector.push(&mut cx, 7.0).unwrap();
    drop(vector);
    drop(root);
    drop(cx);
    assert_eq!(runtime.collect_garbage().unwrap().live_objects, 0);
    assert_eq!(runtime.gc().active_roots(), 0);
}

#[test]
fn unordered_large_merge_recovers_through_the_public_payload_guard() {
    let (runtime, owner) = fixture();
    let mut values = (0..512)
        .map(|i| (2 * i) as f64)
        .chain((0..512).map(|i| (2 * i + 1) as f64))
        .collect::<Vec<_>>();
    // Inject an unordered value in a pre-sorted run to exercise the merge failure
    // itself, after many writes. Ordinary sort catches NaN in its leaf first.
    values[600] = f64::NAN;
    let mut expected = values.iter().map(|v| v.to_bits()).collect::<Vec<_>>();
    expected.sort_unstable();
    let mut cx = NativeContext::new(&runtime, &owner).unwrap();
    let root = cx.conversion().encode(VecValues(values)).unwrap();
    let vector: ScriptVec<f64> = cx.conversion().decode(&root).unwrap();
    let object = vector.object.borrow(&cx).unwrap();
    // SAFETY: This is the production scalar recovery protocol with scalar buffers;
    // no callback or collection can run while its edit loan is held.
    let guard = unsafe {
        object.try_enter_operation(&cx, NativeOperationKind::ExclusiveSlots, Some(recover))
    }
    .unwrap()
    .unwrap();
    let error = unsafe {
        object.edit(
            &mut cx,
            Some(&guard),
            EditPlan {
                max_logical_units: 1536,
                structural: false,
            },
            |v, _| {
                let (SequenceStorage::F64(values), SequenceStorage::F64(scratch)) =
                    (&mut v.values, &mut v.scratch)
                else {
                    panic!("f64 storage")
                };
                scratch.try_reserve(512).unwrap();
                merge_range(values, scratch, &mut v.work, &runtime, 0..1024, 512)
            },
        )
    }
    .unwrap_err();
    assert_eq!(error.kind(), RuntimeErrorKind::ScriptTrap);
    drop(guard);
    let mut actual = vector
        .with_slice::<f64, _>(&cx, |values| {
            Ok(values.iter().map(|v| v.to_bits()).collect::<Vec<_>>())
        })
        .unwrap();
    actual.sort_unstable();
    assert_eq!(actual, expected);
    unsafe {
        object.read(&cx, None, |v, _| {
            assert!(v.work.is_none());
            assert_eq!(v.scratch.retained_bytes(), 0);
            Ok(())
        })
    }
    .unwrap();
    vector.push(&mut cx, 7.0).unwrap();
    drop(object);
    drop(vector);
    drop(root);
    drop(cx);
    assert_eq!(runtime.collect_garbage().unwrap().live_objects, 0);
}

#[derive(Clone, Copy, PartialEq)]
struct CancelOnCrossing<'a> {
    value: i32,
    side: bool,
    state: &'a CancelState,
}

struct CancelState {
    token: CancellationToken,
    crosses: Cell<usize>,
}

impl PartialEq for CancelState {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self, other)
    }
}

impl PartialOrd for CancelOnCrossing<'_> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        if self.side != other.side {
            let crosses = self.state.crosses.get() + 1;
            self.state.crosses.set(crosses);
            if crosses == 70 {
                self.state.token.cancel();
            }
        }
        self.value.partial_cmp(&other.value)
    }
}

#[test]
fn cancellation_during_large_merge_publishes_a_recoverable_gap() {
    let (runtime, owner) = fixture();
    let options = runtime.execution_options();
    let state = CancelState {
        token: options.cancellation.clone(),
        crosses: Cell::new(0),
    };
    let session = runtime.begin_execution(&owner, options).unwrap();
    // Test-only PartialOrd injects asynchronous cancellation deterministically.
    // Production dispatch admits only the closed pointer-free builtin variants.
    let mut values = (0..1024)
        .map(|i| CancelOnCrossing {
            value: (i % 512) * 2 + i / 512,
            side: i >= 512,
            state: &state,
        })
        .collect::<Vec<_>>();
    let mut scratch = Vec::with_capacity(512);
    let mut work = None;
    let error = sort_range(&mut values, &mut scratch, &mut work, &runtime, 0, 1024).unwrap_err();
    assert_eq!(error.kind(), RuntimeErrorKind::Cancelled);
    assert_eq!(state.crosses.get(), 128, "merge polls every 64 transfers");
    let Some(Work::Merge {
        scratch: next,
        output,
        ..
    }) = work.take()
    else {
        panic!("merge recovery must own the gap")
    };
    values[output..output + scratch.len() - next].copy_from_slice(&scratch[next..]);
    scratch.clear();
    let mut actual = values.iter().map(|v| v.value).collect::<Vec<_>>();
    actual.sort_unstable();
    assert_eq!(actual, (0..1024).collect::<Vec<_>>());
    drop(session);
    sort_range(&mut values, &mut scratch, &mut work, &runtime, 0, 1024).unwrap();
    assert!(values.windows(2).all(|pair| pair[0].value <= pair[1].value));
    assert!(scratch.is_empty());
    assert!(work.is_none());
}
