//! Numeric source forms and source-free execution share the language contract.
use crate::tests::common::round_trip_bytecode;
use crate::{
    error::VmError,
    executor::Executor,
    tests::common::{compile_test_bytecode, compile_test_bytecode_with_options, standard_runtime},
    vm::Vm,
};

use kagari_bytecode::instruction::BytecodeInstruction;
use kagari_compiler::source::lower::instances::MirLoweringOptions;
use kagari_runtime::{
    Runtime,
    error::{RuntimeError, RuntimeErrorKind},
    frame::ExecutionFrame,
    session::{ExecutionEvent, ExecutionObserver},
    value::Value,
};
use std::sync::{Arc, Mutex};

#[test]
fn rotated_loops_preserve_entry_control_flow_and_condition_effects() {
    let source = "fn scan(n: i32) -> i32 { var i = 0; var sum = 0;
         while i < n { i += 1; if i == 2 { continue; } if i == 5 { break; } sum += i; }
         i * 100 + sum }
         pub fn guards() -> bool { scan(0) == 0 && scan(1) == 101 && scan(4) == 408 && scan(8) == 508 }
         fn effects(n: i32) -> i32 { var checks = 0; var steps = 0;
         while { checks += 1; steps < n } { steps += 1; } checks * 10 + steps }
         pub fn effectful() -> bool { effects(0) == 10 && effects(3) == 43 }
         pub fn nested() -> bool { var i = 0; var total = 0;
         while i < 2 { var j = 0; while j < 3 { total += 1; j += 1; } i += 1; } total == 6 }";
    for optimization in [None, Some(Default::default())] {
        check_with_options(
            source,
            &["guards".into(), "effectful".into(), "nested".into()],
            false,
            &MirLoweringOptions {
                optimization,
                ..Default::default()
            },
        );
    }
}

#[test]
fn local_reads_are_snapshots_across_later_writes_and_captured_calls() {
    check(
        "pub fn later_write() -> bool { var x = 7; x = x + { x = 40; 2 }; x == 9 }
         pub fn captured_call() -> bool { var x = 7; val f = || { x = 40; 2 }; x = x + f(); x == 9 }
         fn saved_rhs(input: i32) -> bool { var x = input; val saved = x * 3;
             x = saved; x += 1; saved == 21 && x == 22 }
         pub fn overwritten_rhs() -> bool { saved_rhs(7) }",
        &[
            "later_write".into(),
            "captured_call".into(),
            "overwritten_rhs".into(),
        ],
        false,
    );
}

#[test]
fn control_predicates_preserve_nan_polarity_and_observable_bools() {
    let mut source = String::new();
    let mut entries = Vec::new();
    for ty in ["f32", "f64"] {
        for (name, op, expected) in [
            ("eq", "==", false),
            ("ne", "!=", true),
            ("lt", "<", false),
            ("le", "<=", false),
            ("gt", ">", false),
            ("ge", ">=", false),
        ] {
            source.push_str(&format!(
                "fn case_{name}_{ty}(n: {ty}) -> bool {{
                val value = n {op} 0.0{ty}; var control = false; var inverse = false;
                if n {op} 0.0{ty} {{ control = true; }}
                if !(n {op} 0.0{ty}) {{ inverse = true; }}
                value == {expected} && control == {expected} && inverse == {}
            }} pub fn main_{name}_{ty}() -> bool {{ case_{name}_{ty}(0.0{ty} / 0.0{ty}) }}\n",
                !expected
            ));
            entries.push(format!("main_{name}_{ty}"));
        }
    }
    source.push_str("pub fn managed_predicates() -> bool { val a = [1]; val same = a; val other = [1]; val label = \"ok\"; var selected = 0; if a === same { selected += 1; } if !(a === other) { selected += 2; } if label == \"ok\" { selected += 4; } selected == 7 }");
    entries.push("managed_predicates".into());
    check(&source, &entries, false);
}

#[test]
fn mixed_constant_sides_keep_nonloop_arithmetic_effects_and_boundaries() {
    let mut source = String::from(
        "fn bound(n: i32) -> i32 { var total = 0; var i = 0; while i < n { total += i; i += 1; } total }\n",
    );
    let mut entries = Vec::new();
    for (ty, _, _) in INTEGERS {
        source.push_str(&format!(
            "fn mixed_{ty}(x: {ty}) -> bool {{
            var value = x; value = 20{ty} - value;
            value == 16{ty} && 20{ty} / x == 5{ty} && x / 2{ty} == 2{ty}
                && (3{ty} << 1u32) == 6{ty} && (x << 1u32) == 8{ty}
        }} pub fn main_{ty}() -> bool {{ mixed_{ty}(4{ty}) }}\n"
        ));
        entries.push(format!("main_{ty}"));
    }
    source.push_str("pub fn effects() -> bool { var calls = 0; val f = || { calls += 1; 3 }; val a = 20 - f(); val b = f() - 2; a == 17 && b == 1 && calls == 2 && bound(0) == 0 && bound(-2) == 0 && bound(3) == 3 }");
    entries.push("effects".into());
    check(&source, &entries, false);
}

#[derive(Debug)]
struct CommitObserver(Arc<Mutex<Vec<(Value, Value)>>>);

impl ExecutionObserver for CommitObserver {
    fn observe(
        &mut self,
        runtime: &Runtime,
        event: ExecutionEvent,
        frames: &[ExecutionFrame],
    ) -> Result<(), RuntimeError> {
        if event != ExecutionEvent::Trap {
            return Ok(());
        }
        let frame = frames.last().unwrap();
        let function = frame.function().unwrap();
        let read = |name| {
            let binding = function
                .metadata
                .debug
                .local_live_ranges
                .iter()
                .find(|range| {
                    range.name == name
                        && range.start <= frame.instruction_offset()
                        && frame.instruction_offset() < range.end
                })
                .unwrap();
            frame.read_slot(runtime, binding.slot).unwrap()
        };
        self.0.lock().unwrap().push((read("s"), read("i")));
        Ok(())
    }
}

#[test]
fn direct_local_overflow_keeps_the_old_destination_and_prior_commit() {
    let direct = compile_test_bytecode(
        "pub fn main() { var s = 0i8; var i = 127i8; s = s + 1i8; i = i + 1i8; }",
    );
    assert!(
        direct
            .records()
            .modules
            .iter()
            .flat_map(|module| &module.functions)
            .flat_map(|function| &function.instructions)
            .any(|instruction| matches!(instruction, BytecodeInstruction::BinaryOperands { .. })),
        "failed-commit coverage must execute selected constant operands"
    );
    let decoded = round_trip_bytecode(direct.clone());
    for program in [direct, decoded] {
        let states = Arc::new(Mutex::new(Vec::new()));
        let mut runtime = standard_runtime(Default::default());
        runtime
            .set_execution_observer(CommitObserver(states.clone()))
            .unwrap();
        let loaded = runtime.load_program("local-commit", program).unwrap();
        let _loaded_program = loaded;
        let loaded = runtime.select_program(&_loaded_program).unwrap();
        let vm = Vm::new(runtime);
        let session = vm
            .runtime()
            .begin_execution(&loaded, Default::default())
            .unwrap();
        assert!(vm.runtime().attach_execution_observer().unwrap());
        let error = vm.execute(&loaded, "main").unwrap_err();
        assert!(
            matches!(error, VmError::RuntimeError(ref error) if error.kind() == RuntimeErrorKind::ScriptTrap)
        );
        assert_eq!(*states.lock().unwrap(), [(Value::I32(1), Value::I32(127))]);
        drop(session);
        assert_eq!(vm.runtime().resources().counters().current_call_depth, 0);
        assert_eq!(vm.runtime().gc().active_roots(), 0);
    }
}

const INTEGERS: [(&str, u32, bool); 8] = [
    ("i8", 8, true),
    ("i16", 16, true),
    ("i32", 32, true),
    ("i64", 64, true),
    ("u8", 8, false),
    ("u16", 16, false),
    ("u32", 32, false),
    ("u64", 64, false),
];

fn check(source: &str, entries: &[String], traps: bool) {
    check_with_options(source, entries, traps, &Default::default());
}

fn check_with_options(source: &str, entries: &[String], traps: bool, options: &MirLoweringOptions) {
    println!("numeric source: {source}");
    let source_program = compile_test_bytecode_with_options(
        &format!("{source}\npub fn numeric_ready() -> bool {{ true }}"),
        options,
    );
    let decoded = round_trip_bytecode(source_program.clone());
    // These fixtures use scalar locals and fields. Install/load once per artifact
    // route; the successful probe after every entry checks independent calls
    // still work after traps. The two routes retain separate heaps and roots.
    for (route, program) in [("direct", source_program), ("decoded", decoded)] {
        let mut runtime = standard_runtime(Default::default());
        let loaded = runtime.load_program("numeric", program).unwrap();
        let _loaded_program = loaded;
        let loaded = runtime.select_program(&_loaded_program).unwrap();
        let vm = Vm::new(runtime);
        for entry in entries {
            let result = vm.execute(&loaded, entry.as_str());
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
                     && 9{ty} - a == 3{ty} && 12{ty} / a == 2{ty} && 7{ty} % a == 1{ty}
                     && (15{ty} & a) == 6{ty} && (1{ty} << b) == 8{ty}
                     && (a & b) == 2{ty} && (a | b) == 7{ty} && (a ^ b) == 5{ty}
                     && (a << 1u32) == 12{ty} && (a >> 1u32) == 3{ty}
                     && a > b && a >= b && b < a && b <= a && a != b
                     && plus(a, b) == 9{ty} && (a as f64) == 6.0f64
                     {negation}
                 }}
                 pub fn main_{ty}() -> bool {{ calc_{ty}(6{ty}, 3{ty}) }}\n"
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
                 pub fn main_{ty}_{name}() -> bool {{ calc_{ty}_{name}({left}{ty}, {right}{ty}) == 0{ty} }}\n"
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
                     pub fn main_min_{ty}_{name}() -> bool {{ calc_min_{ty}_{name}({min}{ty}, -1{ty}) == 0{ty} }}\n"
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
                    && 9.0{ty} - a == 3.0{ty} && 12.0{ty} / a == 2.0{ty}
                    && 7.0{ty} % a == 1.0{ty} && 3.0{ty} < a
                    && -b == -3.0{ty} && a > b && b <= a
                    && nan != nan && !(nan < a) && -0.0{ty} == 0.0{ty}
                    && (a as i32) == 6
                }}
                pub fn main_{ty}() -> bool {{ calc_{ty}(6.0{ty}, 3.0{ty}) }}\n"
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
             pub fn main_{ty}() -> bool {{ calc_{ty}({input}{ty}) }}\n"
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
         pub fn main_bool() -> bool {{ calc_bool(true, false) }}"
    ));
    entries.push("main_bool".into());
    check(&source, &entries, false);
}

#[test]
fn scalar_fields_transfer_all_checked_integer_domains() {
    let mut source = String::new();
    let mut entries = Vec::new();
    for (ty, width, signed) in INTEGERS {
        let min = if signed { -(1_i128 << (width - 1)) } else { 0 };
        let max = (1_i128 << (width - u32::from(signed))) - 1;
        let entry = format!("fields_{ty}");
        source.push_str(&format!(
            "struct S_{ty} {{ var before: bool, var value: {ty}, var after: bool }}
             pub fn {entry}() -> bool {{
                val p = S_{ty} {{ before: true, value: {min}{ty}, after: false }};
                val initial = p.value == {min}{ty};
                p.value = {max}{ty};
                initial && p.before && !p.after && p.value == {max}{ty}
             }}\n"
        ));
        entries.push(entry);
    }
    check(&source, &entries, false);
}

#[test]
fn scalar_field_overflow_preserves_prior_effects_and_does_not_commit() {
    let program = compile_test_bytecode(
        "struct State { var before: i32, var value: i8, var after: i32 }
         pub fn main() -> State { State { before: 0, value: 127i8, after: 0 } }
         fn update(s: State) { s.before += 1; s.value += 1i8; s.after += 1; }
         fn check(s: State) -> bool { s.before == 1 && s.value == 127i8 && s.after == 0 }",
    );
    let decoded = round_trip_bytecode(program.clone());
    for program in [program, decoded] {
        let mut runtime = standard_runtime(Default::default());
        let loaded = runtime.load_program("field-overflow", program).unwrap();
        let _loaded_program = loaded;
        let loaded = runtime.select_program(&_loaded_program).unwrap();
        let function = |name| {
            loaded
                .bytecode
                .functions
                .iter()
                .find(|f| f.name == name)
                .unwrap()
                .id
        };
        let vm = Vm::new(runtime);
        let kept = vm.execute(&loaded, "main").unwrap();
        let value = kept.return_value.value(vm.runtime().gc()).unwrap();
        let result = Executor::new(vm.runtime(), &loaded, function("update"), &[value])
            .unwrap()
            .run();
        assert!(
            matches!(result, Err(VmError::RuntimeError(error)) if error.kind() == RuntimeErrorKind::ScriptTrap)
        );
        vm.runtime().collect_garbage().unwrap();
        assert_eq!(
            Executor::new(vm.runtime(), &loaded, function("check"), &[value])
                .unwrap()
                .run()
                .unwrap(),
            Value::Bool(true)
        );
        assert_eq!(vm.runtime().resources().counters().current_call_depth, 0);
    }
}
