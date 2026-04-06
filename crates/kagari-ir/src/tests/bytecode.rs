use crate::{
    bytecode::{BinaryOp, BytecodeInstruction, CallTarget, FunctionRef, RuntimeHelper, UnaryOp},
    tests::common,
};

#[test]
fn lowers_function_metadata_into_bytecode() {
    let bytecode = common::bytecode_ok("fn add(a: i32, b: i32) -> i32 { let c = a + b; c }");
    let function = &bytecode.functions[0];

    assert_eq!(function.id, FunctionRef::new(0));
    assert_eq!(function.name, "add");
    assert_eq!(function.parameter_count, 2);
    assert_eq!(function.local_count, 3);
    assert!(function.register_count >= 4);
}

#[test]
fn lowers_arithmetic_into_real_bytecode_instructions() {
    let bytecode = common::bytecode_ok("fn add(a: i32, b: i32) -> i32 { let c = a + b; c }");
    let function = &bytecode.functions[0];

    assert!(function.instructions.iter().any(|instruction| matches!(
        instruction,
        BytecodeInstruction::Binary {
            op: BinaryOp::Add,
            ..
        }
    )));
}

#[test]
fn flattens_branch_targets_to_instruction_offsets() {
    let bytecode = common::bytecode_ok("fn main() -> i32 { if true { 1 } else { 2 } }");
    let function = &bytecode.functions[0];

    let targets = function
        .instructions
        .iter()
        .filter_map(|instruction| match instruction {
            BytecodeInstruction::Branch {
                then_target,
                else_target,
                ..
            } => Some([then_target.index(), else_target.index()]),
            BytecodeInstruction::Jump { target } => Some([target.index(), target.index()]),
            _ => None,
        })
        .flatten()
        .collect::<Vec<_>>();

    assert!(!targets.is_empty());
    assert!(
        targets
            .iter()
            .all(|target| *target < function.instructions.len())
    );
}

#[test]
fn lowers_direct_function_calls_to_function_refs() {
    let bytecode = common::bytecode_ok(
        r#"
fn callee() -> i32 { 1 }
fn caller() -> i32 { callee() }
"#,
    );
    let function = &bytecode.functions[1];

    assert!(function.instructions.iter().any(|instruction| matches!(
        instruction,
        BytecodeInstruction::Call {
            callee: CallTarget::Function(_),
            ..
        }
    )));
}

#[test]
fn lowers_unary_and_short_circuit_expressions() {
    let bytecode = common::bytecode_ok("fn main() -> bool { !false && true }");
    let function = &bytecode.functions[0];

    assert!(function.instructions.iter().any(|instruction| matches!(
        instruction,
        BytecodeInstruction::Unary {
            op: UnaryOp::Not,
            ..
        }
    )));

    let branch_count = function
        .instructions
        .iter()
        .filter(|instruction| matches!(instruction, BytecodeInstruction::Branch { .. }))
        .count();
    assert!(branch_count >= 1);
}

#[test]
fn lowers_loops_and_loop_control_to_jumps() {
    let bytecode = common::bytecode_ok(
        r#"
fn main() -> unit {
    while true { break; }
    loop { continue; }
}
"#,
    );
    let function = &bytecode.functions[0];

    let jump_count = function
        .instructions
        .iter()
        .filter(|instruction| matches!(instruction, BytecodeInstruction::Jump { .. }))
        .count();
    assert!(jump_count >= 3);

    assert!(
        function
            .instructions
            .iter()
            .any(|instruction| matches!(instruction, BytecodeInstruction::Branch { .. }))
    );
}

#[test]
fn lowers_aggregate_and_access_instructions() {
    let bytecode = common::bytecode_ok(
        r#"
struct Point { x: i32 }

fn main() -> unit {
    let tuple = (1, 2);
    let array = [1, 2];
    let point = Point { x: 1 };
    tuple;
    array[0];
    point.x;
}
"#,
    );
    let function = &bytecode.functions[0];

    assert!(
        function
            .instructions
            .iter()
            .any(|instruction| matches!(instruction, BytecodeInstruction::MakeTuple { .. }))
    );
    assert!(
        function
            .instructions
            .iter()
            .any(|instruction| matches!(instruction, BytecodeInstruction::MakeArray { .. }))
    );
    assert!(
        function
            .instructions
            .iter()
            .any(|instruction| matches!(instruction, BytecodeInstruction::MakeStruct { .. }))
    );
    assert!(
        function
            .instructions
            .iter()
            .any(|instruction| matches!(instruction, BytecodeInstruction::ReadIndex { .. }))
    );
    assert!(
        function
            .instructions
            .iter()
            .any(|instruction| matches!(instruction, BytecodeInstruction::ReadField { .. }))
    );
}

#[test]
fn lowers_named_match_pattern_to_local_traffic() {
    let bytecode =
        common::bytecode_ok("fn main(value: i32) -> i32 { match value { bound => bound } }");
    let function = &bytecode.functions[0];

    assert!(
        function
            .instructions
            .iter()
            .any(|instruction| matches!(instruction, BytecodeInstruction::StoreLocal { .. }))
    );
    assert!(
        function
            .instructions
            .iter()
            .any(|instruction| matches!(instruction, BytecodeInstruction::LoadLocal { .. }))
    );
}

#[test]
fn lowers_type_of_builtin_to_runtime_helper_call() {
    let bytecode = common::bytecode_ok("fn main() -> str { type_of(7) }");
    let function = &bytecode.functions[0];

    assert!(function.instructions.iter().any(|instruction| matches!(
        instruction,
        BytecodeInstruction::Call {
            callee: CallTarget::RuntimeHelper(RuntimeHelper::ReflectTypeOf),
            ..
        }
    )));
}

#[test]
fn lowers_reflection_field_builtins_to_runtime_helper_calls() {
    let bytecode = common::bytecode_ok(
        r#"
struct Point { x: i32 }

fn main() -> Point {
    let point = Point { x: 1 };
    let next = set_field(point, "x", 9);
    get_field(next, "x");
    next
}
"#,
    );
    let function = &bytecode.functions[0];

    assert!(function.instructions.iter().any(|instruction| matches!(
        instruction,
        BytecodeInstruction::Call {
            callee: CallTarget::RuntimeHelper(RuntimeHelper::ReflectSetField(field)),
            ..
        } if field == "x"
    )));
    assert!(function.instructions.iter().any(|instruction| matches!(
        instruction,
        BytecodeInstruction::Call {
            callee: CallTarget::RuntimeHelper(RuntimeHelper::ReflectGetField(field)),
            ..
        } if field == "x"
    )));
}

#[test]
fn lowers_set_index_builtin_to_runtime_helper_call() {
    let bytecode = common::bytecode_ok(
        r#"
fn main(values: [i32]) -> [i32] {
    set_index(values, 0, 9)
}
"#,
    );
    let function = &bytecode.functions[0];

    assert!(function.instructions.iter().any(|instruction| matches!(
        instruction,
        BytecodeInstruction::Call {
            callee: CallTarget::RuntimeHelper(RuntimeHelper::ReflectSetIndex),
            ..
        }
    )));
}

#[test]
fn lowers_place_assignments_to_reflection_helpers() {
    let bytecode = common::bytecode_ok(
        r#"
struct Point { x: i32 }
struct Holder { inner: Point }

fn main() -> i32 {
    let mut holder = Holder { inner: Point { x: 1 } };
    holder.inner.x = 7;
    let mut values = [1, 2];
    values[0] = 5;
    holder.inner.x + values[0]
}
"#,
    );
    let function = &bytecode.functions[0];

    assert!(function.instructions.iter().any(|instruction| matches!(
        instruction,
        BytecodeInstruction::Call {
            callee: CallTarget::RuntimeHelper(RuntimeHelper::ReflectSetField(field)),
            ..
        } if field == "x"
    )));
    assert!(function.instructions.iter().any(|instruction| matches!(
        instruction,
        BytecodeInstruction::Call {
            callee: CallTarget::RuntimeHelper(RuntimeHelper::ReflectSetIndex),
            ..
        }
    )));
}

#[test]
fn preserves_module_init_function_metadata_in_bytecode() {
    let bytecode = common::bytecode_ok(
        r#"
let boot = 1;

fn main() -> i32 { 1 }
"#,
    );

    assert!(bytecode.module_init.is_some());
}

#[test]
fn does_not_allocate_module_slots_for_const_items() {
    let bytecode = common::bytecode_ok(
        r#"
const BASE: i32 = 1;
const VALUE: i32 = BASE + 2;

fn main() -> i32 { VALUE }
"#,
    );
    let function = bytecode
        .functions
        .iter()
        .find(|function| function.name == "main")
        .expect("expected main function");

    assert!(bytecode.module_slots.is_empty());
    assert!(
        function
            .instructions
            .iter()
            .any(|instruction| matches!(instruction, BytecodeInstruction::LoadConst { .. }))
    );
    assert!(
        !function
            .instructions
            .iter()
            .any(|instruction| matches!(instruction, BytecodeInstruction::LoadModule { .. }))
    );
}
