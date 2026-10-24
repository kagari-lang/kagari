use crate::engine::KagariEngine;
use kagari_source::source::SourceFile;

#[test]
fn conversions_reject_implicit_loss_and_invalid_cast_targets() {
    let engine = KagariEngine::default();
    for expr in [
        "1u8 as bool",
        "true as f64",
        "1u8 as String",
        "1u8 as Missing",
    ] {
        let result = engine.compile_to_artifact(
            SourceFile::new(
                "invalid-cast.kgr",
                format!("fn main() {{ val value = {expr}; }}"),
            ),
            Default::default(),
        );
        assert!(result.is_err(), "unexpectedly accepted {expr}");
    }
}

#[test]
fn narrow_remainder_is_rejected_in_constants() {
    let engine = KagariEngine::default();
    assert!(
        engine
            .compile_to_artifact(
                SourceFile::new(
                    "checked-const.kgr",
                    "const VALUE: i8 = -128i8 % -1i8; fn main() -> i8 { VALUE }"
                ),
                Default::default()
            )
            .is_err()
    );
}
