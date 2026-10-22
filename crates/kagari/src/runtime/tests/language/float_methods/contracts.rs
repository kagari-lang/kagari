use super::{FLOATS, engine};
use kagari_source::source::SourceFile;

#[test]
fn installed_signatures_reject_arity_type_width_and_dispatch_mismatches() {
    let compiler = engine();
    let mut invalid = Vec::new();
    for float in FLOATS {
        let ty = float.ty;
        let uint = float.uint;
        let (other, other_uint) = if ty == "f32" {
            ("f64", "u64")
        } else {
            ("f32", "u32")
        };
        let value = format!("1.0{ty}");
        for method in [
            "abs",
            "sqrt",
            "floor",
            "ceil",
            "round",
            "trunc",
            "fract",
            "is_nan",
            "is_infinite",
            "is_finite",
            "to_bits",
        ] {
            invalid.push(format!("val result = ({value}).{method}({value});"));
            invalid.push(format!("val result = {ty}::{method}();"));
            let wrong_result = match method {
                "is_nan" | "is_infinite" | "is_finite" => ty,
                "to_bits" => other_uint,
                _ => other,
            };
            invalid.push(format!(
                "val result: {wrong_result} = ({value}).{method}();"
            ));
        }
        for method in ["min", "max", "clamp"] {
            let args = if method == "clamp" {
                format!("{value}, {value}")
            } else {
                value.clone()
            };
            invalid.push(format!("val result = ({value}).{method}();"));
            invalid.push(format!("val result = ({value}).{method}({args}, {value});"));
            invalid.push(format!("val result: {other} = ({value}).{method}({args});"));
            for wrong in [format!("1.0{other}"), "1i32".into(), "true".into()] {
                if method == "clamp" {
                    invalid.push(format!("val result = ({value}).clamp({wrong}, {value});"));
                    invalid.push(format!("val result = ({value}).clamp({value}, {wrong});"));
                } else {
                    invalid.push(format!("val result = ({value}).{method}({wrong});"));
                }
            }
        }
        invalid.extend([
            format!("val result = ({value}).clamp({value});"),
            format!("val result = {ty}::from_bits();"),
            format!("val result = {ty}::from_bits(0{uint}, 0{uint});"),
            format!("val result = {ty}::from_bits(0{other_uint});"),
            format!("val result = {ty}::from_bits(0i32);"),
            format!("val result = {ty}::from_bits({value});"),
            format!("val result = {ty}::from_bits(true);"),
            format!("val result = ({value}).from_bits(0{uint});"),
            format!("val result: {other} = {ty}::from_bits(0{uint});"),
            format!("val result: i32 = ({value}).to_bits();"),
        ]);
    }
    for statement in invalid {
        assert!(
            compiler
                .compile_to_artifact(
                    SourceFile::new(
                        "invalid-float-method.kgr",
                        format!("fn main() {{ {statement} }}")
                    ),
                    Default::default(),
                )
                .is_err(),
            "unexpectedly accepted {statement}"
        );
    }
}
