use crate::{
    source::lower::{instances::MirLoweringOptions, lower_to_mir},
    tests::common,
};
use kagari_common::cancellation::CancellationToken;
use kagari_contract::{effects::EffectSet, operations::BinaryOp};
use kagari_mir::{
    ids::BlockId,
    instruction::{CallTarget, Constant, Instruction, Terminator},
    passes::{PassOptions, PassResult, optimize},
    verify::{MirVerificationErrorKind, VerifiedMirModule, verify_mir},
};

fn checked(source: &str) -> VerifiedMirModule {
    lower_to_mir(&common::analyze_ok(source), &Default::default()).unwrap()
}

fn optimized(module: VerifiedMirModule) -> PassResult {
    optimize(module, &Default::default(), &Default::default()).unwrap()
}

#[test]
fn scalar_folding_removes_dead_points_and_rebuilds_verified_metadata() {
    let input = common::program_ok("fn main() -> i32 { if 1 + 2 == 3 { 42 } else { 0 } }");
    let original = lower_to_mir(input.root(), &Default::default()).unwrap();
    let before = common::bytecode_with_edited_root(&input, &original);
    let result = optimized(original.clone());
    assert!(result.statistics.constants_folded >= 2);
    assert_eq!(result.statistics.branches_simplified, 1);
    assert!(result.statistics.dead_operations_removed >= 3);
    let function = &result.module.functions[0];
    assert!(matches!(
        function.blocks[0].terminator,
        Some(Terminator::Jump(_))
    ));
    assert!(function.blocks[0].instructions.is_empty());
    let before_facts = original.analysis(function.id).unwrap();
    let after_facts = result.module.analysis(function.id).unwrap();
    assert!(function.blocks.iter().enumerate().any(|(index, _)| {
        before_facts.block(BlockId::new(index)).unwrap().reachable()
            && !after_facts.block(BlockId::new(index)).unwrap().reachable()
    }));
    let after = common::bytecode_with_edited_root(&input, &result.module);
    assert!(
        after.modules[after.root.index()].functions[0]
            .instructions
            .len()
            < before.modules[before.root.index()].functions[0]
                .instructions
                .len()
    );
    for block in &function.blocks {
        assert_eq!(block.instructions.len(), block.instruction_spans.len());
        assert_eq!(block.instructions.len(), block.instruction_scopes.len());
    }
    assert_eq!(function.debug.source, original.functions[0].debug.source);
}

#[test]
fn checked_traps_calls_and_allocations_are_not_discarded() {
    for source in [
        "fn main() { 2147483647 + 1; }",
        "fn main() { 1 / 0; }",
        "fn main() { 1i8 << 8; }",
        "fn main() { 127i8 + 1i8; }",
        "fn main() { 18446744073709551615u64 + 1u64; }",
    ] {
        let input = common::program_ok(source);
        let original = lower_to_mir(input.root(), &Default::default()).unwrap();
        let result = optimized(original.clone());
        let before = common::bytecode_with_edited_root(&input, &original);
        let after = common::bytecode_with_edited_root(&input, &result.module);
        assert_eq!(
            before.modules[before.root.index()].functions[0].instructions,
            after.modules[after.root.index()].functions[0].instructions,
            "{source}"
        );
    }
    let input = common::program_ok("fn touch() {} fn main() { touch(); [1, 2]; }");
    let original = lower_to_mir(input.root(), &Default::default()).unwrap();
    let result = optimized(original.clone());
    for (before, after) in original.functions.iter().zip(&result.module.functions) {
        for (before, after) in before.blocks.iter().zip(&after.blocks) {
            for (before, after) in before.instructions.iter().zip(&after.instructions) {
                if before.effects().calls || before.effects().allocates {
                    assert_eq!(format!("{before:?}"), format!("{after:?}"));
                }
            }
        }
    }
}

#[test]
fn non_ssa_redefinitions_and_unavailable_loads_kill_constant_facts() {
    let mut raw =
        checked("fn main(flag: bool) -> i32 { if true { 1 } else { 2 } }").into_unverified();
    let function = &mut raw.functions[0];
    let block = &mut function.blocks[0];
    let Some(Terminator::Branch { cond, .. }) = block.terminator else {
        panic!("branch")
    };
    // The same temporary gets a runtime value after its constant definition.
    block.instructions.push(Instruction::LoadLocal {
        dst: cond,
        local: function.params[0].local,
    });
    block.instruction_spans.push(Default::default());
    block.instruction_scopes.push(0);
    function.effects = function.effects.union(EffectSet::local_read());
    let result = optimized(verify_mir(raw, &Default::default()).unwrap());
    assert_eq!(result.statistics.branches_simplified, 0);
    assert!(matches!(
        result.module.functions[0].blocks[0].terminator,
        Some(Terminator::Branch { .. })
    ));
}

#[test]
fn scalar_facts_do_not_flow_through_heap_reads_or_calls() {
    let result = optimized(checked(
        "fn touch() {} fn main() -> i32 { val x = [1]; touch(); if x[0] == 1 { 2 } else { 3 } }",
    ));
    assert_eq!(result.statistics.branches_simplified, 0);
    assert!(
        result
            .module
            .functions
            .iter()
            .flat_map(|function| &function.blocks)
            .flat_map(|block| &block.instructions)
            .any(|instruction| matches!(instruction, Instruction::Call { callee: CallTarget::SourceFunction(contract), .. } if result.module.definitions().resolve(contract.declaration).unwrap().segments().last().is_some_and(|part| part.name == "index")))
    );
}

#[test]
fn raw_unsigned_arithmetic_is_trapping_even_without_a_result_use() {
    let mut raw = checked("fn main(value: u64) { value + 1u64; }").into_unverified();
    let mut replaced = false;
    for instruction in &mut raw.functions[0].blocks[0].instructions {
        if let Instruction::Binary { op, .. } = instruction {
            *op = BinaryOp::Add;
            replaced = true;
        }
        if let Instruction::Numeric {
            dst,
            lhs,
            rhs: Some(rhs),
            ..
        } = instruction
        {
            *instruction = Instruction::Binary {
                dst: *dst,
                op: BinaryOp::Add,
                lhs: *lhs,
                rhs: *rhs,
            };
            replaced = true;
        }
    }
    assert!(replaced);
    let result = optimized(verify_mir(raw, &Default::default()).unwrap());
    assert!(
        result.module.functions[0].blocks[0]
            .instructions
            .iter()
            .any(|instruction| matches!(
                instruction,
                Instruction::Binary {
                    op: BinaryOp::Add,
                    ..
                }
            ) && instruction.effects().may_trap)
    );
}

#[test]
fn pass_limits_and_cancellation_do_not_publish_partial_modules() {
    let original = checked("fn main() -> i32 { 1 + 2 }");
    let error = optimize(
        original.clone(),
        &PassOptions { max_work: 0 },
        &Default::default(),
    )
    .unwrap_err();
    assert_eq!(
        error.kind,
        MirVerificationErrorKind::Limit {
            resource: "MIR optimization work",
            limit: 0
        }
    );
    let cancel = CancellationToken::default();
    cancel.cancel();
    assert_eq!(
        optimize(original, &Default::default(), &cancel)
            .unwrap_err()
            .kind,
        MirVerificationErrorKind::Cancelled
    );
}

#[test]
fn compiler_options_apply_the_public_frontend_free_pass_pipeline() {
    let source = common::program_ok("fn main() -> i32 { 6 * 7 }");
    let module = lower_to_mir(
        source.root(),
        &MirLoweringOptions {
            optimization: Some(PassOptions::default()),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(
        module.functions[0].blocks[0]
            .instructions
            .iter()
            .any(|instruction| matches!(
                instruction,
                Instruction::LoadConst {
                    constant: Constant::I32(42),
                    ..
                }
            ))
    );
    assert_eq!(module.functions[0].blocks[0].instructions.len(), 1);
    common::bytecode_with_edited_root(&source, &module);
}

#[test]
fn folded_integers_respect_width_signedness_and_shift_rules() {
    for (source, expected) in [
        ("fn main() -> i8 { 126i8 + 1i8 }", Constant::I32(127)),
        ("fn main() -> u8 { 255u8 << 1 }", Constant::I64(254)),
        ("fn main() -> i8 { -128i8 >> 7 }", Constant::I32(-1)),
        (
            "fn main() -> u64 { 18446744073709551614u64 + 1u64 }",
            Constant::U64(u64::MAX),
        ),
    ] {
        let input = common::program_ok(source);
        let result = optimized(lower_to_mir(input.root(), &Default::default()).unwrap());
        let block = &result.module.functions[0].blocks[0];
        let Some(Terminator::Return(Some(value))) = block.terminator else {
            panic!("return: {source}")
        };
        assert!(
            block.instructions.iter().any(|instruction| matches!(
                instruction, Instruction::LoadConst { dst, constant }
                    if *dst == value && *constant == expected
            )),
            "{source}"
        );
        common::bytecode_with_edited_root(&input, &result.module);
    }
}

#[test]
fn calls_invalidate_previously_known_branch_conditions() {
    let mut raw = checked("fn touch() {} fn main() -> i32 { touch(); if true { 1 } else { 2 } }")
        .into_unverified();
    let function = raw.functions.iter_mut().find(|f| f.name == "main").unwrap();
    let block = &mut function.blocks[0];
    let call = block
        .instructions
        .iter()
        .position(|i| matches!(i, Instruction::Call { .. }))
        .unwrap();
    // Move the call after the condition's definition, keeping all origin vectors aligned.
    let instruction = block.instructions.remove(call);
    let span = block.instruction_spans.remove(call);
    let scope = block.instruction_scopes.remove(call);
    block.instructions.push(instruction);
    block.instruction_spans.push(span);
    block.instruction_scopes.push(scope);
    let result = optimized(verify_mir(raw, &Default::default()).unwrap());
    assert_eq!(result.statistics.branches_simplified, 0);
}

#[test]
fn live_float_arithmetic_and_potentially_trapping_casts_are_preserved() {
    for source in [
        "fn main() -> f64 { 1.25 + 2.5 }",
        "fn main(value: i8) { value as i32; }",
    ] {
        let input = common::program_ok(source);
        let original = lower_to_mir(input.root(), &Default::default()).unwrap();
        let result = optimized(original.clone());
        let before = common::bytecode_with_edited_root(&input, &original);
        let after = common::bytecode_with_edited_root(&input, &result.module);
        assert_eq!(
            before.modules[before.root.index()].functions[0].instructions,
            after.modules[after.root.index()].functions[0].instructions,
            "{source}"
        );
    }
}

#[test]
fn local_forwarding_keeps_stores_and_moves_loop_constants_to_entry() {
    let result = optimized(checked(
        "fn main(n: i32) -> i32 { var i = 0; var sum = 0; while i < n { val old = i; sum += old % 7; i = old + 1; } sum }",
    ));
    assert!(result.statistics.copies_removed > 0);
    assert!(result.statistics.constants_reused > 0);
    let function = &result.module.functions[0];
    assert!(
        function
            .blocks
            .iter()
            .flat_map(|block| &block.instructions)
            .any(|instruction| matches!(instruction, Instruction::StoreLocal { .. }))
    );
    for (index, block) in function.blocks.iter().enumerate() {
        if index != function.entry.index() {
            assert!(!block.instructions.iter().any(|instruction| matches!(
                instruction,
                Instruction::LoadConst {
                    constant: Constant::I32(7 | 1),
                    ..
                }
            )));
        }
    }
    assert!(
        function.blocks[function.entry.index()]
            .instructions
            .iter()
            .any(|instruction| matches!(
                instruction,
                Instruction::LoadConst {
                    constant: Constant::I32(7),
                    ..
                }
            ))
    );
}

#[test]
fn cfg_local_snapshots_merge_only_when_every_predecessor_agrees() {
    for source in [
        "fn main(flag: bool, value: i32) -> i32 { var copy = value; if flag { copy + 1; } else { copy + 2; } copy + 3 }",
        "fn main(flag: bool, value: i32) -> i32 { var copy = value; if flag { copy = value + 1; } else { copy = value + 2; } copy + 3 }",
        "fn main(n: i32) -> i32 { var copy = 0; while copy < n { copy += 1; } copy }",
        "fn touch() {} fn main(flag: bool, value: i32) -> i32 { var copy = value; if flag { touch(); } copy + 3 }",
    ] {
        let original = checked(source);
        let result = optimized(original.clone());
        let stores = |module: &VerifiedMirModule| {
            module
                .functions
                .iter()
                .flat_map(|function| &function.blocks)
                .flat_map(|block| &block.instructions)
                .filter(|instruction| matches!(instruction, Instruction::StoreLocal { .. }))
                .count()
        };
        assert_eq!(
            stores(&result.module),
            stores(&original),
            "named debug stores remain: {source}"
        );
    }
    let result = optimized(checked(
        "fn main(flag: bool, value: i32) -> i32 { var copy = value; if flag { copy + 1; } else { copy + 2; } copy + 3 }",
    ));
    let function = &result.module.functions[0];
    assert!(!function.blocks.iter().flat_map(|block| &block.instructions).any(|instruction| matches!(instruction, Instruction::LoadLocal { local, .. } if local.index() == 2)), "a common snapshot dominates both arms and their merge");
}
