//! Captures and explicit arguments feed frame roots without a temporary vector.
use crate::{error::RuntimeError, value::Value};
use std::slice;

#[derive(Clone, Copy)]
pub(crate) struct FrameArguments<'args> {
    captures: &'args [Value],
    explicit: &'args [Value],
    count: usize,
}
impl<'args> FrameArguments<'args> {
    pub(crate) fn plain(explicit: &'args [Value]) -> Self {
        Self {
            captures: &[],
            explicit,
            count: explicit.len(),
        }
    }
    pub(crate) fn captured(
        captures: &'args [Value],
        explicit: &'args [Value],
    ) -> Result<Self, RuntimeError> {
        let count = captures
            .len()
            .checked_add(explicit.len())
            .ok_or_else(|| RuntimeError::module_validation("closure argument count"))?;
        Ok(Self {
            captures,
            explicit,
            count,
        })
    }
    pub(crate) fn len(self) -> usize {
        self.count
    }
    pub(crate) fn iter(self) -> impl Iterator<Item = &'args Value> + Clone {
        let captures: slice::Iter<'args, Value> = self.captures.iter();
        captures.chain(self.explicit.iter())
    }
}
