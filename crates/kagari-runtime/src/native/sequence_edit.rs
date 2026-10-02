//! An isolated sequence edit. Reference slots can only be permuted, so the
//! unchanged, guarded source roots every value throughout synchronous callbacks.
use crate::{
    error::RuntimeError,
    native::{
        binding::NativeResult,
        sequence::{NativeElement, SequenceStorage},
    },
    value::Value,
};

pub struct SequenceEdit<'buffer> {
    pub(crate) values: &'buffer mut SequenceStorage,
}
impl SequenceEdit<'_> {
    pub fn len(&self) -> usize {
        self.values.len()
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    /// The source argument protects this value for the duration of the edit.
    pub fn get(&self, index: usize) -> NativeResult<Value> {
        self.values
            .get(index)
            .ok_or_else(|| RuntimeError::module_validation("sequence edit index"))
    }
    pub fn with_slice_mut<E: NativeElement, R>(
        &mut self,
        access: impl for<'slice> FnOnce(&'slice mut [E]) -> NativeResult<R>,
    ) -> NativeResult<R> {
        let values = E::slice_mut(self.values)
            .ok_or_else(|| RuntimeError::module_validation("sequence edit scalar layout"))?;
        access(values)
    }
    /// `order[destination]` names the original source slot. Validate the entire
    /// permutation before changing the working buffer, then apply disjoint cycles.
    pub fn reorder(&mut self, mut order: Vec<usize>) -> NativeResult<()> {
        let invalid =
            || RuntimeError::module_validation("sequence edit requires a complete permutation");
        if order.len() != self.len() {
            return Err(invalid());
        }
        let mut seen = Vec::new();
        seen.try_reserve_exact(order.len())
            .map_err(|_| RuntimeError::resource_limit("sequence permutation"))?;
        seen.resize(order.len(), false);
        for &source in &order {
            let visited = seen.get_mut(source).ok_or_else(invalid)?;
            if *visited {
                return Err(invalid());
            }
            *visited = true;
        }
        drop(seen);
        for start in 0..order.len() {
            let mut current = start;
            loop {
                let next = order[current];
                order[current] = current;
                if next == start {
                    break;
                }
                self.values.swap(current, next)?;
                current = next;
            }
        }
        Ok(())
    }
}
