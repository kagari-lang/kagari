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

fn check(source: &str, entries: &[String], traps: bool) {
    println!("numeric source: {source}");
    let source_program =
        compile_test_bytecode(&format!("{source}\nfn numeric_ready() -> bool {{ true }}"));
    let artifact = KbcArtifact::from_program(source_program.clone(), Default::default()).unwrap();
    let decoded = KbcArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
    decoded.validate_for_loader(&Default::default()).unwrap();
    // These fixtures only use scalar locals. Install/load once per artifact
    // route; the successful probe after every entry checks independent calls
    // still work after traps. The two routes retain separate heaps and roots.
    for (route, program) in [("direct", source_program), ("decoded", decoded.program)] {
        let mut runtime = standard_runtime(Default::default());
        let loaded = runtime.load_program("numeric", program).unwrap();
        let vm = Vm::new(runtime);
        for entry in entries {
            let result = vm.execute(&loaded, entry);
            if traps {
                assert!(
                    matches!(result, Err(VmError::RuntimeError(ref error)) if error.kind() == RuntimeErrorKind::ScriptTrap),
                    "{entry} ({route}): {result:?}",
                );
            } else {
                let result = result.unwrap();
                assert_eq!(
                    result.return_value.value(vm.runtime().gc()).unwrap(),
                    Value::Bool(true),
                    "{entry} ({route})",
                );
            }
            let ready = vm
                .execute(&loaded, "numeric_ready")
                .unwrap_or_else(|error| panic!("probe after {entry} ({route}): {error:?}"));
            assert_eq!(
                ready.return_value.value(vm.runtime().gc()).unwrap(),
                Value::Bool(true),
                "probe after {entry} ({route})",
            );
        }
    }
}

#[test]
fn all_integer_widths_cover_arithmetic_bits_comparisons_and_concrete_generics() {
    let mut source =
        String::from("use std::ops::Add; fn plus<T: Add<T>>(a: T, b: T) -> T::Output { a + b }\n");
    let mut entries = Vec::new();
    for (ty, _, signed) in INTEGERS {
        let negation = if signed {
            format!("&& -b == -3{ty}")
        } else {
            String::new()
        };
        source.push_str(&format!(
            "fn calc_{ty}(a: {ty}, b: {ty}) -> bool {{
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
                 fn main_{ty}() -> bool {{ calc_{ty}(6{ty}, 3{ty}) }}\n"
        ));
        entries.push(format!("main_{ty}"));
    }
    check(&source, &entries, false);
}

#[test]
fn integer_overflow_and_zero_divisors_trap_at_every_source_width() {
    let mut source = String::new();
    let mut entries = Vec::new();
    for (ty, bits, signed) in INTEGERS {
        let max = if signed {
            (1i128 << (bits - 1)) - 1
        } else {
            (1i128 << bits) - 1
        };
        for (name, op, left, right) in [
            ("add", "+", max, 1),
            ("multiply", "*", max, 2),
            ("divide", "/", 6, 0),
            ("remainder", "%", 6, 0),
            ("shift", "<<", 1, i128::from(bits)),
        ] {
            source.push_str(&format!(
                "fn calc_{ty}_{name}(a: {ty}, b: {ty}) -> {ty} {{ a {op} b }}
                 fn main_{ty}_{name}() -> bool {{ calc_{ty}_{name}({left}{ty}, {right}{ty}) == 0{ty} }}\n"
            ));
            entries.push(format!("main_{ty}_{name}"));
        }
        if signed {
            let min = -(1i128 << (bits - 1));
            for (name, body) in [
                ("divide", "a / b"),
                ("remainder", "a % b"),
                ("negate", "-a"),
            ] {
                source.push_str(&format!(
                    "fn calc_min_{ty}_{name}(a: {ty}, b: {ty}) -> {ty} {{ {body} }}
                     fn main_min_{ty}_{name}() -> bool {{ calc_min_{ty}_{name}({min}{ty}, -1{ty}) == 0{ty} }}\n"
                ));
                entries.push(format!("main_min_{ty}_{name}"));
            }
        }
    }
    check(&source, &entries, true);
}

#[test]
fn floats_use_their_source_precision_and_ieee_comparisons() {
    let mut source = String::new();
    let mut entries = Vec::new();
    for ty in ["f32", "f64"] {
        source.push_str(&format!(
            "fn calc_{ty}(a: {ty}, b: {ty}) -> bool {{
                    val nan = 0.0{ty} / 0.0{ty};
                    a + b == 9.0{ty} && a - b == 3.0{ty}
                    && a * b == 18.0{ty} && a / b == 2.0{ty} && a % b == 0.0{ty}
                    && -b == -3.0{ty} && a > b && b <= a
                    && nan != nan && !(nan < a) && -0.0{ty} == 0.0{ty}
                    && (a as i32) == 6
                }}
                fn main_{ty}() -> bool {{ calc_{ty}(6.0{ty}, 3.0{ty}) }}\n"
        ));
        entries.push(format!("main_{ty}"));
    }
    check(&source, &entries, false);
}

#[test]
fn numeric_conversions_preserve_all_supported_domains() {
    let mut source = String::new();
    let mut entries = Vec::new();
    let types = INTEGERS
        .iter()
        .map(|(ty, _, _)| *ty)
        .chain(["f32", "f64"])
        .collect::<Vec<_>>();
    for ty in &types {
        let input = if ty.starts_with('f') { "6.0" } else { "6" };
        let checks = types
            .iter()
            .map(|target| {
                let output = if target.starts_with('f') { "6.0" } else { "6" };
                format!("(a as {target}) == {output}{target}")
            })
            .collect::<Vec<_>>()
            .join(" && ");
        source.push_str(&format!(
            "fn calc_{ty}(a: {ty}) -> bool {{ {checks} }}
             fn main_{ty}() -> bool {{ calc_{ty}({input}{ty}) }}\n"
        ));
        entries.push(format!("main_{ty}"));
    }
    let checks = INTEGERS
        .iter()
        .map(|(ty, _, _)| format!("(a as {ty}) == 1{ty} && (b as {ty}) == 0{ty}"))
        .collect::<Vec<_>>()
        .join(" && ");
    source.push_str(&format!(
        "fn calc_bool(a: bool, b: bool) -> bool {{ {checks} }}
         fn main_bool() -> bool {{ calc_bool(true, false) }}"
    ));
    entries.push("main_bool".into());
    check(&source, &entries, false);
}
