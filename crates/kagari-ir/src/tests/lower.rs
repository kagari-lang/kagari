use crate::{
    lower_to_ir,
    module::instruction::RuntimeHelper,
    module::{
        BinaryOp, CallTarget, Instruction, IrFunction, IrValue, StandardIntrinsic, Terminator,
    },
    tests::common,
};

#[test]
fn checked_enum_constructors_report_the_execution_layout_boundary() {
    for expression in ["Event::Empty", "Event::Data(7)"] {
        let source =
            format!("enum Event {{ Empty, Data(i32) }} fn main() -> Event {{ {expression} }}");
        let checked = common::analyze_ok(&source);
        assert!(matches!(
            lower_to_ir(&checked, &Default::default()),
            Err(crate::lower::IrLoweringError::UnsupportedExpr(
                "enum construction requires linked enum layouts"
            ))
        ));
    }
}

#[test]
fn monomorphizes_reachable_arguments_and_deduplicates_instances() {
    let analyzed = common::analyze_ok(
        "fn unused<T>(x: T) -> T { x } fn echo<T>(x: T) -> T { x } fn wrap<U>(x: U) -> U { echo(x) } fn main() -> (i32, i32, String) { (wrap(7), echo(8), echo(\"ok\")) }",
    );
    let ir = lower_to_ir(&analyzed, &Default::default()).unwrap();
    assert_eq!(ir.functions.len(), 4);
    assert!(
        !ir.functions.iter().any(|function| function
            .instance
            .declaration
            .path
            .last()
            .unwrap()
            .name
            == "unused")
    );
    let echo = ir
        .functions
        .iter()
        .filter(|function| function.instance.declaration.path.last().unwrap().name == "echo")
        .collect::<Vec<_>>();
    assert_eq!(echo.len(), 2);
    assert_eq!(echo[0].instance.declaration, echo[1].instance.declaration);
    assert_ne!(echo[0].instance.arguments, echo[1].instance.arguments);
    assert_ne!(echo[0].id, echo[1].id);
    let representations = echo
        .iter()
        .map(|function| function.params[0].ty)
        .collect::<Vec<_>>();
    assert!(representations.contains(&crate::module::ValueType::I32));
    assert!(representations.contains(&crate::module::ValueType::Str));
    crate::bytecode::lower_to_bytecode(&ir).unwrap();
}

#[test]
fn unreachable_calls_after_return_do_not_create_instances() {
    let analyzed =
        common::analyze_ok("fn grow<T>(x: T) { grow((x, x)); } fn main() { return; grow(1); }");
    let ir = lower_to_ir(
        &analyzed,
        &crate::IrLoweringOptions {
            max_generic_instances: 0,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(ir.functions.len(), 1);
    crate::bytecode::lower_to_bytecode(&ir).unwrap();
}

#[test]
fn recursive_instantiation_reuses_the_current_instance() {
    let analyzed = common::analyze_ok(
        "fn repeat<T>(x: T, n: i32) -> T { if n == 0 { x } else { repeat(x, n - 1) } } fn main() -> i32 { repeat(7, 3) }",
    );
    let ir = lower_to_ir(
        &analyzed,
        &crate::IrLoweringOptions {
            max_generic_instances: 1,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(ir.functions.len(), 2);
    crate::bytecode::lower_to_bytecode(&ir).unwrap();
}

#[test]
fn recursive_type_growth_is_bounded_before_execution() {
    let analyzed = common::analyze_ok("fn grow<T>(x: T) { grow((x, x)); } fn main() { grow(1); }");
    for (options, resource, limit) in [
        (
            crate::IrLoweringOptions {
                max_generic_instances: 2,
                ..Default::default()
            },
            "generic instances",
            2,
        ),
        (
            crate::IrLoweringOptions {
                max_type_nodes: 31,
                ..Default::default()
            },
            "instantiated type nodes",
            31,
        ),
        (
            crate::IrLoweringOptions {
                max_type_depth: 2,
                ..Default::default()
            },
            "instantiated type depth",
            2,
        ),
    ] {
        let Err(crate::IrLoweringError::Diagnostic(diagnostic)) = lower_to_ir(&analyzed, &options)
        else {
            panic!("growth must be rejected");
        };
        assert_eq!(
            diagnostic.kind,
            kagari_common::DiagnosticKind::CompileLimitExceeded { resource, limit }
        );
        assert!(diagnostic.span.is_some());
    }
}

#[test]
fn lowering_honors_instruction_limits_and_cancellation() {
    let analyzed = common::analyze_ok("fn main() -> i32 { 7 }");
    assert!(matches!(
        lower_to_ir(
            &analyzed,
            &crate::IrLoweringOptions {
                max_instructions: 1,
                ..Default::default()
            }
        ),
        Err(crate::IrLoweringError::Diagnostic(_))
    ));
    lower_to_ir(
        &analyzed,
        &crate::IrLoweringOptions {
            max_instructions: 2,
            max_generic_instances: 0,
            ..Default::default()
        },
    )
    .unwrap();
    let options = crate::IrLoweringOptions::default();
    options.cancel.cancel();
    assert!(matches!(
        lower_to_ir(&analyzed, &options),
        Err(crate::IrLoweringError::Cancelled)
    ));
    let empty = common::analyze_ok("");
    assert!(matches!(
        lower_to_ir(&empty, &options),
        Err(crate::IrLoweringError::Cancelled)
    ));
}

#[test]
fn lowers_function_into_cfg_shaped_ir() {
    let analyzed = common::analyze_ok("fn main() -> i32 { 0 }");
    let ir = lower_to_ir(&analyzed, &Default::default()).expect("ir lowering should succeed");

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
    let ir = lower_to_ir(&analyzed, &Default::default()).expect("ir lowering should succeed");
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
fn integer_operations_expose_traps_to_downstream_backends() {
    for expression in ["value + 1", "value - 1", "value * 2", "value / 2", "-value"] {
        let analyzed =
            common::analyze_ok(&format!("fn main(value: i32) -> i32 {{ {expression} }}"));
        let ir = lower_to_ir(&analyzed, &Default::default()).unwrap();
        assert!(ir.functions[0].effects.may_trap, "{expression}");
        let bytecode = crate::bytecode::lower_to_bytecode(&ir).unwrap();
        assert!(
            bytecode.functions[0].metadata.effects.may_trap,
            "{expression}"
        );
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
    let ir = lower_to_ir(&analyzed, &Default::default()).expect("ir lowering should succeed");
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
fn records_ir_source_span_and_local_debug_metadata() {
    let analyzed = common::analyze_ok(
        r#"
fn main(value: i32) -> i32 {
    val next = value + 1;
    next
}
"#,
    );
    let ir = lower_to_ir(&analyzed, &Default::default()).expect("ir lowering should succeed");
    let function = &ir.functions[0];

    assert!(function.debug.source_span.end > function.debug.source_span.start);
    assert!(
        function
            .debug
            .locals
            .iter()
            .any(|local| local.name == "value" && local.is_parameter)
    );
    assert!(
        function
            .debug
            .locals
            .iter()
            .any(|local| local.name == "next" && !local.is_parameter)
    );
    for block in &function.blocks {
        assert_eq!(block.instructions.len(), block.instruction_spans.len());
    }
    assert!(
        function
            .blocks
            .iter()
            .flat_map(|block| block.instruction_spans.iter())
            .any(|span| span.end > span.start)
    );
}

#[test]
fn lowers_if_expression_into_branching_blocks() {
    let analyzed = common::analyze_ok("fn main() -> i32 { if true { 1 } else { 2 } }");
    let ir = lower_to_ir(&analyzed, &Default::default()).expect("ir lowering should succeed");
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
    let ir = lower_to_ir(&analyzed, &Default::default()).expect("ir lowering should succeed");
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
    let ir = lower_to_ir(&analyzed, &Default::default()).expect("ir lowering should succeed");
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
    let ir = lower_to_ir(&analyzed, &Default::default()).expect("ir lowering should succeed");
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
    let ir = lower_to_ir(&analyzed, &Default::default()).expect("ir lowering should succeed");
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
fn lowers_field_and_index_assignments_to_aggregate_writes() {
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
    let ir = lower_to_ir(&analyzed, &Default::default()).expect("ir lowering should succeed");
    let function = &ir.functions[0];

    let instructions = function
        .blocks
        .iter()
        .flat_map(|block| block.instructions.iter())
        .collect::<Vec<_>>();

    assert!(instructions.iter().any(|instruction| matches!(
        instruction,
        Instruction::WriteAggregateField { field, .. } if ir.structure(&field.owner).unwrap().fields[field.slot].name == "x"
    )));
    assert!(
        instructions
            .iter()
            .any(|instruction| matches!(instruction, Instruction::WriteAggregateIndex { .. }))
    );
    assert!(!instructions.iter().any(|instruction| matches!(
        instruction,
        Instruction::Call {
            callee: CallTarget::RuntimeHelper(
                RuntimeHelper::ReflectSetField(_) | RuntimeHelper::ReflectSetIndex
            ),
            ..
        }
    )));
}

#[test]
fn stdlib_lowers_standard_library_calls_to_intrinsic_ids() {
    let analyzed = common::analyze_ok(
        r#"
fn main() -> usize {
    val values = [1, 2];
    values.push(3);
    values.pop();
    values.len()
}
"#,
    );
    let ir = lower_to_ir(&analyzed, &Default::default()).expect("ir lowering should succeed");
    let function = &ir.functions[0];

    assert!(
        function
            .blocks
            .iter()
            .flat_map(|block| block.instructions.iter())
            .any(|instruction| matches!(
                instruction,
                Instruction::Call {
                    callee: CallTarget::StandardIntrinsic(StandardIntrinsic::ArrayPush),
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
                    callee: CallTarget::StandardIntrinsic(StandardIntrinsic::ArrayPop),
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
                    callee: CallTarget::StandardIntrinsic(StandardIntrinsic::ArrayLen),
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
    let ir = lower_to_ir(&analyzed, &Default::default()).expect("ir lowering should succeed");
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
        Instruction::WriteAggregateField { base, value, .. } => vec![*base, *value],
        Instruction::ReadAggregateIndex { dst, base, index } => vec![*dst, *base, *index],
        Instruction::WriteAggregateIndex { base, index, value } => vec![*base, *index, *value],
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
