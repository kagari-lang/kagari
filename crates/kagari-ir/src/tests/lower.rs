use crate::{
    builtin::array,
    lower_to_ir,
    module::instruction::RuntimeHelper,
    module::{BinaryOp, BuiltinMethod, CallTarget, Instruction, IrFunction, IrValue, Terminator},
    tests::common,
};

#[test]
fn lowers_function_into_cfg_shaped_ir() {
    let analyzed = common::analyze_ok("fn main() -> i32 { 0 }");
    let ir = lower_to_ir(&analyzed).expect("ir lowering should succeed");

    assert_eq!(ir.functions.len(), 1);
    let function = &ir.functions[0];
    assert_eq!(function.blocks.len(), 1);
    assert_eq!(function.entry.index(), 0);
    assert!(matches!(
        function.blocks[0].terminator,
        Some(Terminator::Return(Some(_)))
    ));
}

#[test]
fn normalizes_ir_operands_as_typed_values() {
    let analyzed = common::analyze_ok("fn main(value: i32) -> i32 { val next = value + 1; next }");
    let ir = lower_to_ir(&analyzed).expect("ir lowering should succeed");
    let function = &ir.functions[0];

    assert_eq!(
        function.params[0].ty,
        function.locals[function.params[0].local.index()].ty
    );
    for block in &function.blocks {
        for instruction in &block.instructions {
            for value in instruction_values(instruction) {
                assert_value_matches_temp_layout(function, value);
            }
        }
        if let Some(terminator) = &block.terminator {
            for value in terminator_values(terminator) {
                assert_value_matches_temp_layout(function, value);
            }
        }
    }
}

#[test]
fn records_ir_function_effect_summary() {
    let analyzed = common::analyze_ok(
        r#"
fn main() -> usize {
    val values = [1, 2];
    values.push(3);
    print("ok");
    values.len()
}
"#,
    );
    let ir = lower_to_ir(&analyzed).expect("ir lowering should succeed");
    let effects = ir.functions[0].effects;

    assert!(effects.reads_local);
    assert!(effects.writes_local);
    assert!(effects.reads_aggregate);
    assert!(effects.writes_aggregate);
    assert!(effects.allocates);
    assert!(effects.calls);
    assert!(effects.touches_runtime);
    assert!(effects.may_trap);
}

#[test]
fn lowers_if_expression_into_branching_blocks() {
    let analyzed = common::analyze_ok("fn main() -> i32 { if true { 1 } else { 2 } }");
    let ir = lower_to_ir(&analyzed).expect("ir lowering should succeed");
    let function = &ir.functions[0];

    assert!(function.blocks.len() >= 4);
    assert!(matches!(
        function.blocks[0].terminator,
        Some(Terminator::Branch { .. })
    ));
    assert!(
        function
            .blocks
            .iter()
            .flat_map(|block| block.instructions.iter())
            .any(|instruction| matches!(instruction, Instruction::Move { .. }))
    );
}

#[test]
fn lowers_short_circuit_boolean_operators_into_branches() {
    let analyzed = common::analyze_ok("fn main() -> bool { true && false || true }");
    let ir = lower_to_ir(&analyzed).expect("ir lowering should succeed");
    let function = &ir.functions[0];

    let branch_count = function
        .blocks
        .iter()
        .filter(|block| matches!(block.terminator, Some(Terminator::Branch { .. })))
        .count();
    assert!(branch_count >= 2);

    assert!(
        !function
            .blocks
            .iter()
            .flat_map(|block| block.instructions.iter())
            .any(|instruction| {
                matches!(
                    instruction,
                    Instruction::Binary {
                        op: BinaryOp::AndAnd | BinaryOp::OrOr,
                        ..
                    }
                )
            })
    );
}

#[test]
fn lowers_match_expression_into_decision_chain() {
    let analyzed = common::analyze_ok("fn main() -> i32 { match 1 { 0 => 10, _ => 20 } }");
    let ir = lower_to_ir(&analyzed).expect("ir lowering should succeed");
    let function = &ir.functions[0];

    assert!(function.blocks.len() >= 5);
    assert!(
        function
            .blocks
            .iter()
            .any(|block| matches!(block.terminator, Some(Terminator::Unreachable)))
    );
    assert!(
        function
            .blocks
            .iter()
            .flat_map(|block| block.instructions.iter())
            .any(|instruction| matches!(instruction, Instruction::Binary { .. }))
    );
}

#[test]
fn lowers_named_match_pattern_binding() {
    let analyzed =
        common::analyze_ok("fn main(value: i32) -> i32 { match value { bound => bound } }");
    let ir = lower_to_ir(&analyzed).expect("ir lowering should succeed");
    let function = &ir.functions[0];

    assert!(
        function
            .blocks
            .iter()
            .flat_map(|block| block.instructions.iter())
            .any(|instruction| matches!(instruction, Instruction::StoreLocal { .. }))
    );
    assert!(
        function
            .blocks
            .iter()
            .flat_map(|block| block.instructions.iter())
            .any(|instruction| matches!(instruction, Instruction::LoadLocal { .. }))
    );
}

#[test]
fn lowers_const_references_as_plain_constants_without_module_slots() {
    let analyzed = common::analyze_ok(
        r#"
const BASE: i32 = 1;
const VALUE: i32 = BASE + 2;

fn main() -> i32 { VALUE }
"#,
    );
    let ir = lower_to_ir(&analyzed).expect("ir lowering should succeed");
    let function = ir
        .functions
        .iter()
        .find(|function| function.name == "main")
        .expect("expected main function");

    assert!(ir.module_slots.is_empty());
    assert!(
        function
            .blocks
            .iter()
            .flat_map(|block| block.instructions.iter())
            .any(|instruction| matches!(instruction, Instruction::LoadConst { .. }))
    );
    assert!(
        !function
            .blocks
            .iter()
            .flat_map(|block| block.instructions.iter())
            .any(|instruction| matches!(instruction, Instruction::LoadModule { .. }))
    );
}

#[test]
fn lowers_field_and_index_assignments_via_runtime_helpers() {
    let analyzed = common::analyze_ok(
        r#"
struct Point { var x: i32 }
struct Holder { var inner: Point }

fn main() -> i32 {
    var holder = Holder { inner: Point { x: 1 } };
    holder.inner.x = 7;
    var values = [1, 2];
    values[0] = 5;
    holder.inner.x + values[0]
}
"#,
    );
    let ir = lower_to_ir(&analyzed).expect("ir lowering should succeed");
    let function = &ir.functions[0];

    assert!(
        function
            .blocks
            .iter()
            .flat_map(|block| block.instructions.iter())
            .any(|instruction| matches!(
                instruction,
                Instruction::Call {
                    callee: CallTarget::RuntimeHelper(RuntimeHelper::ReflectSetField(field)),
                    ..
                } if field == "x"
            ))
    );
    assert!(
        function
            .blocks
            .iter()
            .flat_map(|block| block.instructions.iter())
            .any(|instruction| matches!(
                instruction,
                Instruction::Call {
                    callee: CallTarget::RuntimeHelper(RuntimeHelper::ReflectSetIndex),
                    ..
                }
            ))
    );
}

#[test]
fn lowers_array_methods_to_builtin_method_calls() {
    let analyzed = common::analyze_ok(
        r#"
fn main() -> [i32] {
    val values = [1, 2];
    values.push(3).pop()
}
"#,
    );
    let ir = lower_to_ir(&analyzed).expect("ir lowering should succeed");
    let function = &ir.functions[0];

    assert!(
        function
            .blocks
            .iter()
            .flat_map(|block| block.instructions.iter())
            .any(|instruction| matches!(
                instruction,
                Instruction::Call {
                    callee: CallTarget::BuiltinMethod(BuiltinMethod::Array(array::Method::Push)),
                    ..
                }
            ))
    );
    assert!(
        function
            .blocks
            .iter()
            .flat_map(|block| block.instructions.iter())
            .any(|instruction| matches!(
                instruction,
                Instruction::Call {
                    callee: CallTarget::BuiltinMethod(BuiltinMethod::Array(array::Method::Pop)),
                    ..
                }
            ))
    );
}

#[test]
fn lowers_tuple_array_struct_and_access_expressions() {
    let analyzed = common::analyze_ok(
        r#"
struct Point { var x: i32 }

fn main() -> () {
    val tuple = (1, 2);
    tuple;
    val array = [1, 2];
    array[0];
    val point = Point { x: 1 };
    point.x;
}
"#,
    );
    let ir = lower_to_ir(&analyzed).expect("ir lowering should succeed");
    let function = &ir.functions[0];

    assert!(
        function
            .blocks
            .iter()
            .flat_map(|block| block.instructions.iter())
            .any(|instruction| matches!(instruction, Instruction::MakeTuple { .. }))
    );
    assert!(
        function
            .blocks
            .iter()
            .flat_map(|block| block.instructions.iter())
            .any(|instruction| matches!(instruction, Instruction::MakeArray { .. }))
    );
    assert!(
        function
            .blocks
            .iter()
            .flat_map(|block| block.instructions.iter())
            .any(|instruction| matches!(instruction, Instruction::MakeStruct { .. }))
    );
    assert!(
        function
            .blocks
            .iter()
            .flat_map(|block| block.instructions.iter())
            .any(|instruction| matches!(instruction, Instruction::ReadAggregateIndex { .. }))
    );
    assert!(
        function
            .blocks
            .iter()
            .flat_map(|block| block.instructions.iter())
            .any(|instruction| matches!(instruction, Instruction::ReadAggregateField { .. }))
    );
}

fn assert_value_matches_temp_layout(function: &IrFunction, value: IrValue) {
    assert_eq!(function.temps[value.temp.index()].ty, value.ty);
}

fn instruction_values(instruction: &Instruction) -> Vec<IrValue> {
    match instruction {
        Instruction::LoadConst { dst, .. }
        | Instruction::LoadLocal { dst, .. }
        | Instruction::LoadModule { dst, .. } => vec![*dst],
        Instruction::StoreLocal { src, .. } | Instruction::StoreModule { src, .. } => vec![*src],
        Instruction::Move { dst, src } => vec![*dst, *src],
        Instruction::Unary { dst, operand, .. } => vec![*dst, *operand],
        Instruction::Binary { dst, lhs, rhs, .. } => vec![*dst, *lhs, *rhs],
        Instruction::Call { dst, callee, args } => {
            let mut values = Vec::new();
            if let Some(dst) = dst {
                values.push(*dst);
            }
            if let CallTarget::Value(callee) = callee {
                values.push(*callee);
            }
            values.extend(args.iter().copied());
            values
        }
        Instruction::MakeTuple { dst, elements } | Instruction::MakeArray { dst, elements } => {
            let mut values = vec![*dst];
            values.extend(elements.iter().copied());
            values
        }
        Instruction::MakeStruct { dst, fields, .. } => {
            let mut values = vec![*dst];
            values.extend(fields.iter().map(|field| field.value));
            values
        }
        Instruction::ReadAggregateField { dst, base, .. } => vec![*dst, *base],
        Instruction::ReadAggregateIndex { dst, base, index } => vec![*dst, *base, *index],
        Instruction::ReadPath {
            dst,
            root_or_view,
            dynamic_args,
            ..
        }
        | Instruction::MakePathView {
            dst,
            root_or_view,
            dynamic_args,
            ..
        } => {
            let mut values = vec![*dst, *root_or_view];
            values.extend(dynamic_args.iter().copied());
            values
        }
        Instruction::SetPath {
            root_or_view,
            dynamic_args,
            value,
            ..
        } => {
            let mut values = vec![*root_or_view, *value];
            values.extend(dynamic_args.iter().copied());
            values
        }
        Instruction::ModifyPath {
            dst,
            root_or_view,
            dynamic_args,
            value,
            ..
        } => {
            let mut values = vec![*root_or_view, *value];
            if let Some(dst) = dst {
                values.push(*dst);
            }
            values.extend(dynamic_args.iter().copied());
            values
        }
    }
}

fn terminator_values(terminator: &Terminator) -> Vec<IrValue> {
    match terminator {
        Terminator::Return(value) => value.iter().copied().collect(),
        Terminator::Branch { cond, .. } => vec![*cond],
        Terminator::Jump(_) | Terminator::Unreachable => Vec::new(),
    }
}
