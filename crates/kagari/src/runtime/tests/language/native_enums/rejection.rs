use super::{artifact, engine};
use crate::program::PreparedProgram;
use kagari_runtime::module::entry::ProgramEntry;
use rstest::rstest;

#[rstest]
#[case::wrong_payload_type("fn main() { enums::wrong_payload(40, 2); }")]
#[case::wrong_payload_count("fn main() { enums::wrong_count(40, 2); }")]
#[case::foreign_variant("fn main() { enums::foreign_variant(40, 2); }")]
#[case::field_index("fn main() { enums::wrong_field(enums::make(40, 2)); }")]
#[case::variant_member("fn main() { enums::wrong_variant(enums::make(40, 2)); }")]
fn native_enum_operations_reject_wrong_payloads_members_and_indices(#[case] body: &str) {
    let engine = engine();
    let program = PreparedProgram::from_artifact(
        artifact(&engine, body),
        &Default::default(),
        &Default::default(),
    )
    .unwrap();
    let mut runtime = engine.runtime(Default::default());
    let loaded = runtime.load_program(&program, Default::default()).unwrap();
    assert!(
        runtime
            .execute(&loaded, ProgramEntry::Main, &[], &Default::default())
            .is_err(),
        "{body}"
    );
    assert_eq!(runtime.runtime().gc().active_roots(), 0);
    assert_eq!(
        runtime.runtime().resources().counters().current_call_depth,
        0
    );
    runtime.runtime().collect_garbage().unwrap();
    assert_eq!(runtime.runtime().gc().allocated_objects(), 0);
}
