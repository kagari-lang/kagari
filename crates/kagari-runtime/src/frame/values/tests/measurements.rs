//! Actual frame admission controls, distinct from full owned-session workloads.
use super::{allocate, module};
#[cfg(feature = "execution-diagnostics")]
use crate::diagnostics::{self, allocations};
use crate::{
    Runtime,
    execution::roots::{ExecutionRoots, SessionId},
    frame::{arguments::FrameArguments, values::FrameStorage},
    module::{
        ExecutableModule,
        execution::{
            layout::{FrameLayout, Location},
            operands::OperandSlot,
        },
    },
    value::carriers::RootedValue,
};
use kagari_abi::representation::ValueType;
#[cfg(not(feature = "execution-diagnostics"))]
use std::{hint::black_box, time::Instant};

#[derive(Clone, Copy, Debug)]
enum Ownership {
    Nested,
    Independent,
}

const CASES: [(usize, usize, Ownership); 3] = [
    (32, 8, Ownership::Nested),
    (256, 128, Ownership::Nested),
    (256, 128, Ownership::Independent),
];

struct Control {
    depth: usize,
    ownership: Ownership,
    layout: FrameLayout,
    arguments: Vec<RootedValue>,
    parameters: Vec<usize>,
    frames: Vec<Option<(SessionId, FrameStorage)>>,
}

impl Control {
    fn new(depth: usize, width: usize, ownership: Ownership) -> Self {
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
        Self {
            depth,
            ownership,
            layout: FrameLayout {
                locations,
                reused_slots: Box::default(),
                count: width,
                scalar_count: width.div_ceil(2),
                managed_count: width / 2,
            },
            // Inline scalar Host carriers register no roots and are prepared
            // outside timing, just as the old raw scalar input vector was.
            arguments: vec![RootedValue::from(7i32); width],
            parameters: (0..width).collect(),
            frames: Vec::with_capacity(depth),
        }
    }

    fn admit(
        &self,
        runtime: &mut Runtime,
        owner: &ExecutableModule,
        execution: SessionId,
    ) -> FrameStorage {
        allocate(
            runtime,
            owner,
            execution,
            &self.layout,
            &self.parameters,
            FrameArguments::retained(&self.arguments),
        )
        .unwrap()
    }

    fn fill(&mut self, runtime: &mut Runtime, owner: &ExecutableModule) {
        assert!(self.frames.is_empty());
        let shared = matches!(self.ownership, Ownership::Nested)
            .then(|| runtime.memory.enter_execution(&runtime.resources).unwrap());
        for _ in 0..self.depth {
            let execution = shared
                .unwrap_or_else(|| runtime.memory.enter_execution(&runtime.resources).unwrap());
            self.frames
                .push(Some((execution, self.admit(runtime, owner, execution))));
        }
    }

    fn retire_frame(runtime: &mut Runtime, frame: FrameStorage) {
        runtime
            .memory
            .scope(&runtime.resources, |cx, roots| {
                ExecutionRoots::with(roots, cx, |slots| frame.retire(slots))
            })
            .unwrap();
    }

    fn replace(&mut self, runtime: &mut Runtime, owner: &ExecutableModule) {
        match self.ownership {
            Ownership::Nested => {
                // A linear stack can replace only its top. Independent owners
                // below exercise arbitrary retirement without illegal holes.
                let index = self.frames.len() - 1;
                let (execution, frame) = self.frames[index].take().unwrap();
                Self::retire_frame(runtime, frame);
                self.frames[index] = Some((execution, self.admit(runtime, owner, execution)));
            }
            Ownership::Independent => {
                for index in (0..self.depth).step_by(2) {
                    let (execution, frame) = self.frames[index].take().unwrap();
                    Self::retire_frame(runtime, frame);
                    runtime
                        .memory
                        .retire_execution(&runtime.resources, execution)
                        .unwrap();
                    let execution = runtime.memory.enter_execution(&runtime.resources).unwrap();
                    self.frames[index] = Some((execution, self.admit(runtime, owner, execution)));
                }
            }
        }
    }

    fn retire(&mut self, runtime: &mut Runtime) {
        match self.ownership {
            Ownership::Nested => {
                let mut execution = None;
                while let Some(frame) = self.frames.pop() {
                    let (id, frame) = frame.unwrap();
                    execution = Some(id);
                    Self::retire_frame(runtime, frame);
                }
                runtime
                    .memory
                    .retire_execution(&runtime.resources, execution.unwrap())
                    .unwrap();
            }
            Ownership::Independent => {
                for frame in self.frames.drain(..) {
                    let (execution, frame) = frame.unwrap();
                    Self::retire_frame(runtime, frame);
                    runtime
                        .memory
                        .retire_execution(&runtime.resources, execution)
                        .unwrap();
                }
            }
        }
    }
}

#[test]
#[ignore = "manual release-profile frame admission timing; run without competing builds"]
#[cfg(not(feature = "execution-diagnostics"))]
fn frame_storage_timing() {
    let (mut runtime, _program, owner) = module();
    for (depth, width, ownership) in CASES {
        let mut control = Control::new(depth, width, ownership);
        for sample in 0..14 {
            let mut elapsed = [0u128; 3];
            for _ in 0..32 {
                let started = Instant::now();
                control.fill(&mut runtime, &owner);
                elapsed[0] += started.elapsed().as_nanos();
                let started = Instant::now();
                control.replace(&mut runtime, &owner);
                elapsed[1] += started.elapsed().as_nanos();
                let started = Instant::now();
                control.retire(&mut runtime);
                elapsed[2] += started.elapsed().as_nanos();
            }
            if sample >= 3 {
                println!(
                    "FRAME_STORAGE,{ownership:?},{depth},{width},sample={},admit_ns={},replace_ns={},retire_ns={}",
                    sample - 3,
                    elapsed[0],
                    elapsed[1],
                    elapsed[2]
                );
            }
            black_box(&runtime);
        }
    }
}

#[test]
#[ignore = "manual allocation/transition counts, not throughput"]
#[cfg(feature = "execution-diagnostics")]
fn frame_storage_counts() {
    for (depth, width, ownership) in CASES {
        let mut control = Control::new(depth, width, ownership);
        let ((phases, counts), total) = allocations::measure(|| {
            diagnostics::measure(|| {
                let (mut runtime, _program, owner) = module();
                let mut phases = [(
                    allocations::Counts::default(),
                    allocations::Counts::default(),
                ); 3];
                for phase in &mut phases {
                    phase.0 = allocations::current().unwrap();
                    control.fill(&mut runtime, &owner);
                    control.replace(&mut runtime, &owner);
                    control.retire(&mut runtime);
                    phase.1 = allocations::current().unwrap();
                }
                phases
            })
        });
        assert_eq!(
            phases[1].1.net_bytes - phases[1].0.net_bytes,
            phases[2].1.net_bytes - phases[2].0.net_bytes
        );
        assert_eq!(
            phases[1].1.requests - phases[1].0.requests,
            phases[2].1.requests - phases[2].0.requests
        );
        assert!(counts.bank_reservations > 0);
        assert!(counts.bank_bytes_initialized > 0);
        assert!(counts.argument_transfers > 0);
        assert!(counts.root_registrations > 0);
        if matches!(ownership, Ownership::Nested) {
            assert!(counts.bank_bytes_reallocated > 0);
        }
        println!(
            "FRAME_STORAGE_COUNTS,{ownership:?},{depth},{width},phases={phases:?},counts={counts:?},total={total:?}"
        );
    }
}
