use crate::tests::execution::common::load_test_module;
use kagari_runtime::{
    Runtime,
    error::{
        RuntimeErrorKind,
        reason::{ErrorReason, IndexError},
    },
    module::entry::ProgramEntry,
    native::conversion::context::ConversionContext,
    value::view::ValueView,
};

#[test]
fn interpreter_conformance_executes_control_flow_match_arrays_and_structs() {
    let (runtime, loaded) = load_test_module(include_str!(
        "../../../tests/fixtures/execution/control-flow-aggregates.kgr"
    ));
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();
    let mut vm = runtime;
    let report = vm
        .execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
        .expect("vm should execute");

    assert_eq!(report.return_value.as_i32(), Some(13));
}

#[test]
fn machine_boundaries_preserve_wide_indices_and_repeat_resource_errors() {
    let (runtime, loaded) = load_test_module(include_str!(
        "../../../tests/fixtures/execution/machine-index-boundaries.kgr"
    ));
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();
    let mut vm = runtime;
    vm.collect_garbage().unwrap();
    let baseline = vm.gc_stats();
    assert_eq!(
        vm.execute_typed::<_, i32>(&_loaded_program, "read", (0_u64,), &Default::default())
            .unwrap(),
        7
    );
    assert_eq!(
        vm.execute_typed::<_, u64>(&_loaded_program, "repeat", (1_u64,), &Default::default())
            .unwrap(),
        1
    );
    for index in [1_u64 << 32, u64::MAX] {
        for name in ["read", "write"] {
            let error = if name == "read" {
                vm.execute_typed::<_, i32>(&_loaded_program, name, (index,), &Default::default())
                    .unwrap_err()
            } else {
                vm.execute_typed::<_, ()>(&_loaded_program, name, (index,), &Default::default())
                    .unwrap_err()
            };
            if name == "write" {
                assert!(
                    matches!(error.reason(), ErrorReason::Index(IndexError::InvalidIndex { index: actual }) if *actual == index),
                    "{error:?}"
                );
            } else {
                assert!(
                    matches!(&error, error if error.kind() == RuntimeErrorKind::IndexOutOfBounds && error.to_string().contains(&index.to_string())),
                    "{error:?}"
                );
            }
        }
    }
    let error = vm
        .execute_typed::<_, i32>(
            &_loaded_program,
            "signed",
            (1_i64 << 32,),
            &Default::default(),
        )
        .unwrap_err();
    assert!(
        matches!(
            error.reason(),
            ErrorReason::Index(IndexError::InvalidIndex {
                index: 4_294_967_296
            })
        ),
        "{error:?}"
    );
    for count in [u64::MAX, isize::MAX as u64 / 8 + 1] {
        let error = vm
            .execute_typed::<_, u64>(&_loaded_program, "repeat", (count,), &Default::default())
            .unwrap_err();
        assert!(
            matches!(&error, error if error.kind() == RuntimeErrorKind::ResourceLimitExceeded),
            "{error:?}"
        );
    }
    for index in [1_u64 << 32, u64::MAX] {
        let report = vm
            .execute_module(&loaded, "make", vm.execution_options())
            .unwrap();
        let error = vm
            .with_host_scope(|scope| {
                let value = scope.import(&report.return_value)?;
                Runtime::execute_scoped(
                    scope,
                    &_loaded_program,
                    "write_order",
                    &[value, ValueView::from(index)],
                    &Default::default(),
                )
                .map(|_| ())
            })
            .unwrap_err();
        assert_eq!(error.kind(), RuntimeErrorKind::IndexOutOfBounds);
        assert!(error.to_string().contains(&index.to_string()));
        let mut cx = ConversionContext::new(&mut vm, &loaded).unwrap();
        let value = cx.import(&report.return_value).unwrap();
        assert_eq!(cx.array_len(&value).unwrap(), 2);
        assert_eq!(
            cx.array_element(&value, 0usize).unwrap().unwrap().as_i32(),
            Some(123)
        );
        assert_eq!(
            cx.array_element(&value, 1usize).unwrap().unwrap().as_i32(),
            Some(7)
        );
    }
    vm.collect_garbage().unwrap();
    assert_eq!(vm.gc_stats().objects, baseline.objects);
    assert_eq!(
        vm.gc_stats().root_registrations,
        baseline.root_registrations
    );
    assert_eq!(vm.resources().counters().current_call_depth, 0);
}
