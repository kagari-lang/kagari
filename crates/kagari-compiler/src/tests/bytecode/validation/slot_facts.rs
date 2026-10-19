use super::replace_fixture_code;
use crate::tests::bytecode::*;
use kagari_bytecode::{
    artifact::KbcArtifact,
    instruction::{BinaryOp, ConstantId, ConstantOperand, ScalarOperand, Slot},
    module::RootSlotLayout,
    program::{BytecodeProgram, verify_program},
};
use kagari_contract::callable::generic::GenericBody;
use kagari_types::{
    scalar::BuiltinType,
    ty::{GenericParam, Ty, substitution::MAX_TYPE_DEPTH},
};

fn verify_source_free_fixture(
    context: &DefinitionContext,
    valid: &BytecodeProgram,
    forged: BytecodeProgram,
    expected: Result<(), BytecodeVerificationError>,
) {
    assert_eq!(verify_program(&forged, &context.snapshot()), expected);
    let base = if expected.is_ok() {
        forged.clone()
    } else {
        valid.clone()
    };
    let mut artifact = KbcArtifact::from_program(base, Default::default(), context).unwrap();
    artifact.program = forged;
    let decoded = KbcArtifact::from_bytes(&artifact.to_bytes(&context.snapshot()).unwrap())
        .map(|metadata| common::adopt(context, metadata))
        .unwrap();
    match decoded.validate_for_loader(&Default::default(), context) {
        Ok(()) => assert_eq!(expected, Ok(())),
        Err(ArtifactValidationError::Bytecode(error)) => assert_eq!(Err(error), expected),
        Err(error) => panic!("unexpected loader failure: {error}"),
    }
}

#[test]
fn scalar_constant_forms_reject_missing_constants_ranges_width_erasure_and_uninitialized_slots() {
    let context = DefinitionContext::new().unwrap();
    let valid = common::adopt_bytecode(
        &context,
        common::bytecode_ok("fn probe(x: i8) -> i8 { x + 1i8 } fn main() {}"),
    );
    for mutation in 0..5 {
        let mut forged = valid.clone();
        let module = &mut forged.modules[forged.root.index()];
        let function = module
            .functions
            .iter_mut()
            .find(|function| function.name == "probe")
            .unwrap();
        function.metadata.debug = Default::default();
        let scratch = Slot::new(function.metadata.slots.len());
        if mutation == 4 {
            function.metadata.slots.push(ValueType::I32);
            function
                .metadata
                .semantic
                .slots
                .insert(scratch.index(), Ty::Builtin(BuiltinType::I8));
            function.slot_count += 1;
        }
        let instruction = function
            .instructions
            .iter_mut()
            .find(|instruction| matches!(instruction, BytecodeInstruction::BinaryOperands { .. }))
            .unwrap();
        let BytecodeInstruction::BinaryOperands { op, lhs, rhs, .. } = instruction else {
            unreachable!()
        };
        let ScalarOperand::Constant { constant, ty } = rhs else {
            panic!("canonical constant rhs")
        };
        match mutation {
            0 => *constant = ConstantId::new(u32::MAX as usize),
            1 => module.constants[constant.index()] = ConstantOperand::I32(128),
            2 => *ty = BuiltinType::I16,
            3 => *op = BinaryOp::Add,
            _ => *lhs = ScalarOperand::Slot(scratch),
        }
        let reason = match mutation {
            0 | 1 => "invalid scalar constant operand",
            2 | 3 => "invalid collection access flow",
            _ => "uninitialized executable slot",
        };
        let expected = Err(BytecodeVerificationError::InvalidOperation {
            function: function.id,
            reason,
        });
        verify_source_free_fixture(&context, &valid, forged, expected);
    }
}

#[test]
fn predicate_branches_verify_operation_targets_and_initialization_without_source() {
    let context = DefinitionContext::new().unwrap();
    let valid = common::adopt_bytecode(
        &context,
        common::bytecode_ok(
            "fn probe(x: i32) -> i32 { if x < 1 { x } else { x + 2 } } fn main() {}",
        ),
    );
    for mutation in 0..3 {
        let mut forged = valid.clone();
        let function = forged.modules[forged.root.index()]
            .functions
            .iter_mut()
            .find(|function| function.name == "probe")
            .unwrap();
        function.metadata.debug = Default::default();
        let target = JumpTarget::new(function.instructions.len());
        let scratch = Slot::new(function.metadata.slots.len());
        if mutation == 2 {
            function.metadata.slots.push(ValueType::I32);
            function.slot_count += 1;
        }
        let instruction = function
            .instructions
            .iter_mut()
            .find(|instruction| matches!(instruction, BytecodeInstruction::PredicateBranch { .. }))
            .unwrap();
        let BytecodeInstruction::PredicateBranch {
            op,
            lhs,
            then_target,
            ..
        } = instruction
        else {
            unreachable!()
        };
        match mutation {
            0 => *op = BinaryOp::Add,
            1 => *then_target = target,
            _ => *lhs = ScalarOperand::Slot(scratch),
        }
        let error = match mutation {
            0 => BytecodeVerificationError::InvalidOperation {
                function: function.id,
                reason: "invalid branch predicate",
            },
            1 => BytecodeVerificationError::InvalidJumpTarget {
                function: function.id,
                target,
            },
            _ => BytecodeVerificationError::InvalidOperation {
                function: function.id,
                reason: "uninitialized executable slot",
            },
        };
        verify_source_free_fixture(&context, &valid, forged, Err(error));
    }
}

#[test]
fn inferred_tuple_node_and_depth_limits_apply_before_convergence() {
    let context = DefinitionContext::new().unwrap();
    let valid = common::adopt_bytecode(
        &context,
        common::bytecode_ok("fn probe(seed: i32) {} fn main() {}"),
    );
    for width in [1, 2] {
        let mut forged = valid.clone();
        let module = &mut forged.modules[forged.root.index()];
        module.types.push(ValueType::HeapObject);
        let function = module
            .functions
            .iter_mut()
            .find(|function| function.name == "probe")
            .unwrap();
        let seed = function.metadata.parameter_slots[0];
        let tuple = Slot::new(function.metadata.slots.len());
        function.metadata.slots.push(ValueType::HeapObject);
        function.slot_count += 1;
        function.metadata.roots = RootSlotLayout::from_types(&function.metadata.slots);
        let mut code = (0..=MAX_TYPE_DEPTH)
            .map(|level| BytecodeInstruction::MakeTuple {
                dst: tuple,
                elements: vec![if level == 0 { seed } else { tuple }; width],
            })
            .collect::<Vec<_>>();
        code.push(BytecodeInstruction::Return(None));
        replace_fixture_code(function, code);
        let expected = Err(BytecodeVerificationError::InvalidOperation {
            function: function.id,
            reason: "access flow type shape limit",
        });
        verify_source_free_fixture(&context, &valid, forged, expected);
    }
}

#[test]
fn inferred_type_storage_is_bounded_but_cfg_copies_share_charged_trees() {
    let context = DefinitionContext::new().unwrap();
    let source = format!(
        "fn probe(seed: ({})) {{}} fn main() {{}}",
        vec!["i32"; 256].join(",")
    );
    let valid = common::adopt_bytecode(&context, common::bytecode_ok(&source));
    for share in [false, true] {
        let mut forged = valid.clone();
        let function = forged.modules[forged.root.index()]
            .functions
            .iter_mut()
            .find(|function| function.name == "probe")
            .unwrap();
        let seed = function.metadata.parameter_slots[0];
        let tuple = Slot::new(function.metadata.slots.len());
        function.metadata.slots.push(ValueType::HeapObject);
        function.slot_count += 1;
        function.metadata.roots = RootSlotLayout::from_types(&function.metadata.slots);
        let mut code = Vec::new();
        for block in 0..if share { 4096 } else { 512 } {
            code.push(if share {
                BytecodeInstruction::Move {
                    dst: tuple,
                    src: seed,
                }
            } else {
                BytecodeInstruction::MakeTuple {
                    dst: tuple,
                    elements: vec![seed; 8],
                }
            });
            code.push(BytecodeInstruction::Jump {
                target: JumpTarget::new(2 * block + 2),
            });
        }
        code.push(BytecodeInstruction::Return(None));
        replace_fixture_code(function, code);
        let expected = if share {
            Ok(())
        } else {
            Err(BytecodeVerificationError::InvalidOperation {
                function: function.id,
                reason: "access flow state limit",
            })
        };
        verify_source_free_fixture(&context, &valid, forged, expected);
    }
}

#[test]
fn unused_generic_parameters_cannot_hide_a_scalar_layout_by_omitting_the_home_type() {
    let context = DefinitionContext::new().unwrap();
    let mut valid = common::adopt_bytecode(
        &context,
        common::bytecode_ok("fn probe(value: i32) {} fn main() {}"),
    );
    let module = &mut valid.modules[valid.root.index()];
    module.types.push(ValueType::Generic);
    let function = module
        .functions
        .iter_mut()
        .find(|function| function.name == "probe")
        .unwrap();
    let parameter = GenericParam {
        owner: function.identity.as_ref().unwrap().declaration,
        position: 0,
    };
    let home = function.metadata.parameter_slots[0];
    function.metadata.semantic.generic = Some(GenericBody {
        parameters: vec![parameter.clone()],
        bounds: vec![],
    });
    function
        .metadata
        .semantic
        .params
        .insert(0, parameter.as_type());
    function
        .metadata
        .semantic
        .slots
        .insert(home.index(), parameter.as_type());
    function.metadata.params[0] = ValueType::Generic;
    function.metadata.slots[home.index()] = ValueType::Generic;
    function.metadata.roots = RootSlotLayout::from_types(&function.metadata.slots);
    replace_fixture_code(function, vec![BytecodeInstruction::Return(None)]);
    let id = function.id;
    module.function_table[id.index()].params[0] = ValueType::Generic;
    verify_source_free_fixture(&context, &valid, valid.clone(), Ok(()));
    let mut forged = valid.clone();
    let module = &mut forged.modules[forged.root.index()];
    let function = &mut module.functions[id.index()];
    function.metadata.semantic.slots.remove(&home.index());
    function.metadata.params[0] = ValueType::I32;
    function.metadata.slots[home.index()] = ValueType::I32;
    function.metadata.roots = RootSlotLayout::from_types(&function.metadata.slots);
    module.function_table[id.index()].params[0] = ValueType::I32;
    let expected = Err(BytecodeVerificationError::InvalidOperation {
        function: id,
        reason: "invalid collection access flow",
    });
    verify_source_free_fixture(&context, &valid, forged, expected);
}
