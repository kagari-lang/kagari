use crate::{engine::KagariEngine, error::EmbeddingError};
use kagari_source::source::SourceFile;

#[test]
fn propagation_requires_one_infallible_conversion_bound() {
    for source in [
        "fn forward<T,E,F>(value:Result<T,E>)->Result<T,F>{Ok(value?)} fn main(){}",
        "struct E{} struct F{} impl F {fn convert(e:E)->Result<Self,String>{Ok(F{})}} fn main()->Result<i32,F>{val x:Result<i32,E> = Err(E{});Ok(x?)}",
        "struct E{} struct Mid{} struct F{} impl From<E> for Mid {fn from(e:E)->Self{Mid{}}} impl From<Mid> for F {fn from(e:Mid)->Self{F{}}} fn main()->Result<i32,F>{val x:Result<i32,E> = Err(E{});Ok(x?)}",
    ] {
        let error = KagariEngine::default()
            .compile_source(SourceFile::new("missing-error-conversion.kgr", source))
            .unwrap_err();
        assert!(
            format!("{error:?}").contains("KG_TYPE_GENERIC_BOUND_NOT_SATISFIED"),
            "{error:?}"
        );
    }
}

#[test]
fn invalid_propagation_constructors_and_callbacks_are_diagnosed() {
    for (source, code) in [
        (
            "fn f()->i32 { val x: Option<i32> = None; x? }",
            "KG_TYPE_GENERIC_BOUND_NOT_SATISFIED",
        ),
        (
            "fn f()->Result<i32, String> { val x: Option<i32> = None; Ok(x?) }",
            "KG_TYPE_GENERIC_BOUND_NOT_SATISFIED",
        ),
        (
            "fn f()->Option<i32> { val x: Result<i32, String> = Err(\"x\"); Some(x?) }",
            "KG_TYPE_GENERIC_BOUND_NOT_SATISFIED",
        ),
        (
            "fn f()->Result<i32, String> { val x: Result<i32, i32> = Err(1); Ok(x?) }",
            "KG_TYPE_GENERIC_BOUND_NOT_SATISFIED",
        ),
        (
            "fn f()->Option<i32> { Some(42?) }",
            "KG_TYPE_GENERIC_BOUND_NOT_SATISFIED",
        ),
        (
            "fn f()->Option<i32> { Some(\"bad\") }",
            "KG_TYPE_ARGUMENT_TYPE_MISMATCH",
        ),
        (
            "fn f()->i32 { val x = None; 42 }",
            "KG_TYPE_CANNOT_INFER_GENERIC_ARGUMENT",
        ),
        (
            "fn f()->Option<i32> { None() }",
            "KG_TYPE_INVALID_CALL_TARGET",
        ),
        (
            "fn f()->i32 { val x: Option<i32> = Some(1); match x { Some => 42, _ => 0, } }",
            "KG_TYPE_PATTERN_MISMATCH",
        ),
        (
            "fn f()->Option<i32> { val cb = || { val x: Option<i32> = None; x?; 42 }; Some(cb()) }",
            "KG_TYPE_GENERIC_BOUND_NOT_SATISFIED",
        ),
    ] {
        let error = KagariEngine::default()
            .compile_to_artifact(SourceFile::new("invalid.kgr", source), Default::default())
            .unwrap_err();
        let EmbeddingError::Diagnostics { diagnostics } = error else {
            panic!("{source}: {error:?}");
        };
        assert!(
            diagnostics.iter().any(|d| d.code == code),
            "{source}: expected {code}, got {diagnostics:?}"
        );
    }
}
