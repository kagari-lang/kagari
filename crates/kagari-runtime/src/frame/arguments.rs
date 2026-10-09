//! Captures and explicit arguments feed frame roots without a temporary vector.
use crate::{
    Runtime, error::RuntimeError, frame::values::FrameSlots,
    module::execution::calls::ArgumentTransfer, value::Value,
};
use std::slice;

#[derive(Clone, Copy)]
pub(crate) struct FrameArguments<'args> {
    captures: &'args [Value],
    window: Option<(FrameSlots, &'args [ArgumentTransfer])>,
    explicit: &'args [Value],
    count: usize,
}

impl<'args> FrameArguments<'args> {
    pub(crate) fn plain(explicit: &'args [Value]) -> Self {
        Self {
            window: None,
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
            window: None,
            captures,
            explicit,
            count,
        })
    }

    pub(crate) fn frame(slots: FrameSlots, transfers: &'args [ArgumentTransfer]) -> Self {
        Self {
            captures: &[],
            explicit: &[],
            count: transfers.len(),
            window: Some((slots, transfers)),
        }
    }

    pub(crate) fn window(self) -> Option<(FrameSlots, &'args [ArgumentTransfer])> {
        self.window
    }

    pub(crate) fn all_managed(
        self,
        runtime: &Runtime,
        mut check: impl FnMut(&Value) -> bool,
    ) -> Result<bool, RuntimeError> {
        let Some((slots, transfers)) = self.window else {
            return Ok(self.iter().all(check));
        };
        let storage = runtime.resources().frame_values.try_borrow().map_err(|_| {
            runtime
                .resources()
                .quarantine("argument window borrowed during call entry")
        })?;
        for transfer in transfers {
            let valid = storage
                .check_managed_location(slots, transfer.source, &mut check)
                .ok_or_else(|| {
                    runtime
                        .resources()
                        .quarantine("invalid call argument register")
                })?;
            if !valid {
                return Ok(false);
            }
        }
        Ok(true)
    }

    pub(crate) fn len(self) -> usize {
        self.count
    }

    pub(crate) fn iter(self) -> impl Iterator<Item = &'args Value> + Clone {
        assert!(
            self.window.is_none(),
            "frame arguments require their owning arena"
        );
        let captures: slice::Iter<'args, Value> = self.captures.iter();
        captures.chain(self.explicit.iter())
    }
}
