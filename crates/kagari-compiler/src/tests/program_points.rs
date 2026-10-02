use crate::{source::lower::lower_to_mir, tests::common};

use kagari_mir::verify::verify_mir;

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
