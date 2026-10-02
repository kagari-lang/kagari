use crate::{source::lower::lower_to_mir, tests::common};

use kagari_bytecode::{
    artifact::KbcArtifact, instruction::BytecodeInstruction, program::verify_program,
    verifier::BytecodeVerificationError,
};

use kagari_mir::{
    analysis::SafepointKind,
    instruction::{Constant, Instruction},
    verify::verify_mir,
};

#[test]
fn removed_pure_operations_keep_charge_points_and_origins() {
    let checked = common::program_ok("fn main() -> i32 { 5; 7 }");
    let original = lower_to_mir(checked.root(), &Default::default()).unwrap();
    let before = common::bytecode_with_edited_root(&checked, &original);
    let mut raw = original.into_unverified();
    let function = &mut raw.functions[0];
    let block = &mut function.blocks[function.entry.index()];
    let index = block
        .instructions
        .iter()
        .position(|instruction| {
            matches!(
                instruction,
                Instruction::LoadConst {
                    constant: Constant::I32(5),
                    ..
                }
            )
        })
        .unwrap();
    block.instructions[index] = Instruction::BudgetCheckpoint;
    let verified = verify_mir(raw, &Default::default()).unwrap();
    let function = &verified.functions[0];
    let point = verified
        .analysis(function.id)
        .unwrap()
        .block(function.entry)
        .unwrap()
        .instruction(index)
        .unwrap();
    assert_eq!(point.budget(), LogicalBudgetCharge::Step);
    assert_eq!(point.budget().instruction_steps(), 1);
    assert_eq!(point.safepoint(), SafepointKind::Poll);
    let after = common::bytecode_with_edited_root(&checked, &verified);
    assert!(matches!(
        after.modules[after.root.index()].functions[0].instructions[index],
        BytecodeInstruction::BudgetCheckpoint
    ));
    assert_eq!(
        after.modules[after.root.index()].functions[0]
            .metadata
            .instruction_budgets,
        before.modules[before.root.index()].functions[0]
            .metadata
            .instruction_budgets
    );
    assert_eq!(
        after.modules[after.root.index()].functions[0]
            .metadata
            .debug
            .source_spans,
        before.modules[before.root.index()].functions[0]
            .metadata
            .debug
            .source_spans
    );
    assert_eq!(
        after.modules[after.root.index()].functions[0]
            .metadata
            .debug
            .line_table,
        before.modules[before.root.index()].functions[0]
            .metadata
            .debug
            .line_table
    );
    let artifact = KbcArtifact::from_program(after, Default::default()).unwrap();
    let decoded = KbcArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
    decoded.validate_for_loader(&Default::default()).unwrap();
    assert_eq!(
        decoded.program.modules[decoded.program.root.index()].functions[0]
            .metadata
            .instruction_budgets,
        before.modules[before.root.index()].functions[0]
            .metadata
            .instruction_budgets
    );
    assert!(matches!(
        decoded.program.modules[decoded.program.root.index()].functions[0].instructions[index],
        BytecodeInstruction::BudgetCheckpoint
    ));
}

#[test]
fn missing_or_extra_charges_cannot_create_unbudgeted_execution() {
    let original = common::bytecode_ok("fn main() { while true {} }");
    for extra in [false, true] {
        let mut module = original.clone();
        let charges = &mut module.modules[module.root.index()].functions[0]
            .metadata
            .instruction_budgets;
        if extra {
            charges.push(LogicalBudgetCharge::Step);
        } else {
            charges.pop();
        }
        assert!(matches!(
            verify_program(&module),
            Err(BytecodeVerificationError::MetadataCountMismatch {
                layout: "instruction budgets",
                ..
            })
        ));
    }
    assert!(
        bincode::deserialize::<LogicalBudgetCharge>(&1u32.to_le_bytes()).is_err(),
        "no zero-cost or batched charge can be decoded"
    );
}

#[test]
fn logical_offsets_follow_the_verified_entry_and_bytecode_charge_order() {
    use kagari_mir::ids::BlockId;
    let checked = common::program_ok("fn main() -> i32 { 7 }");
    let mut raw = lower_to_mir(checked.root(), &Default::default())
        .unwrap()
        .into_unverified();
    let function = &mut raw.functions[0];
    function.blocks.push(function.blocks[0].clone());
    function.entry = BlockId::new(1);
    let verified = verify_mir(raw, &Default::default()).unwrap();
    let function = &verified.functions[0];
    let facts = verified.analysis(function.id).unwrap();
    assert_eq!(facts.block(function.entry).unwrap().start_offset(), 0);
    assert!(facts.block(BlockId::new(0)).unwrap().start_offset() > 0);
    let bytecode = common::bytecode_with_edited_root(&checked, &verified);
    let mut offsets = Vec::new();
    for (index, block) in function.emission_order() {
        let facts = facts.block(BlockId::new(index)).unwrap();
        for point in (0..block.instructions.len())
            .map(|index| facts.instruction(index).unwrap())
            .chain(Some(facts.terminator()))
        {
            offsets.push(point.logical_offset());
            assert_eq!(
                point.budget(),
                bytecode.modules[bytecode.root.index()].functions[0]
                    .metadata
                    .instruction_budgets[point.logical_offset()]
            );
        }
    }
    assert_eq!(
        offsets,
        (0..bytecode.modules[bytecode.root.index()].functions[0]
            .instructions
            .len())
            .collect::<Vec<_>>()
    );
}
