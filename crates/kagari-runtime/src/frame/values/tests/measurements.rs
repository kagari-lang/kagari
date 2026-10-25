//! Fixed storage controls, distinct from full owned-session lifecycle measurements.
use super::module;
use crate::{
    frame::{
        arguments::FrameArguments,
        values::{ExecutionValues, FrameMetadata, FrameSlots},
    },
    module::{
        ExecutableModule,
        execution::{
            OperandSlot,
            layout::{FrameLayout, Location},
        },
    },
    value::Value,
};
use kagari_abi::representation::ValueType;
use std::sync::Arc;

#[cfg(not(feature = "execution-diagnostics"))]
use std::{hint::black_box, time::Instant};

#[cfg(feature = "execution-diagnostics")]
use crate::diagnostics::{self, allocations};

const CASES: [(usize, usize, &str); 6] = [
    (1, 0, "tail"),
    (32, 8, "tail"),
    (256, 128, "tail"),
    (256, 0, "random"),
    (256, 8, "forward"),
    (256, 128, "random"),
];

struct Control {
    depth: usize,
    width: usize,
    order: Vec<usize>,
    layout: Arc<FrameLayout>,
    arguments: Vec<Value>,
}

impl Control {
    fn new(depth: usize, width: usize, order: &str) -> Self {
        let locations = (0..width)
            .map(|index| Location {
                operand: OperandSlot::new(index / 2, index % 2 == 1),
                representation: if index % 2 == 1 {
                    ValueType::Generic
                } else {
                    ValueType::I32
                },
                semantic: None,
                borrowed_receiver: false,
            })
            .collect();
        let layout = Arc::new(FrameLayout {
            locations,

            reused_slots: Box::default(),
            count: width,
            scalar_count: width / 2,
            managed_count: width / 2,
        });
        let mut indices: Vec<_> = (0..depth).collect();
        match order {
            "tail" => indices.reverse(),
            "random" => {
                let mut state = 0x4b41_4741_5249_u64;
                for index in (1..depth).rev() {
                    state ^= state << 13;
                    state ^= state >> 7;
                    state ^= state << 17;
                    indices.swap(index, (state % (index + 1) as u64) as usize);
                }
            }
            "forward" => {}
            _ => panic!("unknown control order"),
        }
        Self {
            depth,
            width,
            order: indices,
            layout,
            arguments: vec![Value::I32(7); width],
        }
    }

    fn allocate(&self, values: &mut ExecutionValues, module: &ExecutableModule) -> FrameSlots {
        values
            .allocate(
                self.width,
                &(0..self.arguments.len()).collect::<Vec<_>>(),
                &FrameArguments::plain(&self.arguments),
                FrameMetadata {
                    program: module.clone(),
                    environment: None,
                    invocation: None,
                },
                Some(self.layout.clone()),
            )
            .unwrap()
    }

    fn fill(
        &self,
        values: &mut ExecutionValues,
        module: &ExecutableModule,
        slots: &mut Vec<FrameSlots>,
    ) {
        for _ in 0..self.depth {
            slots.push(self.allocate(values, module));
        }
    }

    fn fragment(
        &self,
        values: &mut ExecutionValues,
        module: &ExecutableModule,
        slots: &mut [FrameSlots],
    ) {
        for index in (0..self.depth).step_by(2) {
            values.release(slots[index]).unwrap();
        }
        for index in (0..self.depth).step_by(2) {
            slots[index] = self.allocate(values, module);
        }
    }

    fn retire(&self, values: &mut ExecutionValues, slots: &mut Vec<FrameSlots>) {
        for &index in &self.order {
            values.release(slots[index]).unwrap();
        }
        slots.clear();
        assert_eq!(values.active_windows(), 0);
        assert!(
            values.values.is_empty() && values.payloads.is_empty() && values.initialized.is_empty()
        );
    }
}

#[test]
#[ignore = "manual release-profile storage timing; never run concurrently with builds or diagnostics"]
#[cfg(not(feature = "execution-diagnostics"))]
fn window_storage_timing() {
    let (_runtime, _program, module) = module();
    for (depth, width, order) in CASES {
        let control = Control::new(depth, width, order);
        let mut values = ExecutionValues::default();
        let mut slots = Vec::with_capacity(depth);
        for sample in 0..14 {
            let mut totals = [0_u128; 3];
            for _ in 0..32 {
                let started = Instant::now();
                control.fill(&mut values, &module, &mut slots);
                totals[0] += started.elapsed().as_nanos();
                let started = Instant::now();
                control.retire(&mut values, &mut slots);
                totals[1] += started.elapsed().as_nanos();
                control.fill(&mut values, &module, &mut slots);
                let started = Instant::now();
                control.fragment(&mut values, &module, &mut slots);
                totals[2] += started.elapsed().as_nanos();
                control.retire(&mut values, &mut slots);
            }
            if sample >= 3 {
                let [admission, retirement, fragmentation] = totals;
                println!(
                    "WINDOW,{depth},{width},{order},32,{},{admission},{retirement},{fragmentation}",
                    sample - 3
                );
            }
            black_box(&values);
        }
    }
}

#[test]
#[ignore = "manual allocation/transition counts, not throughput"]
#[cfg(feature = "execution-diagnostics")]
fn window_storage_counts() {
    let (_runtime, _program, module) = module();
    for (depth, width, order) in CASES {
        let control = Control::new(depth, width, order);
        let mut slots = Vec::with_capacity(depth);
        let ((phases, counts), total) = allocations::measure(|| {
            diagnostics::measure(|| {
                let mut values = ExecutionValues::default();
                let mut phases = [(
                    allocations::Counts::default(),
                    allocations::Counts::default(),
                ); 3];
                for phase in &mut phases {
                    control.fill(&mut values, &module, &mut slots);
                    phase.0 = allocations::current().unwrap();
                    control.fragment(&mut values, &module, &mut slots);
                    control.retire(&mut values, &mut slots);
                    phase.1 = allocations::current().unwrap();
                }
                phases
            })
        });
        assert_eq!(total.net_bytes, 0);
        assert_eq!(phases[1].1.net_bytes, phases[2].1.net_bytes);
        assert_eq!(phases[1].1.requests, phases[2].1.requests);
        println!(
            "WINDOW_COUNTS,{depth},{width},{order},phases={phases:?},counts={counts:?},total={total:?}"
        );
    }
}
