use kagari_abi::representation::ValueType;
use kagari_common::{
    cancellation::CancellationToken,
    identity::{DefinitionKind, ModuleIdentity, map::DefinitionContext},
    span::Span,
};
use kagari_contract::types::ConcreteFunctionIdentity;

use crate::{
    function::{BasicBlock, MirFunction, MirModule},
    ids::{BlockId, InstanceId, TempId},
    instruction::{Constant, Instruction, MirValue, Terminator},
    passes::{Work, compaction},
    verify::MirVerificationErrorKind,
};

fn block(size: usize, origin: usize) -> BasicBlock {
    BasicBlock {
        instructions: (0..size)
            .map(|index| Instruction::LoadConst {
                dst: MirValue {
                    temp: TempId::new(index),
                    ty: ValueType::I32,
                },
                constant: Constant::I32((origin + index) as i32),
            })
            .collect(),
        instruction_spans: (0..size)
            .map(|index| Span::new(origin + index * 2, origin + index * 2 + 1))
            .collect(),
        instruction_scopes: (origin..origin + size).collect(),
        terminator: Some(Terminator::Return(None)),
        terminator_span: Some(Span::new(origin, origin + 1)),
        terminator_scope: Some(origin),
    }
}

fn module() -> MirModule {
    let context = DefinitionContext::new().unwrap();
    let identity = ModuleIdentity::single_file("compaction");
    let root = context.intern_root(&identity).unwrap();
    MirModule {
        native_targets: vec![],
        interface_instances: vec![],
        external_types: vec![],
        dependencies: vec![],
        structures: vec![],
        enumerations: vec![],
        identity,
        source_name: String::new(),
        module_slots: vec![],
        abi: Default::default(),
        functions: (0..2)
            .map(|index| MirFunction {
                semantic: Default::default(),
                id: InstanceId::new(index),
                instance: ConcreteFunctionIdentity {
                    declaration: context
                        .intern_child(root, DefinitionKind::Function, &format!("f{index}"), 0)
                        .unwrap(),
                    arguments: vec![],
                },
                name: format!("f{index}"),
                params: vec![],
                return_type: ValueType::Unit,
                locals: vec![],
                temps: vec![],
                blocks: (0..4).map(|b| block(8, index * 100 + b * 10)).collect(),
                entry: BlockId::new(0),
                effects: Default::default(),
                debug: Default::default(),
            })
            .collect(),
    }
}

fn origins(block: &BasicBlock) -> Vec<(usize, i32, Span, usize)> {
    assert_eq!(block.instructions.len(), block.instruction_spans.len());
    assert_eq!(block.instructions.len(), block.instruction_scopes.len());
    block
        .instructions
        .iter()
        .zip(&block.instruction_spans)
        .zip(&block.instruction_scopes)
        .map(|((instruction, &span), &scope)| match instruction {
            Instruction::LoadConst {
                dst,
                constant: Constant::I32(value),
            } => (dst.temp.index(), *value, span, scope),
            _ => panic!("fixture instruction"),
        })
        .collect()
}

#[test]
fn grouped_compaction_preserves_shapes_and_exact_provenance() {
    let mut module = module();
    let original = module.clone();
    // Match the finder's group order and descending indices within each group.
    let shapes = [
        vec![],
        vec![7, 6, 5, 4, 3, 2, 1, 0],
        vec![3, 2, 1, 0],
        vec![6, 4, 2, 0],
    ];
    let removals: Vec<_> = (0..2)
        .flat_map(|function| {
            shapes.iter().enumerate().flat_map(move |(block, indices)| {
                indices.iter().map(move |&index| (function, block, index))
            })
        })
        .collect();
    let cancel = CancellationToken::default();
    let mut work = Work {
        used: 0,
        limit: 1000,
        cancel: &cancel,
    };
    compaction::apply(&mut module, &removals, &mut work).unwrap();
    for (function, original) in module.functions.iter().zip(&original.functions) {
        for (index, (block, before)) in function.blocks.iter().zip(&original.blocks).enumerate() {
            let expected: Vec<_> = origins(before)
                .into_iter()
                .enumerate()
                .filter(|(position, _)| !shapes[index].contains(position))
                .map(|(_, origin)| origin)
                .collect();
            assert_eq!(origins(block), expected);
            assert_eq!(
                format!("{:?}", block.terminator),
                format!("{:?}", before.terminator)
            );
            assert_eq!(block.terminator_span, before.terminator_span);
            assert_eq!(block.terminator_scope, before.terminator_scope);
        }
    }
    // Per function: 16 preparation + 24 visits + 48 relocations + 48 tail entries.
    assert_eq!(work.used, 2 * 136);
}

#[test]
fn no_removals_do_not_visit_blocks_or_allocate_scratch() {
    let mut module = module();
    let cancel = CancellationToken::default();
    let mut work = Work {
        used: 0,
        limit: 0,
        cancel: &cancel,
    };
    compaction::apply(&mut module, &[], &mut work).unwrap();
    assert_eq!(work.used, 0);
    assert!(module.functions.iter().all(|function| {
        function
            .blocks
            .iter()
            .all(|block| block.instructions.len() == 8)
    }));
}

#[test]
fn exhaustion_charges_preparation_retained_visits_moves_and_tail_cleanup() {
    let cancel = CancellationToken::default();
    let mut module = module();
    let mut work = Work {
        used: 0,
        limit: 1,
        cancel: &cancel,
    };
    assert!(matches!(
        compaction::apply(&mut module, &[(0, 0, 1), (0, 0, 0)], &mut work)
            .unwrap_err()
            .kind,
        MirVerificationErrorKind::Limit { limit: 1, .. }
    ));
    assert_eq!(
        origins(&module.functions[0].blocks[0]),
        origins(&block(8, 0))
    );

    // A first deletion has been consumed and a retained entry already moved.
    let mut partial = block(8, 0);
    let mut work = Work {
        used: 0,
        limit: 9,
        cancel: &cancel,
    };
    assert!(matches!(
        compaction::compact_block(&mut partial, [0].into_iter(), &mut work)
            .unwrap_err()
            .kind,
        MirVerificationErrorKind::Limit { limit: 9, .. }
    ));
    assert_eq!(origins(&partial)[0], origins(&block(8, 0))[1]);
    assert_eq!(partial.instructions.len(), 8);

    // All keep decisions succeed; the dead tail is charged before destruction.
    let mut partial = block(1024, 0);
    let mut work = Work {
        used: 0,
        limit: 1024 + 3 * 256,
        cancel: &cancel,
    };
    assert!(matches!(
        compaction::compact_block(&mut partial, 0..1024, &mut work)
            .unwrap_err()
            .kind,
        MirVerificationErrorKind::Limit { .. }
    ));
    assert_eq!(partial.instructions.len(), 768);
    assert_eq!(partial.instruction_spans.len(), 768);
    assert_eq!(partial.instruction_scopes.len(), 768);
}

#[test]
fn cancellation_after_retained_moves_stops_inside_compaction() {
    let cancel = CancellationToken::default();
    let mut partial = block(1024, 0);
    // The production iterator is lazy too. Cancel only after two dead entries
    // have been reached, with 299 retained entries moved in between.
    let removals = [0, 300, 600].into_iter().inspect(|&index| {
        if index == 600 {
            cancel.cancel();
        }
    });
    let mut work = Work {
        used: 0,
        limit: usize::MAX,
        cancel: &cancel,
    };
    assert_eq!(
        compaction::compact_block(&mut partial, removals, &mut work)
            .unwrap_err()
            .kind,
        MirVerificationErrorKind::Cancelled
    );
    assert_eq!(origins(&partial)[0], origins(&block(1024, 0))[1]);
    assert_eq!(work.used, 302 + 299 * 6);
    assert_eq!(partial.instructions.len(), 1024);
}

#[test]
fn prefix_compaction_work_and_relocations_scale_linearly() {
    let cancel = CancellationToken::default();
    for size in [16, 256, 4096] {
        let mut partial = block(size, 0);
        let mut work = Work {
            used: 0,
            limit: usize::MAX,
            cancel: &cancel,
        };
        compaction::compact_block(&mut partial, 0..size / 2, &mut work).unwrap();
        // One decision per entry, six relocations per moved survivor and three
        // removed tail entries per deletion. Group preparation adds size / 2.
        assert_eq!(work.used + size / 2, 6 * size);
        assert_eq!(origins(&partial), origins(&block(size, 0))[size / 2..]);
    }
}
