use crate::{
    Runtime,
    error::{RuntimeError, RuntimeErrorKind},
    native::conversion::context::ConversionContext,
    test_support::{layouts::allocation_owner, update_programs::load_empty},
    value::{carriers::RootedValue, view::ValueView},
};
use kagari_types::{scalar::BuiltinType, ty::Ty};
use std::ops::Bound::{Excluded, Included, Unbounded};

fn snapshot(cx: &mut ConversionContext<'_>, value: &ValueView<'_>) -> Vec<i32> {
    (0..cx.array_len(value).unwrap())
        .map(|index| {
            cx.array_element(value, index)
                .unwrap()
                .unwrap()
                .as_i32()
                .unwrap()
        })
        .collect()
}

#[test]
fn bulk_failure_preserves_slots_and_releases_preparation_resources() -> Result<(), RuntimeError> {
    let mut runtime = Runtime::default();
    let owner = allocation_owner(&mut runtime);
    let array = runtime.alloc_array(
        &owner,
        Ty::Builtin(BuiltinType::I32),
        &[RootedValue::from(1i32), RootedValue::from(2i32)],
    )?;
    let short = runtime.alloc_array(
        &owner,
        Ty::Builtin(BuiltinType::I32),
        &[RootedValue::from(0i32)],
    )?;
    let mut foreign = Runtime::default();
    let foreign_owner = allocation_owner(&mut foreign);
    let foreign_array = foreign.alloc_array(&foreign_owner, Ty::Builtin(BuiltinType::I32), &[])?;
    let mut foreign_cx = ConversionContext::new(&mut foreign, &foreign_owner)?;
    let foreign_value = foreign_cx.import(&foreign_array)?;
    let mut cx = ConversionContext::new(&mut runtime, &owner)?;
    let array = cx.import(&array)?;
    let short = cx.import(&short)?;
    let before = cx.runtime().resources().counters().current_heap_units;
    let error = cx.array_copy_from(&array, &short).unwrap_err();
    assert_eq!(error.kind(), RuntimeErrorKind::ScriptTrap);
    assert_eq!(snapshot(&mut cx, &array), [1, 2]);
    assert_eq!(
        cx.array_fill(&array, &foreign_value).unwrap_err().kind(),
        RuntimeErrorKind::ModuleValidation
    );
    assert_eq!(
        cx.array_copy_from(&array, &foreign_value)
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ModuleValidation
    );
    assert_eq!(
        cx.runtime().resources().counters().current_heap_units,
        before
    );
    // Value admission precedes bounds for set, preserving the original trap
    // even when a bad index would independently fail afterward.
    assert_eq!(
        cx.array_set(&array, usize::MAX, &ValueView::from(1i64))
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ScriptTrap
    );
    assert_eq!(
        cx.array_set(&array, usize::MAX, &ValueView::from(1i32))
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::IndexOutOfBounds
    );
    assert_eq!(
        cx.array_len(&ValueView::unit()).unwrap_err().kind(),
        RuntimeErrorKind::ScriptTrap
    );
    cx.array_fill(&array, &ValueView::from(7i32))?;
    assert_eq!(snapshot(&mut cx, &array), [7, 7]);
    assert_eq!(
        cx.runtime().resources().counters().current_heap_units,
        before
    );
    Ok(())
}

#[test]
fn copy_within_validates_before_commit_and_accounts_temporary_storage() -> Result<(), RuntimeError>
{
    let mut runtime = Runtime::default();
    let owner = allocation_owner(&mut runtime);
    let original = [1, 2, 3, 4];
    let inputs = original.map(RootedValue::from);
    let target = runtime.alloc_array(&owner, Ty::Builtin(BuiltinType::I32), &inputs)?;
    let options = runtime.execution_options();
    let mut session = runtime.begin_execution(&owner, options)?;
    let mut cx = ConversionContext::new(session.runtime_mut(), &owner)?;
    let target = cx.import(&target)?;
    let before = cx.runtime().resources().counters().current_heap_units;
    for (start, end, destination) in [
        (Included(3), Excluded(1), 0),
        (Included(0), Excluded(5), 0),
        (Unbounded, Unbounded, 1),
        (Included(4), Excluded(4), 5),
        (Excluded(usize::MAX), Unbounded, 0),
        (Unbounded, Included(usize::MAX), 0),
    ] {
        assert_eq!(
            cx.array_copy_within(&target, start, end, destination)
                .unwrap_err()
                .kind(),
            RuntimeErrorKind::IndexOutOfBounds
        );
        assert_eq!(snapshot(&mut cx, &target), original);
        assert_eq!(
            cx.runtime().resources().counters().current_heap_units,
            before
        );
    }
    cx.array_copy_within(&target, Included(0), Excluded(3), 1)?;
    assert_eq!(snapshot(&mut cx, &target), [1, 1, 2, 3]);
    assert_eq!(
        cx.runtime().resources().counters().current_heap_units,
        before
    );
    drop(cx);
    assert_eq!(session.counters().current_heap_units, before);
    assert_eq!(session.counters().peak_heap_units, before + 3);
    Ok(())
}

#[test]
fn repeat_allocation_rejects_length_and_byte_capacity_without_publication()
-> Result<(), RuntimeError> {
    let mut runtime = Runtime::default();
    let owner = allocation_owner(&mut runtime);
    let before = runtime.gc_stats();
    for (element, value, count, kind) in [
        (
            BuiltinType::Unit,
            RootedValue::unit(),
            usize::MAX,
            RuntimeErrorKind::ResourceLimitExceeded,
        ),
        (
            BuiltinType::U64,
            RootedValue::from(7u64),
            isize::MAX as usize / 8 + 1,
            RuntimeErrorKind::AllocationFailure,
        ),
    ] {
        let error = runtime
            .alloc_array_repeat(&owner, Ty::Builtin(element), &value, count)
            .unwrap_err();
        assert_eq!(error.kind(), kind);
        let after = runtime.gc_stats();
        assert_eq!(after.objects, before.objects);
        assert_eq!(after.provider_units, before.provider_units);
        assert_eq!(after.root_registrations, before.root_registrations);
    }
    let value = RootedValue::from(1u64 << 32);
    let root = runtime.alloc_array_repeat(&owner, Ty::Builtin(BuiltinType::U64), &value, 1)?;
    let mut cx = ConversionContext::new(&mut runtime, &owner)?;
    let array = cx.import(&root)?;
    assert_eq!(
        cx.array_element(&array, 0usize)?.unwrap().as_u64(),
        Some(1u64 << 32)
    );
    Ok(())
}

#[test]
fn candidate_heap_mutations_cannot_modify_preexisting_arrays() {
    let mut runtime = Runtime::default();
    let baseline_program = load_empty(&mut runtime, "candidate-array");
    let baseline = runtime.select_program(&baseline_program).unwrap();
    runtime.collect_garbage().unwrap();
    let published = runtime.gc_stats();
    let retained = runtime
        .alloc_array(
            &baseline,
            Ty::Builtin(BuiltinType::I32),
            &[RootedValue::from(7i32)],
        )
        .unwrap();
    let candidate = runtime
        .stage_reload_verified_program(&baseline_program, None, baseline.verified_program().clone())
        .unwrap();
    // Keep an actual preexisting source view across the phase switch. This reaches
    // each operation's candidate check rather than failing only at root import.
    let mut outer = runtime.host_scope(&[]).unwrap();
    let array = outer.import(&retained).unwrap();
    let mut session = outer
        .runtime_mut()
        .begin_candidate_initialization(&candidate)
        .unwrap();
    let local_root = {
        let mut cx = ConversionContext::new(session.runtime_mut(), candidate.module()).unwrap();
        let before = cx.runtime().resources().counters();
        let nine = ValueView::from(9i32);
        assert_eq!(
            cx.array_set(&array, 0usize, &nine).unwrap_err().kind(),
            RuntimeErrorKind::ExecutionPhaseViolation
        );
        assert_eq!(
            cx.array_fill(&array, &nine).unwrap_err().kind(),
            RuntimeErrorKind::ExecutionPhaseViolation
        );
        assert_eq!(
            cx.array_copy_within(&array, Included(0), Excluded(1), 0)
                .unwrap_err()
                .kind(),
            RuntimeErrorKind::ExecutionPhaseViolation
        );
        assert_eq!(
            cx.array_len(&array).unwrap_err().kind(),
            RuntimeErrorKind::ExecutionPhaseViolation
        );
        assert_eq!(
            cx.array_element(&array, 0usize).unwrap_err().kind(),
            RuntimeErrorKind::ExecutionPhaseViolation
        );
        assert_eq!(
            cx.import(&retained).unwrap_err().kind(),
            RuntimeErrorKind::ExecutionPhaseViolation
        );
        assert_eq!(cx.runtime().resources().counters(), before);
        let local = cx
            .runtime_mut()
            .alloc_array(
                candidate.module(),
                Ty::Builtin(BuiltinType::I32),
                &[RootedValue::from(1i32)],
            )
            .unwrap();
        let value = cx.import(&local).unwrap();
        cx.array_set(&value, 0usize, &ValueView::from(2i32))
            .unwrap();
        cx.runtime_mut().collect_garbage().unwrap();
        assert_eq!(
            cx.array_element(&value, 0usize).unwrap().unwrap().as_i32(),
            Some(2)
        );
        local
    };
    drop(session);
    {
        let mut cx = ConversionContext::new(outer.runtime_mut(), &baseline).unwrap();
        assert_eq!(cx.array_len(&array).unwrap(), 1);
        assert_eq!(
            cx.array_element(&array, 0usize).unwrap().unwrap().as_i32(),
            Some(7)
        );
        cx.array_set(&array, 0usize, &ValueView::from(9i32))
            .unwrap();
        assert_eq!(
            cx.array_element(&array, 0usize).unwrap().unwrap().as_i32(),
            Some(9)
        );
    }
    drop(outer);
    drop(candidate);
    drop(local_root);
    drop(retained);
    runtime.collect_garbage().unwrap();
    assert_eq!(
        runtime.gc_stats().root_registrations,
        published.root_registrations
    );
    assert_eq!(runtime.gc_stats().objects, published.objects);
    assert!(!runtime.is_quarantined());
}
