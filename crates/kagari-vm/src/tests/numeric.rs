//! Numeric source forms and source-free execution share the language contract.
use crate::{
    error::VmError,
    tests::common::{compile_test_bytecode, standard_runtime},
    vm::Vm,
};
use kagari_bytecode::artifact::KbcArtifact;
use kagari_runtime::{error::RuntimeErrorKind, value::Value};

const INTEGERS: [(&str, u32, bool); 10] = [
    ("i8", 8, true),
    ("i16", 16, true),
    ("i32", 32, true),
    ("i64", 64, true),
    ("isize", 64, true),
    ("u8", 8, false),
    ("u16", 16, false),
    ("u32", 32, false),
    ("u64", 64, false),
    ("usize", 64, false),
];

fn check(source: &str, traps: bool) {
    println!("numeric source: {source}");
    let source_program = compile_test_bytecode(source);
    let artifact = KbcArtifact::from_program(source_program.clone(), Default::default()).unwrap();
    let decoded = KbcArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
    decoded.validate_for_loader(&Default::default()).unwrap();
    for program in [source_program, decoded.program] {
        let mut runtime = standard_runtime(Default::default());
        let loaded = runtime.load_program("numeric", program).unwrap();
        let vm = Vm::new(runtime);
        let result = vm.execute(&loaded, "main");
        if traps {
            assert!(
                matches!(result, Err(VmError::RuntimeError(ref error)) if error.kind() == RuntimeErrorKind::ScriptTrap),
                "{source}: {result:?}",
            );
        } else {
            let result = result.unwrap();
            assert_eq!(
                result.return_value.value(vm.runtime().gc()).unwrap(),
                Value::Bool(true),
                "{source}",
            );
        }
    }
}

#[test]
fn all_integer_widths_cover_arithmetic_bits_comparisons_and_concrete_generics() {
    for (ty, _, signed) in INTEGERS {
        let negation = if signed {
            format!("&& -b == -3{ty}")
        } else {
            String::new()
        };
        check(
            &format!(
                "use std::ops::Add;
                 fn plus<T: Add<T>>(a: T, b: T) -> T::Output {{ a + b }}
                 fn calc(a: {ty}, b: {ty}) -> bool {{
                     var x = a;
                     x = x + b;
                     x == 9{ty} && a + b == 9{ty} && a - b == 3{ty}
                     && a * b == 18{ty} && a / b == 2{ty} && a % b == 0{ty}
                     && (a & b) == 2{ty} && (a | b) == 7{ty} && (a ^ b) == 5{ty}
                     && (a << 1u32) == 12{ty} && (a >> 1u32) == 3{ty}
                     && a > b && a >= b && b < a && b <= a && a != b
                     && plus(a, b) == 9{ty} && (a as f64) == 6.0f64
                     {negation}
                 }}
                 fn main() -> bool {{ calc(6{ty}, 3{ty}) }}"
            ),
            false,
        );
    }
}

#[test]
fn integer_overflow_and_zero_divisors_trap_at_every_source_width() {
    for (ty, bits, signed) in INTEGERS {
        let max = if signed {
            (1i128 << (bits - 1)) - 1
        } else {
            (1i128 << bits) - 1
        };
        for (op, left, right) in [
            ("+", max, 1),
            ("*", max, 2),
            ("/", 6, 0),
            ("%", 6, 0),
            ("<<", 1, i128::from(bits)),
        ] {
            check(
                &format!(
                    "fn calc(a: {ty}, b: {ty}) -> {ty} {{ a {op} b }}
                     fn main() -> bool {{ calc({left}{ty}, {right}{ty}) == 0{ty} }}"
                ),
                true,
            );
        }
        if signed {
            let min = -(1i128 << (bits - 1));
            for body in ["a / b", "a % b", "-a"] {
                check(
                    &format!(
                        "fn calc(a: {ty}, b: {ty}) -> {ty} {{ {body} }}
                         fn main() -> bool {{ calc({min}{ty}, -1{ty}) == 0{ty} }}"
                    ),
                    true,
                );
            }
        }
    }
}

#[test]
fn floats_use_their_source_precision_and_ieee_comparisons() {
    for ty in ["f32", "f64"] {
        check(
            &format!(
                "fn calc(a: {ty}, b: {ty}) -> bool {{
                    val nan = 0.0{ty} / 0.0{ty};
                    a + b == 9.0{ty} && a - b == 3.0{ty}
                    && a * b == 18.0{ty} && a / b == 2.0{ty} && a % b == 0.0{ty}
                    && -b == -3.0{ty} && a > b && b <= a
                    && nan != nan && !(nan < a) && -0.0{ty} == 0.0{ty}
                    && (a as i32) == 6
                }}
                fn main() -> bool {{ calc(6.0{ty}, 3.0{ty}) }}"
            ),
            false,
        );
    }
}

#[test]
fn numeric_conversions_preserve_all_supported_domains() {
    let types = INTEGERS
        .iter()
        .map(|(ty, _, _)| *ty)
        .chain(["f32", "f64"])
        .collect::<Vec<_>>();
    for source in &types {
        let input = if source.starts_with('f') { "6.0" } else { "6" };
        let checks = types
            .iter()
            .map(|target| {
                let output = if target.starts_with('f') { "6.0" } else { "6" };
                format!("(a as {target}) == {output}{target}")
            })
            .collect::<Vec<_>>()
            .join(" && ");
        check(
            &format!(
                "fn calc(a: {source}) -> bool {{ {checks} }}
                 fn main() -> bool {{ calc({input}{source}) }}"
            ),
            false,
        );
    }
    let checks = INTEGERS
        .iter()
        .map(|(ty, _, _)| format!("(a as {ty}) == 1{ty} && (b as {ty}) == 0{ty}"))
        .collect::<Vec<_>>()
        .join(" && ");
    check(
        &format!(
            "fn calc(a: bool, b: bool) -> bool {{ {checks} }}
             fn main() -> bool {{ calc(true, false) }}"
        ),
        false,
    );
}
