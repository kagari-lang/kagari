use crate::{
    builtin::array,
    bytecode::{
        BinaryOp, BuiltinMethod, BytecodeFunction, BytecodeInstruction, BytecodeModule,
        BytecodeVerificationError, CallTarget, FunctionMetadata, FunctionRef, JumpTarget,
        LocalSlot, PathId, PathRecord, Register, RuntimeHelper, UnaryOp, verify_module,
    },
    module::ValueType,
    tests::common,
};

#[test]
fn lowers_function_metadata_into_bytecode() {
    let bytecode = common::bytecode_ok("fn add(a: i32, b: i32) -> i32 { val c = a + b; c }");
    let function = &bytecode.functions[0];

    assert_eq!(function.id, FunctionRef::new(0));
    assert_eq!(function.name, "add");
    assert_eq!(function.parameter_count, 2);
    assert_eq!(function.local_count, 3);
    assert!(function.register_count >= 4);
    assert_eq!(
        function.metadata.params,
        vec![ValueType::I32, ValueType::I32]
    );
    assert_eq!(function.metadata.return_type, ValueType::I32);
    assert_eq!(
        function.metadata.locals[..3],
        [ValueType::I32, ValueType::I32, ValueType::I32]
    );
    assert_eq!(
        function.metadata.registers.len(),
        usize::from(function.register_count)
    );
}

#[test]
fn populates_bytecode_tables_and_effect_metadata() {
    let bytecode = common::bytecode_ok(
        r#"
fn add(a: i32, b: i32) -> i32 { a + b }

fn main() -> i32 {
    print("ok");
    add(1, 2)
}
"#,
    );
    let main = bytecode
        .functions
        .iter()
        .find(|function| function.name == "main")
        .expect("expected main function");

    assert!(bytecode.constants.iter().any(|constant| matches!(
        constant,
        crate::bytecode::ConstantOperand::Str(text) if text == "ok"
    )));
    assert!(bytecode.types.contains(&ValueType::I32));
    assert!(bytecode.types.contains(&ValueType::Str));
    assert_eq!(bytecode.function_table.len(), bytecode.functions.len());
    assert_eq!(bytecode.function_table[0].name, "add");
    assert_eq!(
        bytecode.function_table[0].params,
        vec![ValueType::I32, ValueType::I32]
    );
    assert_eq!(bytecode.function_table[0].return_type, ValueType::I32);
    assert!(main.metadata.effects.calls);
    assert!(main.metadata.effects.touches_runtime);
    assert!(verify_module(&bytecode).is_ok());
}

#[test]
fn verifier_rejects_malformed_register_local_and_control_flow_bytecode() {
    let mut invalid_register = common::bytecode_ok("fn main() -> i32 { 1 }");
    invalid_register.functions[0].instructions[0] = BytecodeInstruction::LoadConst {
        dst: Register::new(999),
        constant: crate::bytecode::ConstantOperand::I32(1),
    };
    assert!(matches!(
        verify_module(&invalid_register),
        Err(BytecodeVerificationError::InvalidRegister { .. })
    ));

    let mut invalid_local = common::bytecode_ok("fn main() -> i32 { val value = 1; value }");
    invalid_local.functions[0].instructions[1] = BytecodeInstruction::StoreLocal {
        local: LocalSlot::new(999),
        src: Register::new(0),
    };
    assert!(matches!(
        verify_module(&invalid_local),
        Err(BytecodeVerificationError::InvalidLocal { .. })
    ));

    let mut invalid_jump = common::bytecode_ok("fn main() -> i32 { if true { 1 } else { 2 } }");
    invalid_jump.functions[0]
        .metadata
        .control_flow_targets
        .push(JumpTarget::new(usize::MAX));
    assert!(matches!(
        verify_module(&invalid_jump),
        Err(BytecodeVerificationError::InvalidJumpTarget { .. })
    ));
}

#[test]
fn verifier_rejects_type_inconsistent_bytecode() {
    let mut bytecode = common::bytecode_ok("fn main() -> i32 { 1 }");
    bytecode.functions[0].metadata.return_type = ValueType::Bool;
    bytecode.function_table[0].return_type = ValueType::Bool;
    bytecode.types.push(ValueType::Bool);

    assert!(matches!(
        verify_module(&bytecode),
        Err(BytecodeVerificationError::TypeMismatch {
            context: "return value",
            expected: ValueType::Bool,
            found: ValueType::I32,
            ..
        })
    ));
}

#[test]
fn lowers_arithmetic_into_real_bytecode_instructions() {
    let bytecode = common::bytecode_ok("fn add(a: i32, b: i32) -> i32 { val c = a + b; c }");
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
fn main() -> () {
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
struct Point { var x: i32 }

fn main() -> () {
    val tuple = (1, 2);
    val array = [1, 2];
    val point = Point { x: 1 };
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
        function.instructions.iter().any(|instruction| matches!(
            instruction,
            BytecodeInstruction::ReadAggregateIndex { .. }
        ))
    );
    assert!(
        function.instructions.iter().any(|instruction| matches!(
            instruction,
            BytecodeInstruction::ReadAggregateField { .. }
        ))
    );
    assert!(bytecode.fields.iter().any(|field| field.name == "x"));
    assert!(function.instructions.iter().any(|instruction| matches!(
        instruction,
        BytecodeInstruction::ReadAggregateField { field, .. }
            if bytecode.fields.get(field.index()).is_some_and(|record| record.name == "x")
    )));
}

#[test]
fn verifier_accepts_resolved_typed_path_instructions() {
    let module = BytecodeModule {
        types: vec![ValueType::HeapObject, ValueType::I32],
        paths: vec![PathRecord {
            id: PathId::new(0),
            root_ty: ValueType::HeapObject,
            result_ty: ValueType::I32,
            read_only: false,
            debug_name: "Actor.health".to_owned(),
        }],
        function_table: vec![crate::bytecode::FunctionRecord {
            id: FunctionRef::new(0),
            name: "read_health".to_owned(),
            params: vec![ValueType::HeapObject],
            return_type: ValueType::I32,
            effects: crate::module::EffectSet::path_read(),
        }],
        functions: vec![BytecodeFunction {
            id: FunctionRef::new(0),
            name: "read_health".to_owned(),
            parameter_count: 1,
            local_count: 1,
            register_count: 2,
            metadata: FunctionMetadata {
                params: vec![ValueType::HeapObject],
                return_type: ValueType::I32,
                locals: vec![ValueType::HeapObject],
                registers: vec![ValueType::HeapObject, ValueType::I32],
                effects: crate::module::EffectSet::path_read(),
                ..Default::default()
            },
            instructions: vec![
                BytecodeInstruction::ReadPath {
                    dst: Register::new(1),
                    root_or_view: Register::new(0),
                    path: PathId::new(0),
                    dynamic_args: Vec::new(),
                },
                BytecodeInstruction::Return(Some(Register::new(1))),
            ],
        }],
        ..Default::default()
    };

    assert!(verify_module(&module).is_ok());
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
    let bytecode = common::bytecode_ok("fn main() -> String { type_of(7) }");
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
struct Point { var x: i32 }

fn main() -> Point {
    val point = Point { x: 1 };
    val next = set_field(point, "x", 9);
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
val boot = 1;

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

#[test]
fn lowers_array_methods_to_builtin_method_calls() {
    let bytecode = common::bytecode_ok(
        r#"
fn main() -> usize {
    val values = [1, 2];
    values.push(3).pop().len()
}
"#,
    );
    let function = bytecode
        .functions
        .iter()
        .find(|function| function.name == "main")
        .expect("expected main function");

    assert!(function.instructions.iter().any(|instruction| matches!(
        instruction,
        BytecodeInstruction::Call {
            callee: CallTarget::BuiltinMethod(BuiltinMethod::Array(array::Method::Push)),
            ..
        }
    )));
    assert!(function.instructions.iter().any(|instruction| matches!(
        instruction,
        BytecodeInstruction::Call {
            callee: CallTarget::BuiltinMethod(BuiltinMethod::Array(array::Method::Pop)),
            ..
        }
    )));
    assert!(function.instructions.iter().any(|instruction| matches!(
        instruction,
        BytecodeInstruction::Call {
            callee: CallTarget::BuiltinMethod(BuiltinMethod::Array(array::Method::Len)),
            ..
        }
    )));
}
