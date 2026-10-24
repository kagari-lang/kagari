use crate::engine::KagariEngine;
use kagari_source::source::SourceFile;
use rstest::rstest;

#[rstest]
#[case::array_free_len("std::array::len([1]);")]
#[case::set_free_contains("std::set::contains(LinkedHashSet::from([1]), 1);")]
#[case::array_join_receiver("[1].join(\",\");")]
#[case::readonly_vec_push("val a: List<i32> = Vec::from([1]); Vec::push(a, 2);")]
fn inherent_native_declarations_enforce_receiver_shapes_and_remove_old_exports(#[case] body: &str) {
    let engine = KagariEngine::default();
    assert!(
        engine
            .compile_to_artifact(
                SourceFile::new(
                    "removed-standard-api.kgr",
                    format!("fn main() {{ {body} }}")
                ),
                Default::default(),
            )
            .is_err(),
        "{body}"
    );
}
