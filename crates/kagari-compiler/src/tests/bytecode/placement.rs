//! General placement contracts over optimized MIR, not a benchmark-name shortcut.
use crate::{
    bytecode::lower_program_to_bytecode,
    native_input::verify_native_input,
    source::{lower::instances::MirLoweringOptions, program::lower_program_to_mir},
    tests::common,
};
use kagari_bytecode::instruction::BytecodeInstruction as I;
use kagari_mir::codec::encode_program;
use rstest::rstest;

#[rstest]
#[case::recurrence(
        "fn recurrence(n: i32) -> i32 { var state = 2; var sum = 0; var i = 0;
         while i < n { state = (state * 5 + 11) % 257; sum += state; i += 1; } sum }",
    &["n", "state", "sum", "i"],
    None
)]
#[case::counter_snapshot(
    "fn recurrence(n: i32) -> i32 { var sum = 0; var i = 0;
     while i < n { sum = sum + i % 13; i = i + 1; } sum }",
    &["n", "sum", "i"],
    Some((4, 8))
)]
fn scalar_recurrence_commits_to_homes_without_transport_instructions(
    #[case] source: &str,
    #[case] bindings: &[&str],
    #[case] shape: Option<(u32, usize)>,
) {
    let input = common::program_ok(source);
    let mir = lower_program_to_mir(
        &input,
        &MirLoweringOptions {
            optimization: Some(Default::default()),
            ..Default::default()
        },
    )
    .unwrap();
    let code = lower_program_to_bytecode(&mir, &Default::default()).unwrap();
    let wire = encode_program(&mir, &Default::default()).unwrap();
    verify_native_input(&wire, &code, &Default::default()).unwrap();
    let program = code.program();
    let function = program.modules[program.root.index()]
        .functions
        .iter()
        .find(|function| function.name == "recurrence")
        .unwrap();
    if let Some((slots, instructions)) = shape {
        assert_eq!(function.slot_count, slots);
        assert_eq!(function.instructions.len(), instructions);
    }
    assert!(
        function
            .instructions
            .iter()
            .all(|instruction| !matches!(instruction, I::Move { .. })),
        "eligible scalar initialization, multi-use assignment and return need no transport: {:?}",
        function.instructions
    );
    assert!(function.instructions.iter().enumerate().any(|(pc, instruction)| {
        matches!(instruction, I::PredicateBranch { then_target, .. } if then_target.index() < pc)
    }), "the bounded condition belongs on a conditional backedge");
    assert!(
        function
            .instructions
            .iter()
            .enumerate()
            .all(|(pc, instruction)| {
                !matches!(instruction, I::Jump { target } if target.index() == pc + 1)
            })
    );
    for binding in bindings {
        assert!(
            function
                .metadata
                .debug
                .local_live_ranges
                .iter()
                .any(|range| range.name == *binding),
            "missing visible binding {binding}"
        );
    }
}
