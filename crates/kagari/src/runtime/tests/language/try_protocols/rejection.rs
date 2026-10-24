use crate::{engine::KagariEngine, error::EmbeddingError};
use kagari_source::source::SourceFile;

#[test]
fn invalid_protocols_and_residuals_fail_during_analysis() {
    let prefix = "use core::ops::{Try,FromResidual,ControlFlow}; use core::convert::Infallible; ";
    for (source, code) in [
        (
            "#[lang = \"try\"] pub trait Counterfeit { type Output; type Residual; }",
            "KG_LANGUAGE_ROLE",
        ),
        (
            "struct Carrier {} impl Try for Carrier { type Output = i32; type Residual = Option<Infallible>; fn from_output(value:i32)->Self {Carrier{}} fn branch(self)->ControlFlow<Option<Infallible>,i32> {ControlFlow::Continue(42)} }",
            "KG_TYPE_INVALID_TRAIT_IMPL",
        ),
        (
            "struct Carrier {} impl FromResidual<Option<Infallible>> for Carrier { fn from_residual(value:Option<Infallible>)->Self {Carrier{}} } impl Try for Carrier { type Output = i32; type Residual = Option<Infallible>; fn from_output(value:i32)->Self {Carrier{}} fn branch(self)->ControlFlow<Option<Infallible>,bool> {ControlFlow::Continue(true)} }",
            "KG_TYPE_TRAIT_METHOD_MISMATCH",
        ),
        (
            "fn forward(value:ControlFlow<String,i32>)->Option<i32> {Some(value?)}",
            "KG_TYPE_GENERIC_BOUND_NOT_SATISFIED",
        ),
        (
            "fn forward<A: Try, R>(value:A)->R { value?; loop {} }",
            "KG_TYPE_GENERIC_BOUND_NOT_SATISFIED",
        ),
        (
            "struct Carrier {} impl FromResidual<Option<Infallible>> for Carrier { fn from_residual(value:Option<Infallible>)->Self {Carrier{}} } impl FromResidual<Option<Infallible>> for Carrier { fn from_residual(value:Option<Infallible>)->Self {Carrier{}} }",
            "KG_TYPE_INVALID_TRAIT_IMPL",
        ),
    ] {
        let error = KagariEngine::default()
            .compile_to_artifact(
                SourceFile::new("memory://invalid-try.kgr", format!("{prefix}{source}")),
                Default::default(),
            )
            .unwrap_err();
        let EmbeddingError::Diagnostics { diagnostics } = error else {
            panic!("{error:?}")
        };
        assert!(
            diagnostics.iter().any(|d| d.code == code),
            "{source}: {diagnostics:?}"
        );
    }
}
