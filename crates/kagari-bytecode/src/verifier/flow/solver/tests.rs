use super::*;
use crate::{
    availability::DebugAvailability,
    instruction::Slot,
    module::BytecodeFunction,
    suspension::FlowBudget,
    verifier::flow::{Block, MAX_FLOW_WORK},
};

fn fixture<'a>(
    function: &'a BytecodeFunction,
    budget: &'a mut FlowBudget,
    edges: &[Vec<usize>],
) -> Flow<'a> {
    let blocks = edges
        .iter()
        .enumerate()
        .map(|(i, successors)| Block {
            start: i,
            end: i + 1,
            successors: successors.clone(),
            predecessors: edges
                .iter()
                .enumerate()
                .filter_map(|(p, next)| next.contains(&i).then_some(p))
                .collect(),
            reachable: false,
            uses: vec![if i + 1 == edges.len() { 1 } else { 0 }],
            defines: vec![0],
            initialized: vec![u64::MAX],
            live: vec![0],
        })
        .collect();
    Flow {
        function,
        budget,
        availability: DebugAvailability::new(&[], |_| Ok::<_, ()>(())).unwrap(),
        blocks,
        registers: 3,
        words: 1,
        visits: [0; 2],
    }
}

// Bounded reference only: the original simultaneous fixed point, independent of
// the new queue/order machinery. It deliberately retains whole-graph sweeps.
fn reference(flow: &mut Flow<'_>) -> usize {
    let mut visits = 0;
    loop {
        let mut changed = false;
        for i in 0..flow.blocks.len() {
            if !flow.blocks[i].reachable {
                continue;
            }
            visits += 1;
            let mut initialized = flow.incoming(i);
            let mut live = flow.live_out(i);
            let block = &mut flow.blocks[i];
            for word in 0..flow.words {
                initialized[word] |= block.defines[word];
                live[word] = (live[word] & !block.defines[word]) | block.uses[word];
            }
            changed |= initialized != block.initialized || live != block.live;
            block.initialized = initialized;
            block.live = live;
        }
        if !changed {
            return visits;
        }
    }
}

#[test]
fn directional_facts_match_reference_for_small_cfgs_and_no_exit_cycles() {
    let mut function = BytecodeFunction::default();
    function.metadata.parameter_slots = vec![Slot::new(0)];
    for mask in 0..512 {
        let edges = (0..3)
            .map(|i| {
                (0..3)
                    .filter(|j| mask & (1 << (i * 3 + j)) != 0)
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        let mut left_budget = FlowBudget::new(&Default::default());
        let mut right_budget = FlowBudget::new(&Default::default());
        let mut left = fixture(&function, &mut left_budget, &edges);
        let mut right = fixture(&function, &mut right_budget, &edges);
        for i in 0..3 {
            left.blocks[i].defines[0] = 1 << (i % 2 + 1);
            left.blocks[i].uses[0] |= 1 << ((i + 1) % 3);
            right.blocks[i].defines = left.blocks[i].defines.clone();
            right.blocks[i].uses = left.blocks[i].uses.clone();
        }
        left.reachability().unwrap();
        right.reachability().unwrap();
        left.fixed_points().unwrap();
        reference(&mut right);
        for (a, b) in left.blocks.iter().zip(&right.blocks) {
            assert_eq!(
                (&a.initialized, &a.live),
                (&b.initialized, &b.live),
                "mask {mask}"
            );
        }
    }
}

#[test]
fn chain_work_is_linear_in_both_layout_orders_without_debug_seeding() {
    let mut function = BytecodeFunction::default();
    function.metadata.parameter_slots = vec![Slot::new(0)];
    for count in [16usize, 128, 1024] {
        for reversed in [false, true] {
            let layout = if reversed {
                std::iter::once(0)
                    .chain((1..count).rev())
                    .collect::<Vec<_>>()
            } else {
                (0..count).collect()
            };
            let mut edges = vec![vec![]; count];
            for pair in layout.windows(2) {
                edges[pair[0]].push(pair[1]);
            }
            let mut budget = FlowBudget::new(&Default::default());
            let mut flow = fixture(&function, &mut budget, &edges);
            for block in &mut flow.blocks {
                block.uses[0] = 0;
            }
            flow.blocks[*layout.last().unwrap()].uses[0] = 1;
            flow.reachability().unwrap();
            flow.fixed_points().unwrap();
            assert_eq!(flow.visits, [count, count]);
            assert!(
                flow.blocks
                    .iter()
                    .all(|b| b.live[0] == 1 && b.initialized[0] == 1)
            );
            assert!(flow.budget.work < count * 30);
            println!(
                "flow blocks={count} reversed={reversed} old_forward_chain_visits={} new_visits={} work={}",
                count * (count + 1),
                flow.visits.iter().sum::<usize>(),
                flow.budget.work
            );
        }
    }
}

#[test]
fn graph_scratch_admission_counts_zero_slot_order_and_queue_storage() {
    use crate::verifier::flow::{MAX_FLOW_BYTES, state_bytes};

    let bytes_per_block = state_bytes(1, 0, 0);
    let admitted = MAX_FLOW_BYTES / bytes_per_block;
    assert!(state_bytes(admitted, 0, 0) <= MAX_FLOW_BYTES);
    assert!(state_bytes(admitted + 1, 0, 0) > MAX_FLOW_BYTES);
    assert!(state_bytes(262_144, 0, 0) > MAX_FLOW_BYTES);
    assert_eq!(state_bytes(usize::MAX, usize::MAX, usize::MAX), usize::MAX);
}

#[test]
fn exhaustion_inside_each_direction_stops_a_requeued_cycle() {
    let function = BytecodeFunction::default();
    for direction in [Direction::Initialization, Direction::Liveness] {
        let mut budget = FlowBudget::new(&Default::default());
        let mut flow = fixture(&function, &mut budget, &[vec![1], vec![0]]);
        flow.reachability().unwrap();
        flow.budget.work = MAX_FLOW_WORK - 18;
        let error = flow.propagate(&[1, 0], direction).unwrap_err();
        assert!(matches!(
            error,
            Error::InvalidOperation {
                reason: "executable flow analysis work limit",
                ..
            }
        ));
        match direction {
            Direction::Initialization => {
                assert_eq!(flow.visits, [3, 0]);
                assert!(flow.blocks.iter().all(|block| block.initialized[0] == 0));
            }
            Direction::Liveness => {
                assert_eq!(flow.visits, [0, 3]);
                assert!(flow.blocks.iter().all(|block| block.live[0] == 1));
            }
        }
    }
}

#[test]
fn solver_interrupts_on_exhaustion_and_cancellation_without_a_seal() {
    let function = BytecodeFunction::default();
    let mut budget = FlowBudget::new(&Default::default());
    let mut flow = fixture(&function, &mut budget, &[vec![1], vec![0]]);
    flow.reachability().unwrap();
    flow.budget.work = MAX_FLOW_WORK - 12;
    assert!(flow.fixed_points().is_err());
    flow.budget.work = 0;
    flow.budget.cancel.cancel();
    assert!(matches!(flow.fixed_points(), Err(Error::Cancelled)));
}
