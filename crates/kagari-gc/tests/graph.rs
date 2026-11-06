mod model;
mod storage;
mod trace;

use self::{
    model::{Generator, Model, Operation, value},
    storage::Storage,
};
use crate::support::{Node, ObjectVec};
use kagari_gc::{
    error::GcError,
    heap::{Heap, HeapConfig, Phase, pacing::PacingConfig},
    scan::WorkBudget,
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

struct Runner {
    heap: Heap,
    storage: Storage,
    model: Model,
    drops: Vec<Arc<AtomicUsize>>,
    phases: [usize; 4],
}

impl Runner {
    fn new() -> Result<Self, GcError> {
        let mut heap = Heap::new(HeapConfig {
            pacing: PacingConfig {
                min_growth_bytes: 256,
                ..PacingConfig::default()
            },
            ..HeapConfig::default()
        })?;
        let storage = Storage::new(&mut heap)?;
        Ok(Self {
            heap,
            storage,
            model: Model::default(),
            drops: Vec::new(),
            phases: [0; 4],
        })
    }

    fn check(&mut self, exact: bool) -> Result<(), GcError> {
        let live = self.model.reachable();
        for (id, count) in self.drops.iter().enumerate() {
            let count = count.load(Ordering::Relaxed);
            assert!(count <= 1, "object {id} was disposed twice");
            if live[id] {
                assert_eq!(count, 0, "reachable object {id} was disposed");
            } else if exact {
                assert_eq!(count, 1, "unreachable object {id} survived a fresh cycle");
            }
        }
        // Check destructor evidence before following any model-expected live pointer.
        self.heap
            .scope(|cx| self.storage.handles(cx, &self.model).map(|_| ()))?;
        if exact {
            // The vector and dynamic anchor are the only additional heap objects.
            assert_eq!(
                self.heap.stats().objects,
                live.iter().filter(|v| **v).count() + 2
            );
            assert_eq!(self.heap.stats().root_registrations, 3);
        }
        Ok(())
    }

    fn execute(&mut self, op: Operation) -> Result<(), GcError> {
        assert!(self.model.validate(op), "invalid replay operation: {op:?}");
        self.check(false)?;
        let storage = &self.storage;
        let model = &self.model;
        match op {
            Operation::Alloc(slot, next) => {
                let id = model.nodes.len();
                let drops = Arc::new(AtomicUsize::new(0));
                self.heap.scope(|cx| {
                    let handles = storage.handles(cx, model)?;
                    let object = Node::alloc_scoped(cx, value(id, 0), &drops)?;
                    Node::NEXT.set(cx, object, next.map(|id| handles[&id]));
                    cx.set_root(&storage.roots, slot, Some(object))
                })?;
                self.drops.push(drops);
            }
            Operation::Edge(id, next) => self.heap.scope(|cx| {
                let handles = storage.handles(cx, model)?;
                Node::NEXT.set(cx, handles[&id], next.map(|id| handles[&id]));
                Ok(())
            })?,
            Operation::Root(slot, next) => self.heap.scope(|cx| {
                let handles = storage.handles(cx, model)?;
                cx.set_root(&storage.roots, slot, next.map(|id| handles[&id]))
            })?,
            Operation::Dynamic(slot, next) => self.heap.scope(|cx| {
                let handles = storage.handles(cx, model)?;
                let dynamic = cx.get(&storage.dynamic)?;
                storage.links[slot].set(cx, dynamic, next.map(|id| handles[&id].erase()))
            })?,
            Operation::Write(id, tag) => self.heap.scope(|cx| {
                let handles = storage.handles(cx, model)?;
                Node::VALUE.set(cx, handles[&id], value(id, tag));
                Ok(())
            })?,
            Operation::Label(label) => self.heap.scope(|cx| {
                let dynamic = cx.get(&storage.dynamic)?;
                storage.label.set(cx, dynamic, label)
            })?,
            Operation::Push(id) => {
                let root = self.heap.scope(|cx| {
                    let handles = storage.handles(cx, model)?;
                    cx.retain(handles[&id])
                })?;
                ObjectVec::push(&mut self.heap.access()?, &storage.vector, &root)?;
                drop(root);
            }
            Operation::Remove(index) => {
                let root = ObjectVec::remove(&mut self.heap.access()?, &storage.vector, index)?;
                let id = model.vector[index];
                assert_eq!(
                    Node::value(&mut self.heap.access()?, &root)?,
                    value(id, model.nodes[id].tag)
                );
                drop(root);
            }
            Operation::Reset => {
                self.heap.scope(|cx| {
                    for slot in 0..model.roots.len() {
                        cx.set_root::<Node>(&storage.roots, slot, None)?;
                    }
                    let dynamic = cx.get(&storage.dynamic)?;
                    for link in &storage.links {
                        link.set(cx, dynamic, None)?;
                    }
                    Ok(())
                })?;
                for index in (0..model.vector.len()).rev() {
                    drop(ObjectVec::remove(
                        &mut self.heap.access()?,
                        &storage.vector,
                        index,
                    )?);
                }
            }
            Operation::Step(budget) => {
                let report = self.heap.step(WorkBudget::new(budget))?;
                assert!(report.work <= budget && report.retired_owners <= 32);
            }
            Operation::Collect => {
                self.heap.collect()?;
            }
            Operation::Safepoint => {
                self.heap.safepoint()?;
            }
        }
        self.model.apply(op);
        let index = match self.heap.stats().phase {
            Phase::Idle => 0,
            Phase::Roots => 1,
            Phase::Mark => 2,
            Phase::Sweep => 3,
            phase => panic!("valid operation entered {phase:?}"),
        };
        self.phases[index] += 1;
        self.check(matches!(op, Operation::Collect))
    }

    fn finish(mut self) -> Result<[usize; 4], GcError> {
        self.execute(Operation::Collect)?;
        drop(self.storage);
        self.heap.collect()?;
        for count in &self.drops {
            assert_eq!(count.load(Ordering::Relaxed), 1);
        }
        let stats = self.heap.stats();
        assert_eq!(
            (
                stats.objects,
                stats.root_registrations,
                stats.backing_bytes,
                stats.indirect_bytes
            ),
            (0, 0, 0, 0)
        );
        Ok(self.phases)
    }
}

pub(crate) fn run(seed: u64, operations: usize) {
    trace::run(seed, |trace| {
        let mut runner = Runner::new()?;
        let mut generator = Generator::new(seed);
        // Establish a resumable vector, cycles and sharing before random edits.
        for i in 0..24 {
            trace.execute(&mut runner, Operation::Alloc(i % model::ROOTS, None))?;
            trace.execute(&mut runner, Operation::Push(i))?;
        }
        for i in 0..24 {
            trace.execute(&mut runner, Operation::Edge(i, Some((i + 1) % 24)))?;
        }
        for slice in 0..4096 {
            trace.execute(&mut runner, Operation::Step(8))?;
            if runner.heap.stats().phase == Phase::Idle {
                break;
            }
            assert!(slice != 4095, "finite warmup cycle did not complete");
        }
        for index in 0..operations {
            let op = generator.operation(&runner.model);
            trace.execute(&mut runner, op)?;
            // Every mutation is followed by a small service slice; large full
            // collections alone cannot cover between-slice publication/repair.
            trace.execute(&mut runner, Operation::Step([8, 16, 31][index % 3]))?;
        }
        trace.execute(&mut runner, Operation::Collect)?;
        let phases = runner.finish()?;
        assert!(
            phases.iter().all(|count| *count != 0),
            "uncovered phase: {phases:?}"
        );
        Ok(())
    });
}

pub(crate) fn replay(path: &str) {
    trace::replay(path);
}
