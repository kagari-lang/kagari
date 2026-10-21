//! Directional fixed points over the verifier's exact reachable CFG.
use super::{Error, Flow};
use std::collections::VecDeque;

impl Flow<'_> {
    pub(super) fn fixed_points(&mut self) -> Result<(), Error> {
        let order = self.postorder()?;
        self.propagate(&order, Direction::Initialization)?;
        self.propagate(&order, Direction::Liveness)
    }

    fn postorder(&mut self) -> Result<Vec<usize>, Error> {
        self.charge(self.blocks.len().saturating_mul(3))?;
        let mut visited = vec![false; self.blocks.len()];
        let mut stack = Vec::with_capacity(self.blocks.len());
        stack.push((0, 0));
        let mut order = Vec::with_capacity(self.blocks.len());
        visited[0] = true;
        while let Some(&(index, edge)) = stack.last() {
            self.charge(1)?;
            if let Some(&next) = self.blocks[index].successors.get(edge) {
                stack.last_mut().expect("active DFS frame").1 += 1;
                if !visited[next] {
                    visited[next] = true;
                    stack.push((next, 0));
                }
            } else {
                stack.pop();
                order.push(index);
            }
        }
        Ok(order)
    }

    fn propagate(&mut self, order: &[usize], direction: Direction) -> Result<(), Error> {
        self.charge(self.blocks.len().saturating_add(order.len()))?;
        // Seed every reachable block, not just exits. No-exit SCCs can contain
        // reads and must participate in the backward fixed point as well.
        let mut queued = self
            .blocks
            .iter()
            .map(|block| block.reachable)
            .collect::<Vec<_>>();
        let mut pending = VecDeque::with_capacity(order.len());
        match direction {
            Direction::Initialization => pending.extend(order.iter().rev().copied()),
            Direction::Liveness => pending.extend(order.iter().copied()),
        }
        while let Some(index) = pending.pop_front() {
            queued[index] = false;
            #[cfg(test)]
            {
                self.visits[match direction {
                    Direction::Initialization => 0,
                    Direction::Liveness => 1,
                }] += 1;
            }
            let block = &self.blocks[index];
            let edges = match direction {
                Direction::Initialization => block.predecessors.len(),
                Direction::Liveness => block.successors.len(),
            };
            self.charge(
                self.words
                    .saturating_mul(edges.saturating_add(3))
                    .saturating_add(1),
            )?;
            let mut state = match direction {
                Direction::Initialization => self.incoming(index),
                Direction::Liveness => self.live_out(index),
            };
            let block = &mut self.blocks[index];
            for (word, value) in state.iter_mut().enumerate() {
                *value = match direction {
                    Direction::Initialization => *value | block.defines[word],
                    Direction::Liveness => (*value & !block.defines[word]) | block.uses[word],
                };
            }
            let current = match direction {
                Direction::Initialization => &mut block.initialized,
                Direction::Liveness => &mut block.live,
            };
            if *current == state {
                continue;
            }
            *current = state;
            let dependents = match direction {
                Direction::Initialization => self.blocks[index].successors.len(),
                Direction::Liveness => self.blocks[index].predecessors.len(),
            };
            for edge in 0..dependents {
                self.charge(1)?;
                let next = match direction {
                    Direction::Initialization => self.blocks[index].successors[edge],
                    Direction::Liveness => self.blocks[index].predecessors[edge],
                };
                if self.blocks[next].reachable && !queued[next] {
                    queued[next] = true;
                    pending.push_back(next);
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy)]
enum Direction {
    Initialization,
    Liveness,
}

#[cfg(test)]
mod tests;
