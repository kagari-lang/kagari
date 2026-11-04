//! Argument sources feed one checked frame admission and publication protocol.
use crate::{
    Runtime,
    error::RuntimeError,
    frame::values::FrameSlots,
    module::execution::{OperandSlot, layout::Location},
    value::Value,
};
use kagari_abi::representation::ValueType;

/// Read-only argument sources for synchronous backend entry.
///
/// Slice sources must stay protected by their supplying caller until publication.
/// Window sources borrow their scoped owner and retain coordinates, never a bank
/// borrow. Backend admission validates source identities and actual values.
#[derive(Clone, Copy)]
pub struct InvocationArguments<'args> {
    pub(crate) source: FrameArguments<'args>,
}

impl<'args> InvocationArguments<'args> {
    /// Borrow values whose supplying caller owns protection until frame entry.
    pub fn from_values(values: &'args [Value]) -> Self {
        Self {
            source: FrameArguments::plain(values),
        }
    }

    /// Number of arguments, including an attached receiver/capture prefix.
    pub fn len(self) -> usize {
        self.source.len()
    }

    /// Whether this source supplies no arguments.
    pub fn is_empty(self) -> bool {
        self.len() == 0
    }

    /// Copy one argument without retaining a bank borrow or adding a root lease.
    /// Rejects an invalid index, foreign window or expired window generation.
    pub fn get(self, runtime: &Runtime, index: usize) -> Result<Value, RuntimeError> {
        match self.source.iter().nth(index) {
            Some(ArgumentSource::Value(value)) => Ok(*value),
            Some(ArgumentSource::Window(location)) => self
                .source
                .window()
                .and_then(|slots| slots.with_location(runtime.gc(), location, Value::clone))
                .ok_or_else(|| RuntimeError::module_validation("invalid argument window")),
            None => Err(RuntimeError::module_validation("argument index")),
        }
    }

    pub(crate) fn with_prefix(mut self, prefix: &'args [Value]) -> Result<Self, RuntimeError> {
        self.source = self.source.with_prefix(prefix)?;
        Ok(self)
    }
}

#[derive(Clone, Copy)]
pub(crate) enum ArgumentSource<'a> {
    Value(&'a Value),
    Window(Location),
}

#[derive(Clone, Copy)]
enum ExplicitArguments<'a> {
    Values(&'a [Value]),
    Window(&'a WindowArguments<'a>),
}

/// Caller-owned coordinates for one window argument source.
///
/// FrameArguments only borrows this description. Locations are prepared code
/// metadata or an implicit dense prefix, never a reference into the value banks.
pub(crate) struct WindowArguments<'locations> {
    slots: FrameSlots,
    projection: WindowProjection<'locations>,
}

enum WindowProjection<'locations> {
    Mapped(&'locations [Location]),
    Contiguous(usize),
}

impl<'locations> WindowArguments<'locations> {
    pub(crate) fn mapped(slots: FrameSlots, locations: &'locations [Location]) -> Self {
        Self {
            slots,
            projection: WindowProjection::Mapped(locations),
        }
    }

    pub(crate) fn contiguous(slots: FrameSlots, count: usize) -> Self {
        Self {
            slots,
            projection: WindowProjection::Contiguous(count),
        }
    }

    pub(crate) fn slots(&self) -> FrameSlots {
        self.slots
    }

    pub(crate) fn len(&self) -> usize {
        match &self.projection {
            WindowProjection::Mapped(locations) => locations.len(),
            WindowProjection::Contiguous(count) => *count,
        }
    }

    fn location(&self, index: usize) -> Location {
        match &self.projection {
            WindowProjection::Mapped(locations) => locations[index],
            WindowProjection::Contiguous(_) => Location {
                operand: OperandSlot::new(index, true),
                representation: ValueType::Generic,
                semantic: None,
                borrowed_receiver: false,
            },
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct FrameArguments<'args> {
    prefix: &'args [Value],
    explicit: ExplicitArguments<'args>,
    count: usize,
}

impl<'args> FrameArguments<'args> {
    pub(crate) fn plain(explicit: &'args [Value]) -> Self {
        Self {
            prefix: &[],
            explicit: ExplicitArguments::Values(explicit),
            count: explicit.len(),
        }
    }

    pub(crate) fn frame(source: &'args WindowArguments<'args>) -> Self {
        Self {
            prefix: &[],
            explicit: ExplicitArguments::Window(source),
            count: source.len(),
        }
    }

    /// Attach the selected receiver or closure captures to explicit arguments.
    pub(crate) fn with_prefix(mut self, prefix: &'args [Value]) -> Result<Self, RuntimeError> {
        if !self.prefix.is_empty() {
            return Err(RuntimeError::module_validation(
                "call argument prefix already set",
            ));
        }
        self.count = prefix
            .len()
            .checked_add(self.count)
            .ok_or_else(|| RuntimeError::module_validation("call argument count"))?;
        self.prefix = prefix;
        Ok(self)
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
            ExplicitArguments::Window(_) => {
                Some(runtime.resources().frame_values.try_borrow().map_err(|_| {
                    runtime
                        .resources()
                        .quarantine("argument window borrowed during call entry")
                })?)
            }
        };
        let operands = self
            .window()
            .map(|slots| {
                storage
                    .as_ref()
                    .expect("window argument storage")
                    .read_operands(slots)
                    .ok_or_else(|| {
                        runtime
                            .resources()
                            .quarantine("invalid call argument window")
                    })
            })
            .transpose()?;
        for (index, source) in self.iter().enumerate() {
            let valid = match source {
                ArgumentSource::Value(value) => check(index, value),
                ArgumentSource::Window(location) => {
                    let operands = operands.as_ref().expect("admitted argument window");
                    let check = |value: &Value| check(index, value);
                    if managed_only {
                        operands.check_managed(location, check)
                    } else {
                        operands.with_value(location, check)
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

    pub(crate) fn window(self) -> Option<FrameSlots> {
        match self.explicit {
            ExplicitArguments::Values(_) => None,
            ExplicitArguments::Window(source) => Some(source.slots()),
        }
    }

    pub(crate) fn iter(self) -> impl Iterator<Item = ArgumentSource<'args>> + Clone {
        // Private constructors establish the complete range once. Single public
        // reads use this same iterator's checked nth; bulk admission retains the
        // original bounded iteration without per-element Option discovery.
        (0..self.count).map(move |index| {
            if index < self.prefix.len() {
                return ArgumentSource::Value(&self.prefix[index]);
            }
            let index = index - self.prefix.len();
            match self.explicit {
                ExplicitArguments::Values(values) => ArgumentSource::Value(&values[index]),
                ExplicitArguments::Window(source) => ArgumentSource::Window(source.location(index)),
            }
        })
    }
}
