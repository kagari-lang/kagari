use kagari_embed::engine::KagariEngine;
use kagari_source::source::SourceFile;

#[test]
fn inherent_native_declarations_enforce_receiver_shapes_and_remove_old_exports() {
    let engine = KagariEngine::default();
    for body in [
        "std::array::len([1]);",
        "std::set::contains(LinkedHashSet::from([1]), 1);",
        "[1].join(\",\");",
        "val a: List<i32> = [1]; Vec::push(a, 2);",
    ] {
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
}
