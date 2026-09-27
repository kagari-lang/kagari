use crate::tests::bytecode::*;
use kagari_abi::budget::LogicalBudgetCharge;
use kagari_abi::effects::EffectSet;
use kagari_bytecode as bytecode;

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
fn lowers_debugger_metadata_into_bytecode() {
    let bytecode = common::bytecode_ok(
        r#"
fn main(value: i32) -> i32 {
    val next = value + 1;
    print("debug");
    next
}
"#,
    );
    let function = &bytecode.functions[0];
    let debug = &function.metadata.debug;

    assert_eq!(debug.source_spans.len(), function.instructions.len());
    assert_eq!(debug.line_table.len(), function.instructions.len());
    assert_eq!(debug.frame_layout.locals, function.metadata.locals);
    assert_eq!(debug.frame_layout.registers, function.metadata.registers);
    assert!(
        debug
            .safe_debug_points
            .iter()
            .any(|point| point.kind == SafeDebugPointKind::FunctionEntry)
    );
    assert!(
        debug
            .safe_debug_points
            .iter()
            .any(|point| point.kind == SafeDebugPointKind::CallBoundary)
    );
    assert!(
        debug
            .safe_debug_points
            .iter()
            .any(|point| point.kind == SafeDebugPointKind::FunctionReturn)
    );
    assert!(
        debug
            .local_live_ranges
            .iter()
            .any(|range| range.name == "value" && range.is_parameter)
    );
    assert!(
        debug
            .local_live_ranges
            .iter()
            .any(|range| range.name == "next" && !range.is_parameter)
    );
    let parameter = debug
        .local_live_ranges
        .iter()
        .find(|range| range.name == "value")
        .unwrap();
    let next = debug
        .local_live_ranges
        .iter()
        .find(|range| range.name == "next")
        .unwrap();
    let initializing_store = function
        .instructions
        .iter()
        .position(|instruction| {
            matches!(instruction, BytecodeInstruction::StoreLocal { local, .. } if *local == next.local)
        })
        .unwrap();
    assert_eq!(parameter.start, 0);
    assert_eq!(next.start, initializing_store + 1);
    assert!(next.end > next.start);
    assert!(next.end <= function.instructions.len());

    let artifact_debug = DebugMetadata::from_module(&bytecode);
    assert!(!artifact_debug.stripped);
    assert_eq!(artifact_debug.functions.len(), bytecode.functions.len());
    assert!(artifact_debug.debug_names.iter().any(|name| name == "main"));
}

#[test]
fn debug_local_ranges_close_when_their_lexical_block_ends() {
    let source = r#"
fn main() -> i32 {
    val outer = 5;
    if true {
        val inner = 1;
        print("inside");
    } else {
        print("outside");
    };
    outer
}
"#;
    let bytecode = common::bytecode_ok(source);
    let function = bytecode
        .functions
        .iter()
        .find(|f| f.name == "main")
        .unwrap();
    let debug = &function.metadata.debug;
    let tail_source_offset = source.rfind("outer").unwrap();
    let tail_offset = debug
        .source_spans
        .iter()
        .find(|entry| entry.span.start <= tail_source_offset && tail_source_offset < entry.span.end)
        .unwrap()
        .instruction_offset;
    let inner = debug
        .local_live_ranges
        .iter()
        .filter(|range| range.name == "inner")
        .collect::<Vec<_>>();
    assert!(!inner.is_empty());
    assert!(
        inner
            .iter()
            .all(|range| tail_offset < range.start || tail_offset >= range.end)
    );
    assert!(debug.local_live_ranges.iter().any(|range| {
        range.name == "outer" && range.start <= tail_offset && tail_offset < range.end
    }));
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
        bytecode::ConstantOperand::Str(text) if text == "ok"
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
    assert!(
        bytecode
            .structures
            .iter()
            .flat_map(|layout| &layout.fields)
            .any(|field| field.name == "x")
    );
    assert!(function.instructions.iter().any(|instruction| matches!(
        instruction,
        BytecodeInstruction::ReadAggregateField { field, .. }
            if bytecode.structures.get(field.structure.index()).and_then(|layout| layout.fields.get(field.slot as usize)).is_some_and(|record| record.name == "x")
    )));
}

#[test]
fn verifier_accepts_resolved_typed_path_instructions() {
    let mut module = BytecodeModule {
        types: vec![ValueType::HostHandle, ValueType::I32],
        paths: vec![PathRecord {
            contract_fingerprint: 0,
            id: PathId::new(0),
            root_ty: ValueType::HostHandle,
            result_ty: ValueType::I32,
            read_only: false,
            debug_name: "Actor.health".to_owned(),
        }],
        function_table: vec![bytecode::FunctionRecord {
            id: FunctionRef::new(0),
            identity: None,
            name: "read_health".to_owned(),
            params: vec![ValueType::HostHandle],
            return_type: ValueType::I32,
            effects: EffectSet::path_read(),
        }],
        functions: vec![BytecodeFunction {
            id: FunctionRef::new(0),
            identity: None,
            name: "read_health".to_owned(),
            parameter_count: 1,
            local_count: 1,
            register_count: 2,
            metadata: FunctionMetadata {
                instruction_budgets: vec![LogicalBudgetCharge::Step; 2],
                params: vec![ValueType::HostHandle],
                return_type: ValueType::I32,
                locals: vec![ValueType::HostHandle],
                registers: vec![ValueType::HostHandle, ValueType::I32],
                effects: EffectSet::path_read(),
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
    module.paths[0].root_ty = ValueType::HeapObject;
    assert_eq!(
        verify_module(&module),
        Err(BytecodeVerificationError::InvalidPathLayout)
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
fn reflection_helper_operands_are_checked_before_loading() {
    let valid = common::bytecode_ok("fn main() -> String { type_of(7) }");
    let mut wrong_arity = valid.clone();
    let call = wrong_arity.functions[0]
        .instructions
        .iter_mut()
        .find(|instruction| matches!(instruction, BytecodeInstruction::Call { .. }))
        .unwrap();
    let BytecodeInstruction::Call { args, .. } = call else {
        unreachable!()
    };
    args.clear();
    assert!(matches!(
        verify_module(&wrong_arity),
        Err(BytecodeVerificationError::InvalidOperation { .. })
    ));

    let mut wrong_result = valid.clone();
    let call = wrong_result.functions[0]
        .instructions
        .iter_mut()
        .find(|instruction| matches!(instruction, BytecodeInstruction::Call { .. }))
        .unwrap();
    let BytecodeInstruction::Call { dst, args, .. } = call else {
        unreachable!()
    };
    *dst = Some(args[0]);
    assert!(matches!(
        verify_module(&wrong_result),
        Err(BytecodeVerificationError::TypeMismatch { .. })
    ));

    let mut wrong_field_base = valid;
    let call = wrong_field_base.functions[0]
        .instructions
        .iter_mut()
        .find(|instruction| matches!(instruction, BytecodeInstruction::Call { .. }))
        .unwrap();
    let BytecodeInstruction::Call { callee, .. } = call else {
        unreachable!()
    };
    *callee = CallTarget::RuntimeHelper(RuntimeHelper::ReflectGetField("x".into()));
    assert!(matches!(
        verify_module(&wrong_field_base),
        Err(BytecodeVerificationError::TypeMismatch { .. })
    ));

    let mut wrong_index =
        common::bytecode_ok("fn main() -> ArrayList<i32> { set_index([1], 0, 2) }");
    let call = wrong_index.functions[0]
        .instructions
        .iter_mut()
        .find(|instruction| {
            matches!(
                instruction,
                BytecodeInstruction::Call {
                    callee: CallTarget::RuntimeHelper(RuntimeHelper::ReflectSetIndex),
                    ..
                }
            )
        })
        .unwrap();
    let BytecodeInstruction::Call { args, .. } = call else {
        unreachable!()
    };
    args[1] = args[0];
    assert!(matches!(
        verify_module(&wrong_index),
        Err(BytecodeVerificationError::InvalidOperation { .. })
    ));
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
fn main(values: ArrayList<i32>) -> ArrayList<i32> {
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
fn lowers_place_assignments_to_aggregate_writes() {
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
        BytecodeInstruction::WriteAggregateField { field, .. }
            if bytecode.structures.get(field.structure.index()).and_then(|layout| layout.fields.get(field.slot as usize)).is_some_and(|record| record.name == "x")
    )));
    assert!(
        function.instructions.iter().any(|instruction| matches!(
            instruction,
            BytecodeInstruction::WriteAggregateIndex { .. }
        ))
    );
    assert!(!function.instructions.iter().any(|instruction| matches!(
        instruction,
        BytecodeInstruction::Call {
            callee: CallTarget::RuntimeHelper(
                RuntimeHelper::ReflectSetField(_) | RuntimeHelper::ReflectSetIndex
            ),
            ..
        }
    )));
    assert!(function.metadata.effects.writes_aggregate);
    assert!(!function.metadata.effects.calls);
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
fn stdlib_lowers_standard_library_calls_to_bytecode_intrinsic_ids() {
    let bytecode = common::bytecode_ok(
        r#"
fn main() -> usize {
    val values = [1, 2];
    values.push(3);
    values.pop();
    values.len()
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
            callee: CallTarget::StandardIntrinsic(StandardIntrinsic::ArrayPush),
            ..
        }
    )));
    assert!(function.instructions.iter().any(|instruction| matches!(
        instruction,
        BytecodeInstruction::Call {
            callee: CallTarget::StandardIntrinsic(StandardIntrinsic::ArrayPop),
            ..
        }
    )));
    assert!(function.instructions.iter().any(|instruction| matches!(
        instruction,
        BytecodeInstruction::Call {
            callee: CallTarget::StandardIntrinsic(StandardIntrinsic::ArrayLen),
            ..
        }
    )));
}
