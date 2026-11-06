//! An ordinary ID graph; never owns a GC handle or asks the collector for reachability.

use std::collections::VecDeque;

pub(super) const ROOTS: usize = 16;
pub(super) type Id = usize;

#[derive(Debug, Clone, Copy)]
pub(super) enum Operation {
    Alloc(usize, Option<Id>),
    Edge(Id, Option<Id>),
    Root(usize, Option<Id>),
    Dynamic(usize, Option<Id>),
    Write(Id, u32),
    Label(u64),
    Push(Id),
    Remove(usize),
    Reset,
    Step(usize),
    Collect,
    Safepoint,
}

pub(super) struct Node {
    pub next: Option<Id>,
    pub tag: u32,
}

pub(super) struct Model {
    pub nodes: Vec<Node>,
    pub roots: [Option<Id>; ROOTS],
    pub dynamic: [Option<Id>; 2],
    pub label: u64,
    pub vector: Vec<Id>,
}

impl Default for Model {
    fn default() -> Self {
        Self {
            nodes: Vec::new(),
            roots: [None; ROOTS],
            dynamic: [None; 2],
            label: 0,
            vector: Vec::new(),
        }
    }
}

impl Model {
    pub fn reachable(&self) -> Vec<bool> {
        let mut found = vec![false; self.nodes.len()];
        let mut queue: VecDeque<_> = self
            .roots
            .iter()
            .chain(&self.dynamic)
            .flatten()
            .copied()
            .chain(self.vector.iter().copied())
            .collect();
        while let Some(id) = queue.pop_front() {
            if !found[id] {
                found[id] = true;
                queue.extend(self.nodes[id].next);
            }
        }
        found
    }

    pub fn validate(&self, op: Operation) -> bool {
        let live = self.reachable();
        let target = |id: Option<Id>| id.is_none_or(|id| live.get(id) == Some(&true));
        match op {
            Operation::Alloc(slot, next) | Operation::Root(slot, next) => {
                slot < ROOTS && target(next)
            }
            Operation::Edge(id, next) => target(Some(id)) && target(next),
            Operation::Dynamic(slot, next) => slot < 2 && target(next),
            Operation::Write(id, _) | Operation::Push(id) => target(Some(id)),
            Operation::Remove(index) => index < self.vector.len(),
            Operation::Step(budget) => (8..=1024).contains(&budget),
            Operation::Label(_) | Operation::Reset | Operation::Collect | Operation::Safepoint => {
                true
            }
        }
    }

    pub fn apply(&mut self, op: Operation) {
        match op {
            Operation::Alloc(slot, next) => {
                self.roots[slot] = Some(self.nodes.len());
                self.nodes.push(Node { next, tag: 0 });
            }
            Operation::Edge(id, next) => self.nodes[id].next = next,
            Operation::Root(slot, next) => self.roots[slot] = next,
            Operation::Dynamic(slot, next) => self.dynamic[slot] = next,
            Operation::Write(id, tag) => self.nodes[id].tag = tag,
            Operation::Label(value) => self.label = value,
            Operation::Push(id) => self.vector.push(id),
            Operation::Remove(index) => {
                self.vector.remove(index);
            }
            Operation::Reset => {
                self.roots.fill(None);
                self.dynamic.fill(None);
                self.vector.clear();
            }
            Operation::Step(_) | Operation::Collect | Operation::Safepoint => {}
        }
    }
}

pub(super) fn value(id: Id, tag: u32) -> i64 {
    // Encode identity and mutable content separately, so mistaken aliases cannot
    // pass just because two application values happen to compare equal.
    ((u64::from(u32::try_from(id).expect("finite model IDs")) << 32) | u64::from(tag)) as i64
}

/// A specified integer generator: reproducible across hosts/toolchains.
pub(super) struct Generator(u64);

impl Generator {
    pub fn new(seed: u64) -> Self {
        Self(seed)
    }

    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e3779b97f4a7c15);
        let mut x = self.0;
        x = (x ^ (x >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        x = (x ^ (x >> 27)).wrapping_mul(0x94d049bb133111eb);
        x ^ (x >> 31)
    }

    fn index(&mut self, len: usize) -> usize {
        (self.next() % len as u64) as usize
    }

    pub fn operation(&mut self, model: &Model) -> Operation {
        let live: Vec<_> = model
            .reachable()
            .into_iter()
            .enumerate()
            .filter_map(|(i, live)| live.then_some(i))
            .collect();
        let slot = self.index(ROOTS);
        if live.is_empty() {
            return Operation::Alloc(slot, None);
        }
        let id = live[self.index(live.len())];
        let next = (self.next() & 3 != 0).then(|| live[self.index(live.len())]);
        match self.index(24) {
            0..=5 => Operation::Alloc(slot, next),
            6..=8 => Operation::Edge(id, next),
            9..=10 => Operation::Root(slot, next),
            11..=12 => Operation::Dynamic(self.index(2), next),
            13 => Operation::Write(id, self.next() as u32),
            14 => Operation::Label(self.next()),
            15..=17 if model.vector.len() < 128 => Operation::Push(id),
            18..=19 if !model.vector.is_empty() => {
                Operation::Remove(self.index(model.vector.len()))
            }
            20 => Operation::Collect,
            21 => Operation::Safepoint,
            22 => Operation::Reset,
            _ => Operation::Step([8, 16, 31, 64, 1024][self.index(5)]),
        }
    }
}
