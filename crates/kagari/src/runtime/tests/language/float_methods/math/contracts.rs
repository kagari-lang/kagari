use crate::runtime::tests::language::float_methods::{FLOATS, Float, engine};
use kagari_source::source::SourceFile;
use rstest::rstest;

#[rstest]
#[case::f32(FLOATS[0])]
#[case::f64(FLOATS[1])]
fn math_signatures_reject_wrong_arity_exponent_types_and_result_widths(#[case] float: Float) {
    let compiler = engine();
    let mut invalid = Vec::new();
    let ty = float.ty;
    let other = if ty == "f32" { "f64" } else { "f32" };
    let value = format!("1.0{ty}");
    for method in ["sin", "cos", "tan", "asin", "acos", "atan", "exp", "ln"] {
        invalid.push(format!("val result = ({value}).{method}({value});"));
        invalid.push(format!("val result = {ty}::{method}();"));
        invalid.push(format!("val result: {other} = ({value}).{method}();"));
    }
    for (method, argument) in [
        ("atan2", value.clone()),
        ("powf", value.clone()),
        ("powi", "1i32".into()),
    ] {
        invalid.push(format!("val result = ({value}).{method}();"));
        invalid.push(format!(
            "val result = ({value}).{method}({argument}, {argument});"
        ));
        invalid.push(format!(
            "val result: {other} = ({value}).{method}({argument});"
        ));
        invalid.push(format!("val result = {ty}::{method}({argument});"));
    }
    for method in ["atan2", "powf"] {
        for wrong in [format!("1.0{other}"), "1i32".into(), "true".into()] {
            invalid.push(format!("val result = ({value}).{method}({wrong});"));
        }
    }
    // powi's i32 exponent is independent of receiver width. In particular,
    // accepting a numeric cast here would incorrectly admit float or u32.
    for wrong in ["1.0f32", "1.0f64", "1u32", "1i64", "1u64", "true"] {
        invalid.push(format!("val result = ({value}).powi({wrong});"));
    }
    for statement in invalid {
        assert!(
            compiler
                .compile_to_artifact(
                    SourceFile::new(
                        "invalid-float-math.kgr",
                        format!("fn main() {{ {statement} }}")
                    ),
                    Default::default(),
                )
                .is_err(),
            "unexpectedly accepted {statement}"
        );
    }
}
