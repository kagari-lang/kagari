//! Argument sources feed one checked frame admission and publication protocol.
use crate::{
    Runtime, error::RuntimeError, frame::values::FrameSlots, module::execution::layout::Location,
    value::Value,
};

#[derive(Clone, Copy)]
pub(crate) enum ArgumentSource<'a> {
    Value(&'a Value),
    Window(FrameSlots, Location),
}

#[derive(Clone, Copy)]
enum ExplicitArguments<'a> {
    Values(&'a [Value]),
    Window(FrameSlots, &'a [Location]),
}

#[derive(Clone, Copy)]
pub(crate) struct FrameArguments<'args> {
    captures: &'args [Value],
    explicit: ExplicitArguments<'args>,
    count: usize,
}

impl<'args> FrameArguments<'args> {
    pub(crate) fn plain(explicit: &'args [Value]) -> Self {
        Self {
            captures: &[],
            explicit: ExplicitArguments::Values(explicit),
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
            explicit: ExplicitArguments::Values(explicit),
            count,
        })
    }

    pub(crate) fn frame(slots: FrameSlots, sources: &'args [Location]) -> Self {
        Self {
            captures: &[],
            explicit: ExplicitArguments::Window(slots, sources),
            count: sources.len(),
        }
    }

    pub(crate) fn captured_frame(
        captures: &'args [Value],
        slots: FrameSlots,
        sources: &'args [Location],
    ) -> Result<Self, RuntimeError> {
        let count = captures
            .len()
            .checked_add(sources.len())
            .ok_or_else(|| RuntimeError::module_validation("call argument count"))?;
        Ok(Self {
            captures,
            explicit: ExplicitArguments::Window(slots, sources),
            count,
        })
    }

    pub(crate) fn all(
        self,
        runtime: &Runtime,
        check: impl FnMut(usize, &Value) -> bool,
    ) -> Result<bool, RuntimeError> {
        self.check(runtime, false, check)
    }

    pub(crate) fn all_managed(
        self,
        runtime: &Runtime,
        mut check: impl FnMut(&Value) -> bool,
    ) -> Result<bool, RuntimeError> {
        self.check(runtime, true, |_, value| check(value))
    }

    fn check(
        self,
        runtime: &Runtime,
        managed_only: bool,
        mut check: impl FnMut(usize, &Value) -> bool,
    ) -> Result<bool, RuntimeError> {
        let storage = match self.explicit {
            ExplicitArguments::Values(_) => None,
            ExplicitArguments::Window(..) => {
                Some(runtime.resources().frame_values.try_borrow().map_err(|_| {
                    runtime
                        .resources()
                        .quarantine("argument window borrowed during call entry")
                })?)
            }
        };
        for (index, source) in self.iter().enumerate() {
            let valid = match source {
                ArgumentSource::Value(value) => check(index, value),
                ArgumentSource::Window(slots, location) => {
                    let storage = storage.as_ref().expect("window argument storage");
                    let check = |value: &Value| check(index, value);
                    if managed_only {
                        storage.check_managed_location(slots, location, check)
                    } else {
                        storage.with_location(slots, location, check)
                    }
                    .ok_or_else(|| {
                        runtime
                            .resources()
                            .quarantine("invalid call argument register")
                    })?
                }
            };
            if !valid {
                return Ok(false);
            }
        }
        Ok(true)
    }

    pub(crate) fn len(self) -> usize {
        self.count
    }

    pub(crate) fn iter(self) -> impl Iterator<Item = ArgumentSource<'args>> + Clone {
        (0..self.count).map(move |index| {
            if index < self.captures.len() {
                return ArgumentSource::Value(&self.captures[index]);
            }
            let index = index - self.captures.len();
            match self.explicit {
                ExplicitArguments::Values(values) => ArgumentSource::Value(&values[index]),
                ExplicitArguments::Window(slots, locations) => {
                    ArgumentSource::Window(slots, locations[index])
                }
            }
        })
    }
}
