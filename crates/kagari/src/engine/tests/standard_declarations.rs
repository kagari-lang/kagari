use crate::engine::KagariEngine;
use crate::test_support::standard::standard_library;
use kagari_source::source::SourceFile;
use rstest::rstest;

#[rstest]
#[case::array_free_len("std::array::len([1]);")]
#[case::set_free_contains("std::set::contains(LinkedHashSet::from([1]), 1);")]
#[case::array_join_receiver("[1].join(\",\");")]
fn inherent_native_declarations_enforce_receiver_shapes_and_remove_old_exports(#[case] body: &str) {
    let engine = KagariEngine::with_native_modules(
        Default::default(),
        standard_library().modules().unwrap(),
    )
    .unwrap();
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
