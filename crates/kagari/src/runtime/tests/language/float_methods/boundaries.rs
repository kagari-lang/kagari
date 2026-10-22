use super::{FLOATS, assert_checks, check_source, context, engine, programs};
use kagari_runtime::{module::entry::ProgramEntry, native::objects::Object};

#[test]
fn receivers_and_arguments_run_once_in_left_to_right_order() {
    let mut source = String::from("struct Log { var sequence: i32 }\n");
    let mut checks = Vec::new();
    for float in FLOATS {
        let ty = float.ty;
        let uint = float.uint;
        source.push_str(&format!(
            "fn receiver{ty}(log: Log) -> {ty} {{ log.sequence = log.sequence * 10 + 1; 0.5{ty} }}\n\
             fn low{ty}(log: Log) -> {ty} {{ log.sequence = log.sequence * 10 + 2; -1.0{ty} }}\n\
             fn high{ty}(log: Log) -> {ty} {{ log.sequence = log.sequence * 10 + 3; 1.0{ty} }}\n\
             fn bits{ty}(log: Log) -> {uint} {{ log.sequence = log.sequence * 10 + 1; 0{uint} }}\n"
        ));
        for (expression, expected_sequence, expected_bits) in [
            (format!("receiver{ty}(log).abs()"), 1, float.bits(0.5)),
            (
                format!("receiver{ty}(log).min(low{ty}(log))"),
                12,
                float.bits(-1.0),
            ),
            (
                format!("receiver{ty}(log).max(low{ty}(log))"),
                12,
                float.bits(0.5),
            ),
            (
                format!("receiver{ty}(log).clamp(low{ty}(log), high{ty}(log))"),
                123,
                float.bits(0.5),
            ),
            (format!("{ty}::from_bits(bits{ty}(log))"), 1, 0),
        ] {
            checks.push(format!(
                "{{ val log = Log {{ sequence: 0 }}; val result = {expression}; \
                 log.sequence == {expected_sequence} && result.to_bits() == {expected_bits}{uint} }}"
            ));
        }
    }
    source.push_str(&check_source(checks));
    for (route, program) in programs(&source) {
        assert_checks(&program, route, &source);
    }
}

#[test]
fn invalid_clamp_bounds_trap_after_arguments_and_release_roots_without_poisoning_runtime() {
    let mut source = String::from(
        "struct Log { var sequence: i32 }\n\
         fn main() -> Log { Log { sequence: 0 } }\n\
         pub fn inspect(log: Log) -> i32 { log.sequence }\n\
         pub fn after() -> bool { val pressure = Vec::from([1, 2, 3]); \
         pressure.len() == 3u64 && (0.5f32).clamp(-1.0f32, 1.0f32) == 0.5f32 \
         && (0.5f64).clamp(-1.0f64, 1.0f64) == 0.5f64 }\n",
    );
    let mut failures = Vec::new();
    for float in FLOATS {
        let ty = float.ty;
        for (name, digit) in [("receiver", 1), ("low", 2), ("high", 3)] {
            source.push_str(&format!(
                "fn {name}{ty}(log: Log, value: {ty}) -> {ty} {{ \
                 log.sequence = log.sequence * 10 + {digit}; value }}\n"
            ));
        }
        // Cartesian bound categories cover reversed finite/infinite bounds,
        // either/both NaN bounds, and validation despite a NaN receiver.
        let bounds = [
            float.infinity | float.sign,
            float.bits(-1.0),
            float.bits(1.0),
            float.infinity,
            float.quiet_nan,
            float.signaling_nan | float.sign,
        ];
        for (low_index, low) in bounds.into_iter().enumerate() {
            for (high_index, high) in bounds.into_iter().enumerate() {
                if low_index < 4 && high_index < 4 && low_index <= high_index {
                    continue;
                }
                for receiver in [0, float.quiet_nan] {
                    let name = format!("fail{}", failures.len());
                    source.push_str(&format!(
                        "pub fn {name}(log: Log) -> bool {{ log.sequence = 0; \
                         val pressure = Vec::from([log]); \
                         val result = receiver{ty}(log, {}).clamp(\
                         low{ty}(log, {}), high{ty}(log, {})); \
                         pressure.len() == 1u64 && result.is_finite() }}\n",
                        float.value(receiver),
                        float.value(low),
                        float.value(high)
                    ));
                    failures.push((
                        name,
                        format!("{ty}: receiver={receiver:#x}, min={low:#x}, max={high:#x}"),
                    ));
                }
            }
        }
    }
    for (route, program) in programs(&source) {
        let consumer = engine();
        let context = context();
        let mut runtime = consumer.runtime(context.clone());
        let loaded = runtime.load_program(&program, Default::default()).unwrap();
        let retained: Object = runtime
            .execute_typed(&loaded, ProgramEntry::Main, (), &context)
            .unwrap();
        let baseline_roots = runtime.runtime().gc().active_roots();
        assert!(baseline_roots > 0);
        for (name, case) in &failures {
            let error = runtime
                .execute_typed::<_, bool>(&loaded, name.as_str(), (retained.clone(),), &context)
                .unwrap_err();
            assert_eq!(
                error.code(),
                "KG_RUNTIME_SCRIPT_TRAP",
                "{route}: {case}: {error:?}"
            );
            assert_eq!(
                runtime.runtime().gc().active_roots(),
                baseline_roots,
                "{route}: {case}"
            );
            assert_eq!(
                runtime.runtime().resources().counters().current_call_depth,
                0
            );
            assert!(!runtime.runtime().is_quarantined(), "{route}: {case}");
            runtime.runtime().collect_garbage().unwrap();
            let sequence: i32 = runtime
                .execute_typed(&loaded, "inspect", (retained.clone(),), &context)
                .unwrap();
            assert_eq!(
                sequence, 123,
                "{route}: {case}: completed effects survive the trap"
            );
            let after: bool = runtime
                .execute_typed(&loaded, "after", (), &context)
                .unwrap();
            assert!(after, "{route}: {case}: execution can continue");
            assert_eq!(runtime.runtime().gc().active_roots(), baseline_roots);
        }
        assert!(runtime.runtime().gc().stats().collections > 0);
        drop(retained);
        assert_eq!(runtime.runtime().gc().active_roots(), 0, "{route}");
        runtime.runtime().collect_garbage().unwrap();
    }
}
