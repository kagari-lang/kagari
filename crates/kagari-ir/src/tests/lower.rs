use crate::{
    lower_to_ir,
    module::{Instruction, Terminator},
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
                        op: crate::module::instruction::BinaryOp::AndAnd
                            | crate::module::instruction::BinaryOp::OrOr,
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
fn lowers_tuple_array_struct_and_access_expressions() {
    let analyzed = common::analyze_ok(
        r#"
struct Point { x: i32 }

fn main() -> unit {
    let tuple = (1, 2);
    tuple;
    let array = [1, 2];
    array[0];
    let point = Point { x: 1 };
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
            .any(|instruction| matches!(instruction, Instruction::ReadIndex { .. }))
    );
    assert!(
        function
            .blocks
            .iter()
            .flat_map(|block| block.instructions.iter())
            .any(|instruction| matches!(instruction, Instruction::ReadField { .. }))
    );
}
