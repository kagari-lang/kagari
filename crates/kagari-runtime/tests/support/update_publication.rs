//! Publication assertions compiled against each consuming test harness's runtime.
use kagari_runtime::{
    Runtime,
    module::{Program, VerifiedProgram, state::StaticState},
};

pub fn publish_without_execution(
    runtime: &mut Runtime,
    loaded_program: &Program,
    candidate: VerifiedProgram,
) {
    let loaded = runtime.select_program(loaded_program).unwrap();
    let history = loaded.retained_demands().metadata().records().clone();
    let staged = runtime
        .stage_reload_verified_program(loaded_program, None, candidate)
        .unwrap();
    let current = runtime.select_program(loaded_program).unwrap();
    assert_eq!(current.key(), loaded.key());
    assert_eq!(current.retained_demands().metadata().records(), &history);
    runtime.publish_staged_reload(staged).unwrap();

    let published = runtime.select_program(loaded_program).unwrap();
    assert_ne!(published.key(), loaded.key());
    assert_eq!(runtime.resources().counters().peak_call_depth, 0);
    assert_eq!(runtime.gc().active_roots(), 0);
    for member in published.members() {
        assert!(
            runtime
                .module_instance_snapshot(&member)
                .unwrap()
                .module_slots
                .iter()
                .all(|slot| matches!(slot, StaticState::Uninitialized(_)))
        );
    }
}
