use crate::{source::lower::lower_to_mir, tests::common};
use kagari_bytecode::instruction::BytecodeInstruction;
use kagari_common::identity::map::DefinitionContext;
use kagari_mir::ids::BlockId;

#[test]
fn semantic_origins_survive_selection_and_nonzero_entry_order() {
    let context = DefinitionContext::new().unwrap();
    let checked = common::program_ok("fn main() -> i32 { var x = 0; while x < 3 { x += 1; } x }");
    let mut raw = common::adopt(
        &context,
        lower_to_mir(checked.root(), &Default::default())
            .unwrap()
            .into_unverified(),
    );
    let function = &mut raw.functions[0];
    let old_entry = function.entry;
    let new_entry = BlockId::new(function.blocks.len());
    function
        .blocks
        .push(function.blocks[old_entry.index()].clone());
    function.entry = new_entry;
    let verified = common::verify_edited(&context, raw).unwrap();
    let function = &verified.functions[0];
    let analysis = verified.analysis(function.id).unwrap();
    assert_eq!(analysis.block(new_entry).unwrap().start_offset(), 0);
    assert!(analysis.block(old_entry).unwrap().start_offset() > 0);
    let bytecode = common::adopt_bytecode(
        &context,
        common::bytecode_with_edited_root(&checked, &verified),
    );
    let executable = &bytecode.modules[bytecode.root.index()].functions[0];
    let origins = function
        .emission_order()
        .flat_map(|(id, block)| {
            let facts = analysis.block(BlockId::new(id)).unwrap();
            (0..block.instructions.len())
                .map(move |index| {
                    (
                        facts.instruction(index).unwrap(),
                        block.instruction_spans[index],
                    )
                })
                .chain(Some((
                    facts.terminator(),
                    block.terminator_span.unwrap_or_default(),
                )))
        })
        .collect::<Vec<_>>();
    assert_eq!(origins.len(), executable.metadata.origins.len());
    assert!(
        executable.instructions.len() < origins.len(),
        "eligible transport must be selected out"
    );
    let home = executable
        .metadata
        .debug
        .local_live_ranges
        .iter()
        .find(|binding| binding.name == "x")
        .unwrap()
        .slot;
    assert!(
        executable.instructions.iter().any(|instruction| {
            matches!(
                instruction,
                BytecodeInstruction::Binary { .. } | BytecodeInstruction::Numeric { .. }
            ) && instruction.slot_output() == Some(home)
                && instruction.slot_inputs().contains(&home)
        }),
        "compound assignment must read and commit its scalar home directly"
    );
    for (id, (facts, span)) in origins.iter().enumerate() {
        assert_eq!(facts.logical_offset(), id);
        let mapped = &executable.metadata.origins[id];
        assert_eq!(mapped.span, *span);
        assert!(mapped.instruction < executable.instructions.len());
    }
    for instruction in &executable.instructions {
        match instruction {
            BytecodeInstruction::Jump { target } => {
                assert!(target.index() < executable.instructions.len())
            }
            BytecodeInstruction::Branch {
                then_target,
                else_target,
                ..
            } => {
                assert!(then_target.index() < executable.instructions.len());
                assert!(else_target.index() < executable.instructions.len());
            }
            _ => {}
        }
    }
}
