//! Function-local provenance of explicit views in immutable tuples. A shape is
//! a set of index paths to views; the empty path denotes the value itself.
use std::{collections::HashMap, sync::Arc};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Shape(usize);

impl Shape {
    pub(super) const VIEW: Self = Self(1);

    fn paths(self, sets: &[Arc<[usize]>]) -> &[usize] {
        match self.0 {
            0 => &[],
            1 => &[0],
            n => &sets[n - 2],
        }
    }

    pub(super) fn contains_view(self) -> bool {
        self.0 != 0
    }
}

// Intern tails and sets for this verifier invocation only. Slot copies and CFG
// comparisons then cost one ID even when the same nested tuple is used twice.
// Flat tables avoid recursive comparison/destruction on untrusted input.
#[derive(Default)]
pub(super) struct Projections {
    paths: Vec<(usize, usize)>,
    prefixes: HashMap<(usize, usize), usize>,
    sets: Vec<Arc<[usize]>>,
    shapes: HashMap<Arc<[usize]>, Shape>,
}

impl Projections {
    fn shape<E>(
        &mut self,
        mut paths: Vec<usize>,
        charge: &mut impl FnMut(usize, usize) -> Result<(), E>,
    ) -> Result<Shape, E> {
        if paths.is_empty() {
            return Ok(Shape::default());
        }
        charge(
            paths.len().saturating_mul(paths.len().ilog2() as usize + 2),
            0,
        )?;
        paths.sort_unstable();
        paths.dedup();
        if paths == [0] {
            return Ok(Shape::VIEW);
        }
        if let Some(&shape) = self.shapes.get(paths.as_slice()) {
            return Ok(shape);
        }
        // Include the Arc, table capacity and hash entries, not just the payload.
        charge(1, 128 + paths.len() * size_of::<usize>())?;
        let shape = Shape(self.sets.len() + 2);
        let paths: Arc<[usize]> = paths.into();
        self.shapes.insert(paths.clone(), shape);
        self.sets.push(paths);
        Ok(shape)
    }

    pub(super) fn tuple<E>(
        &mut self,
        members: impl Iterator<Item = Shape>,
        charge: &mut impl FnMut(usize, usize) -> Result<(), E>,
    ) -> Result<Shape, E> {
        let mut paths = Vec::new();
        for (index, member) in members.enumerate() {
            for &tail in member.paths(&self.sets) {
                charge(1, 2 * size_of::<usize>())?;
                let key = (index, tail);
                let path = if let Some(&id) = self.prefixes.get(&key) {
                    id
                } else {
                    charge(1, 96)?;
                    self.paths.push(key);
                    let id = self.paths.len();
                    self.prefixes.insert(key, id);
                    id
                };
                paths.push(path);
            }
        }
        self.shape(paths, charge)
    }

    pub(super) fn project<E>(
        &mut self,
        base: Shape,
        index: Option<usize>,
        charge: &mut impl FnMut(usize, usize) -> Result<(), E>,
    ) -> Result<Shape, E> {
        let mut result = Vec::new();
        for &path in base.paths(&self.sets) {
            charge(1, 0)?;
            // A direct view is not itself an indexable tuple.
            if path != 0 {
                let (member, tail) = self.paths[path - 1];
                if index.is_none_or(|index| index == member) {
                    charge(1, 2 * size_of::<usize>())?;
                    result.push(tail);
                }
            }
        }
        self.shape(result, charge)
    }

    pub(super) fn join<E>(
        &mut self,
        left: Shape,
        right: Shape,
        charge: &mut impl FnMut(usize, usize) -> Result<(), E>,
    ) -> Result<Shape, E> {
        if left == right || !right.contains_view() {
            return Ok(left);
        }
        if !left.contains_view() {
            return Ok(right);
        }
        let (left, right) = (left.paths(&self.sets), right.paths(&self.sets));
        let len = left.len() + right.len();
        charge(len, len * size_of::<usize>())?;
        self.shape(left.iter().chain(right.iter()).copied().collect(), charge)
    }
}
