use crate::tests::common;
use kagari_bytecode::{instruction::BytecodeInstruction, program::verify_program};
use kagari_types::{integer::IntegerOp, scalar::BuiltinType};

#[test]
fn narrow_arithmetic_retains_source_width_without_assert_expansion() {
    for (name, input) in [
        ("i8", BuiltinType::I8),
        ("i16", BuiltinType::I16),
        ("u8", BuiltinType::U8),
        ("u16", BuiltinType::U16),
        ("u32", BuiltinType::U32),
    ] {
        for (operator, expected) in [
            ("+", IntegerOp::CheckedAdd),
            ("-", IntegerOp::CheckedSub),
            ("*", IntegerOp::CheckedMul),
            ("/", IntegerOp::CheckedDiv),
            ("%", IntegerOp::CheckedRem),
        ] {
            let source = format!(
                "fn calc(a: {name}, b: {name}) -> {name} {{ a {operator} b }}
                 fn main() -> {name} {{ calc(6{name}, 3{name}) }}"
            );
            let program = common::bytecode_ok(&source);
            let function = program.modules[program.root.index()]
                .functions
                .iter()
                .find(|function| function.name == "calc")
                .unwrap();
            let operations = function
                .instructions
                .iter()
                .filter_map(|instruction| match instruction {
                    BytecodeInstruction::Numeric { operation, .. } => Some(operation),
                    _ => None,
                })
                .collect::<Vec<_>>();
            assert_eq!(operations.len(), 1, "{source}");
            assert_eq!(operations[0].op, expected);
            assert_eq!(operations[0].input, input);
            assert_eq!(operations[0].rhs, Some(input));
            assert!(function.instructions.iter().all(|instruction| !matches!(
                instruction,
                BytecodeInstruction::Call { .. } | BytecodeInstruction::Binary { .. }
            )));

            let mut malformed = program.clone();
            let function = malformed.modules[malformed.root.index()]
                .functions
                .iter_mut()
                .find(|function| function.name == "calc")
                .unwrap();
            let operation = function
                .instructions
                .iter_mut()
                .find_map(|instruction| match instruction {
                    BytecodeInstruction::Numeric { operation, .. } => Some(operation),
                    _ => None,
                })
                .unwrap();
            let original = *operation;
            operation.rhs = Some(BuiltinType::F32);
            assert!(verify_program(&malformed).is_err());
            let operation = malformed.modules[malformed.root.index()]
                .functions
                .iter_mut()
                .find(|function| function.name == "calc")
                .unwrap()
                .instructions
                .iter_mut()
                .find_map(|instruction| match instruction {
                    BytecodeInstruction::Numeric { operation, .. } => Some(operation),
                    _ => None,
                })
                .unwrap();
            *operation = original;
            operation.input = BuiltinType::I64;
            operation.rhs = Some(BuiltinType::I64);
            assert!(verify_program(&malformed).is_err());
        }
    }
}

#[test]
fn narrow_negation_uses_the_checked_source_domain() {
    for (name, input) in [("i8", BuiltinType::I8), ("i16", BuiltinType::I16)] {
        let program = common::bytecode_ok(&format!(
            "fn neg(a: {name}) -> {name} {{ -a }} fn main() -> {name} {{ neg(6{name}) }}"
        ));
        let function = program.modules[program.root.index()]
            .functions
            .iter()
            .find(|function| function.name == "neg")
            .unwrap();
        assert!(function.instructions.iter().any(|instruction| matches!(
            instruction,
            BytecodeInstruction::Numeric { operation, .. }
                if operation.op == IntegerOp::CheckedSub && operation.input == input
        )));
        assert!(function.instructions.iter().all(|instruction| !matches!(
            instruction,
            BytecodeInstruction::Call { .. } | BytecodeInstruction::Unary { .. }
        )));
    }
}
