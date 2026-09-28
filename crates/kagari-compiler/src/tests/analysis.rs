use crate::{bytecode::lower_to_bytecode, lower_to_mir, tests::common};
use kagari_mir::{
    BlockId, Instruction, LocalId, MirModule, TempId, VerifiedMirModule, analysis::SafepointKind,
    verify_mir,
};

fn checked(source: &str) -> VerifiedMirModule {
    lower_to_mir(&common::analyze_ok(source), &Default::default()).unwrap()
}

fn seal(module: MirModule) -> VerifiedMirModule {
    verify_mir(module, &Default::default()).unwrap()
}

#[test]
fn call_operands_and_visible_heap_locals_are_roots_before_the_result_exists() {
    let module = checked(
        "fn echo(x: (i32, bool)) -> (i32, bool) { x } fn main() -> (i32, bool) { val visible = (1, true); echo((2, false)) }",
    );
    let function = module
        .functions
        .iter()
        .find(|function| function.name == "main")
        .unwrap();
    let facts = module.analysis(function.id).unwrap();
    let local = function
        .debug
        .locals
        .iter()
        .find(|local| local.name == "visible")
        .unwrap()
        .local;
    let mut saw_call = false;
    let mut saw_store = false;
    for (id, block) in function.blocks.iter().enumerate() {
        let facts = facts.block(BlockId::new(id)).unwrap();
        for (index, instruction) in block.instructions.iter().enumerate() {
            let point = facts.instruction(index).unwrap();
            match instruction {
                Instruction::Call {
                    dst: Some(dst),
                    args,
                    ..
                } => {
                    saw_call = true;
                    assert_eq!(point.safepoint(), SafepointKind::Runtime);
                    assert!(point.debug_available().contains_local(local));
                    assert!(point.roots().contains_local(local));
                    assert!(args.iter().all(|arg| point.roots().contains_temp(arg.temp)));
                    assert!(!point.live().contains_temp(dst.temp));
                    assert!(!point.roots().contains_temp(dst.temp));
                    assert!(facts.terminator().roots().contains_temp(dst.temp));
                }
                Instruction::StoreLocal { local: stored, src } if *stored == local => {
                    saw_store = true;
                    assert!(!point.debug_available().contains_local(local));
                    assert!(!point.roots().contains_local(local));
                    assert!(point.roots().contains_temp(src.temp));
                }
                _ => {}
            }
        }
    }
    assert!(saw_call && saw_store);
}

#[test]
fn loops_keep_heap_values_live_and_do_not_root_scalar_slots() {
    let module = checked(
        "fn main(flag: bool) -> (i32, bool) { val keep = (1, true); var n = 0; while n < 3 { n += 1; } if flag { keep } else { (2, false) } }",
    );
    let mut raw = module.into_unverified();
    // Hide non-parameter locals from the debugger so the backedge roots must
    // follow script reads after the loop, rather than lexical visibility.
    for block in &mut raw.functions[0].blocks {
        block.instruction_scopes.fill(0);
        block.terminator_scope = Some(0);
    }
    let module = seal(raw);
    let function = &module.functions[0];
    let facts = module.analysis(function.id).unwrap();
    let keep = function
        .debug
        .locals
        .iter()
        .find(|local| local.name == "keep")
        .unwrap()
        .local;
    let mut backedges = 0;
    for (id, block) in function.blocks.iter().enumerate() {
        let terminator = block.terminator.as_ref().unwrap();
        if terminator
            .successors()
            .iter()
            .any(|target| target.index() <= id)
        {
            backedges += 1;
            let point = facts.block(BlockId::new(id)).unwrap().terminator();
            assert!(point.live().contains_local(keep));
            assert!(point.roots().contains_local(keep));
            assert!(!point.roots().contains_local(LocalId::new(0)));
            assert_eq!(point.safepoint(), SafepointKind::ControlFlow);
        }
    }
    assert!(backedges > 0);
}

#[test]
fn debugger_availability_requires_initialization_on_every_incoming_edge() {
    let mut module = checked("fn main(flag: bool) { if flag { val branch_only = \"value\"; } }")
        .into_unverified();
    let function = &mut module.functions[0];
    let local = function
        .debug
        .locals
        .iter()
        .find(|local| local.name == "branch_only")
        .unwrap()
        .local;
    let scope = function
        .debug
        .lexical_scopes
        .iter()
        .position(|scope| scope.local == Some(local))
        .unwrap();
    // Metadata is not proof of initialization. Deliberately extend this scope
    // across the join, where the false branch never initialized the local.
    let join = function
        .blocks
        .iter()
        .position(|block| matches!(block.terminator, Some(kagari_mir::Terminator::Return(_))))
        .unwrap();
    function.blocks[join].terminator_scope = Some(scope);
    let module = seal(module);
    let facts = module.analysis(module.functions[0].id).unwrap();
    assert!(
        !facts
            .block(BlockId::new(join))
            .unwrap()
            .terminator()
            .debug_available()
            .contains_local(local)
    );
    let bytecode = lower_to_bytecode(&module).unwrap();
    let offset = module.functions[0].blocks[..join]
        .iter()
        .map(|block| block.instructions.len() + 1)
        .sum::<usize>()
        + module.functions[0].blocks[join].instructions.len();
    assert!(
        bytecode.functions[0]
            .metadata
            .debug
            .local_live_ranges
            .iter()
            .filter(|range| range.local.index() == local.index())
            .all(|range| !(range.start <= offset && offset < range.end))
    );
}

#[test]
fn unreachable_points_have_no_live_values_or_debugger_availability() {
    let mut module = checked("fn main(x: String) -> String { x }").into_unverified();
    let function = &mut module.functions[0];
    let dead = BlockId::new(function.blocks.len());
    function.blocks.push(function.blocks[0].clone());
    let module = seal(module);
    let facts = module
        .analysis(module.functions[0].id)
        .unwrap()
        .block(dead)
        .unwrap();
    assert!(!facts.reachable());
    for point in [facts.instruction(0).unwrap(), facts.terminator()] {
        assert_eq!(point.live().locals().count(), 0);
        assert_eq!(point.live().temps().count(), 0);
        assert_eq!(point.roots().locals().count(), 0);
        assert_eq!(point.debug_available().locals().count(), 0);
        assert!(!point.live().contains_temp(TempId::new(usize::MAX)));
        assert!(!point.live().contains_local(LocalId::new(usize::MAX)));
    }
}

#[test]
fn editing_a_raw_module_recomputes_scope_roots_on_reverification() {
    let mut raw = checked("fn main() { val visible = (1, true); }").into_unverified();
    let function = &mut raw.functions[0];
    let scope = function
        .debug
        .lexical_scopes
        .iter()
        .position(|scope| scope.local == Some(function.debug.locals[0].local))
        .unwrap();
    function.blocks[function.entry.index()].terminator_scope = Some(scope);
    let original = seal(raw);
    let function = &original.functions[0];
    let id = function.id;
    let block = function.entry;
    let local = function.debug.locals[0].local;
    assert!(
        original
            .analysis(id)
            .unwrap()
            .block(block)
            .unwrap()
            .terminator()
            .roots()
            .contains_local(local)
    );
    let mut raw = original.clone().into_unverified();
    raw.functions[id.index()].blocks[block.index()].terminator_scope = Some(0);
    let changed = seal(raw);
    assert!(
        !changed
            .analysis(id)
            .unwrap()
            .block(block)
            .unwrap()
            .terminator()
            .roots()
            .contains_local(local)
    );
    assert!(
        original
            .analysis(id)
            .unwrap()
            .block(block)
            .unwrap()
            .terminator()
            .roots()
            .contains_local(local)
    );
}

#[test]
fn scalar_temporaries_die_after_their_last_use() {
    let module = checked("fn main() -> i32 { 1 + 2 }");
    let function = &module.functions[0];
    let block = &function.blocks[function.entry.index()];
    let facts = module
        .analysis(function.id)
        .unwrap()
        .block(function.entry)
        .unwrap();
    let (index, lhs, rhs, dst) = block
        .instructions
        .iter()
        .enumerate()
        .find_map(|(index, instruction)| {
            if let Instruction::Binary { lhs, rhs, dst, .. } = instruction {
                Some((index, lhs.temp, rhs.temp, dst.temp))
            } else {
                None
            }
        })
        .unwrap();
    let before = facts.instruction(index).unwrap();
    assert!(before.live().contains_temp(lhs));
    assert!(before.live().contains_temp(rhs));
    assert!(!before.live().contains_temp(dst));
    assert_eq!(before.roots().temps().count(), 0);
    let after = facts.terminator();
    assert!(!after.live().contains_temp(lhs));
    assert!(!after.live().contains_temp(rhs));
    assert!(after.live().contains_temp(dst));
}

#[test]
fn point_analysis_storage_is_bounded_before_allocation() {
    use kagari_abi::representation::ValueType;
    use kagari_mir::{MirTemp, MirVerificationErrorKind};
    let mut module = checked("fn main() -> i32 { 1 }").into_unverified();
    let function = &mut module.functions[0];
    function.temps.resize(
        u16::MAX as usize,
        MirTemp {
            ty: ValueType::Unit,
        },
    );
    let block = &mut function.blocks[function.entry.index()];
    block
        .instructions
        .resize(3000, block.instructions[0].clone());
    block
        .instruction_spans
        .resize(3000, block.instruction_spans[0]);
    block
        .instruction_scopes
        .resize(3000, block.instruction_scopes[0]);
    assert_eq!(
        verify_mir(module, &Default::default()).unwrap_err().kind,
        MirVerificationErrorKind::Limit {
            resource: "MIR analysis state bytes",
            limit: 64 * 1024 * 1024,
        }
    );
}

#[test]
fn parameter_debug_metadata_cannot_be_replaced_by_an_unscoped_local() {
    use kagari_mir::MirVerificationErrorKind;
    let mut module = checked("fn main(x: i32) -> i32 { val y = x; y }").into_unverified();
    let function = &mut module.functions[0];
    function.debug.locals.retain(|local| !local.is_parameter);
    function.debug.lexical_scopes.truncate(1);
    for block in &mut function.blocks {
        block.instruction_scopes.fill(0);
        block.terminator_scope = Some(0);
    }
    assert_eq!(
        verify_mir(module, &Default::default()).unwrap_err().kind,
        MirVerificationErrorKind::InvalidDebugMetadata
    );
}
