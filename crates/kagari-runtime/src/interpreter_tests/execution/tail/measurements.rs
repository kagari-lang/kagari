//! Setup and teardown stay outside the measured uninterrupted execution interval.
use super::{SOURCE, runtime};
use crate::{
    diagnostics::{self, allocations},
    executor::Executor,
    interpreter_tests::common::{compile_test_bytecode, round_trip_bytecode},
    native::conversion::arguments::ArgumentValues,
    test_support::bytecode::load_bytecode,
    value::view::ValueView,
};

#[test]
#[ignore = "manual execution allocation counts, not throughput"]
fn warmed_tail_replacement_allocates_no_iteration_storage_or_roots() {
    let code = round_trip_bytecode(compile_test_bytecode(SOURCE));
    let mut runtime = runtime();
    let program = load_bytecode(&mut runtime, "tail-allocation", code).unwrap();
    let owner = runtime.select_program(&program).unwrap();
    let function = owner
        .bytecode
        .functions
        .iter()
        .find(|f| f.name == "sum")
        .unwrap()
        .id;
    let mut capacity = None;
    for n in [10_000i64, 100_000] {
        let arguments = [ValueView::from(n), ValueView::from(0i64)];
        let mut executor = Executor::new(
            &mut runtime,
            &owner,
            function,
            ArgumentValues::scalars(&arguments),
        )
        .unwrap();
        let ((result, counts), allocation) = allocations::measure(|| {
            diagnostics::measure(|| {
                executor.run(|runtime, stack, value| {
                    let value = stack.retain_result(runtime, value)?;
                    let session = runtime.resources.active_session().unwrap();
                    let frames = runtime.resources.sessions.frames(session.id).unwrap();
                    assert!(frames.is_empty(), "the completed root frame has retired");
                    let reserved = runtime
                        .memory
                        .execution_space(session.id)
                        .unwrap()
                        .reserved();
                    Ok((value, reserved))
                })
            })
        });
        let (result, reserved) = result.unwrap();
        assert_eq!(result.as_i64(), Some(n * (n + 1) / 2));
        assert_eq!(allocation.requests, 0);
        assert_eq!(allocation.requested_bytes, 0);
        assert_eq!(allocation.net_bytes, 0);
        assert_eq!(counts.tail_replacements, n as u64);
        assert_eq!(counts.argument_transfers, 2 * n as u64);
        assert_eq!(counts.call_admissions, 0);
        assert_eq!(counts.call_selections, 0);
        assert_eq!(counts.bank_reservations, 0);
        assert_eq!(counts.root_registrations, 0);
        assert_eq!(counts.metadata_graph_admissions, 0);
        assert!(capacity.is_none_or(|previous| previous == reserved));
        capacity = Some(reserved);
        println!(
            "TAIL_ALLOCATIONS,steps={n},reserved={reserved},allocation={allocation:?},counts={counts:?}"
        );
        drop(executor);
        assert_eq!(runtime.resources().counters().current_call_depth, 0);
    }
}
