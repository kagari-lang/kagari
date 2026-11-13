//! Borrowed argument compositions feed one closed frame admission transaction.
use crate::{
    error::RuntimeError,
    execution::roots::Slots,
    execution_metadata::{
        applications::AdmittedApplication, managed::applications::ApplicationObject,
    },
    frame::values::FrameSlots,
    module::execution::layout::Location,
    native::{conversion::arguments::ArgumentValues, future::storage::ColdObject},
    value::{
        carriers::{RootedValue, ScopedValue},
        closures::ClosureObject,
        interfaces::InterfaceObject,
        view::ValueView,
    },
};
use kagari_gc::{error::GcError, object::Gc, root::Root};

/// Public input rejection is distinct from damage to an admitted frame/provider.
#[derive(Debug)]
pub(crate) enum ArgumentError {
    Input(RuntimeError),
    Storage(GcError),
}

#[derive(Clone, Copy)]
enum ExplicitArguments<'args, 'value: 'args> {
    Views(&'args [ValueView<'value>]),
    Roots(&'args [RootedValue]),
    // Protected caller inputs have not yet proved the callee's semantic domains.
    Pack(ArgumentValues<'args>),
    // A verified call site lends its admitted frame banks, not public input.
    Prepared(ArgumentValues<'args>),
}

#[derive(Clone, Copy)]
enum Prefix<'args, 'value: 'args> {
    Method(&'args AdmittedApplication),
    Interface(&'args RootedValue),
    RootedCaptures {
        source: &'args RootedValue,
        count: usize,
    },
    Empty,
    Cold {
        source: &'args Root<ColdObject>,
        count: usize,
    },
    Captures {
        value: ValueView<'value>,
        count: usize,
    },
}

impl Prefix<'_, '_> {
    fn len(self) -> usize {
        match self {
            Self::Empty => 0,
            Self::Interface(_) | Self::Method(_) => 1,
            Self::Captures { count, .. }
            | Self::Cold { count, .. }
            | Self::RootedCaptures { count, .. } => count,
        }
    }
}

/// The argument borrow can be shorter than its protected views' source lifetime.
/// Prefixes name protected receivers/captures. Neither part owns managed values,
/// and no source/control borrow crosses conversion or a callback.
#[derive(Clone, Copy)]
pub(crate) struct FrameArguments<'args, 'value: 'args> {
    prefix: Prefix<'args, 'value>,
    explicit: ExplicitArguments<'args, 'value>,
    count: usize,
}

impl<'args, 'value: 'args> FrameArguments<'args, 'value> {
    /// Read the concrete receiver directly during frame admission. No prefix pack
    /// or escaping lease is prepared for an already retained interface handle.
    pub(crate) fn with_method_receiver(
        mut self,
        source: &'args AdmittedApplication,
    ) -> Result<Self, GcError> {
        if !matches!(self.prefix, Prefix::Empty) {
            return Err(GcError::Storage("frame argument prefix already set"));
        }
        self.count = self
            .count
            .checked_add(1)
            .ok_or(GcError::Limit("frame argument count"))?;
        self.prefix = Prefix::Method(source);
        Ok(self)
    }

    pub(crate) fn with_interface_receiver(
        mut self,
        source: &'args RootedValue,
    ) -> Result<Self, GcError> {
        if !matches!(self.prefix, Prefix::Empty) {
            return Err(GcError::Storage("frame argument prefix already set"));
        }
        self.count = self
            .count
            .checked_add(1)
            .ok_or(GcError::Limit("frame argument count"))?;
        self.prefix = Prefix::Interface(source);
        Ok(self)
    }

    pub(crate) fn with_rooted_captures(
        mut self,
        source: &'args RootedValue,
        count: usize,
    ) -> Result<Self, GcError> {
        if !matches!(self.prefix, Prefix::Empty) {
            return Err(GcError::Storage("frame argument prefix already set"));
        }
        self.count = self
            .count
            .checked_add(count)
            .ok_or(GcError::Limit("frame argument count"))?;
        self.prefix = Prefix::RootedCaptures { source, count };
        Ok(self)
    }

    pub(crate) fn retained(values: &'args [RootedValue]) -> Self {
        Self {
            prefix: Prefix::Empty,
            explicit: ExplicitArguments::Roots(values),
            count: values.len(),
        }
    }

    pub(crate) fn plain(values: &'args [ValueView<'value>]) -> Self {
        Self {
            prefix: Prefix::Empty,
            explicit: ExplicitArguments::Views(values),
            count: values.len(),
        }
    }

    pub(crate) fn pack(values: ArgumentValues<'args>) -> Self {
        Self {
            prefix: Prefix::Empty,
            explicit: ExplicitArguments::Pack(values),
            count: values.len(),
        }
    }

    pub(crate) fn frame(frame: FrameSlots, locations: &'args [Location]) -> Self {
        let values = ArgumentValues::prepared(
            frame.banks.execution,
            frame.banks.managed,
            frame.banks.scalars,
            locations,
        );
        Self {
            prefix: Prefix::Empty,
            explicit: ExplicitArguments::Prepared(values),
            count: values.len(),
        }
    }

    /// Borrow immutable captures from their original provider. No capture pack
    /// or Host roots are materialized; every source is checked before bank growth.
    pub(crate) fn with_captures(
        mut self,
        value: ValueView<'value>,
        count: usize,
    ) -> Result<Self, GcError> {
        if !matches!(self.prefix, Prefix::Empty) {
            return Err(GcError::Storage("frame argument prefix already set"));
        }
        self.count = self
            .count
            .checked_add(count)
            .ok_or(GcError::Limit("frame argument count"))?;
        self.prefix = Prefix::Captures { value, count };
        Ok(self)
    }

    /// Borrow a cold resume closure directly. Its claim remains alive until the
    /// new frame publishes captures/body/environment into its own source suffix.
    pub(crate) fn with_cold_captures(
        mut self,
        source: &'args Root<ColdObject>,
        count: usize,
    ) -> Result<Self, GcError> {
        if !matches!(self.prefix, Prefix::Empty) {
            return Err(GcError::Storage("frame argument prefix already set"));
        }
        self.count = self
            .count
            .checked_add(count)
            .ok_or(GcError::Limit("frame argument count"))?;
        self.prefix = Prefix::Cold { source, count };
        Ok(self)
    }

    pub(crate) fn len(self) -> usize {
        self.count
    }

    pub(crate) fn reject(self, index: usize, error: GcError) -> ArgumentError {
        if index >= self.prefix.len()
            && index < self.count
            && matches!(
                self.explicit,
                ExplicitArguments::Views(_)
                    | ExplicitArguments::Roots(_)
                    | ExplicitArguments::Pack(_)
            )
        {
            match error {
                GcError::ForeignHeap
                | GcError::InvalidRoot
                | GcError::Index
                | GcError::Representation => {
                    ArgumentError::Input(RuntimeError::module_validation(error.to_string()))
                }
                error => ArgumentError::Storage(error),
            }
        } else {
            ArgumentError::Storage(error)
        }
    }

    /// Check prepared-bank bounds before growth. The frame owner then admits each
    /// original value and target domain before allocating its suffix.
    pub(crate) fn check_bounds(self, slots: &mut Slots<'_, '_>) -> Result<(), GcError> {
        if let Prefix::Method(source) = self.prefix {
            method_receiver(slots, source)?;
        }
        if let Prefix::Interface(source) = self.prefix {
            interface_receiver(slots, source)?;
        }
        if let Prefix::RootedCaptures { source, count } = self.prefix {
            let closure = rooted_closure(slots, source)?;
            if slots
                .scope()
                .read(closure, |closure| closure.record().capture_types.len())
                != count
            {
                return Err(GcError::Representation);
            }
        }
        if let Prefix::Captures { value, count } = self.prefix {
            let value = value.get(slots)?;
            let object = slots
                .scope()
                .downcast::<ClosureObject>(value.object().ok_or(GcError::Representation)?)?;
            if slots
                .scope()
                .read(object, |object| object.record().capture_types.len())
                != count
            {
                return Err(GcError::Representation);
            }
        }
        if let Prefix::Cold { source, count } = self.prefix {
            let closure = cold_closure(slots, source)?;
            if slots
                .scope()
                .read(closure, |closure| closure.record().capture_types.len())
                != count
            {
                return Err(GcError::Representation);
            }
        }
        if let ExplicitArguments::Pack(values) | ExplicitArguments::Prepared(values) = self.explicit
        {
            values.check_bounds(slots)?;
        }
        Ok(())
    }

    pub(crate) fn get<'gc>(
        self,
        slots: &mut Slots<'_, 'gc>,
        index: usize,
    ) -> Result<ScopedValue<'gc>, ArgumentError> {
        self.read(slots, index)
            .map_err(|error| self.reject(index, error))
    }

    fn read<'gc>(
        self,
        slots: &mut Slots<'_, 'gc>,
        index: usize,
    ) -> Result<ScopedValue<'gc>, GcError> {
        if index >= self.count {
            return Err(GcError::Index);
        }
        if index < self.prefix.len() {
            return match self.prefix {
                Prefix::Interface(source) => interface_receiver(slots, source),
                Prefix::Method(source) => method_receiver(slots, source),
                Prefix::RootedCaptures { source, .. } => {
                    let closure = rooted_closure(slots, source)?;
                    ClosureObject::capture(slots.scope(), closure, index)
                }
                Prefix::Captures { value, .. } => {
                    let value = value.get(slots)?;
                    let object = slots.scope().downcast::<ClosureObject>(
                        value.object().ok_or(GcError::Representation)?,
                    )?;
                    ClosureObject::capture(slots.scope(), object, index)
                }
                Prefix::Cold { source, .. } => {
                    let object = cold_closure(slots, source)?;
                    ClosureObject::capture(slots.scope(), object, index)
                }
                Prefix::Empty => Err(GcError::Index),
            };
        }
        let index = index - self.prefix.len();
        match self.explicit {
            ExplicitArguments::Roots(values) => {
                values.get(index).ok_or(GcError::Index)?.get(slots.scope())
            }
            ExplicitArguments::Views(values) => values.get(index).ok_or(GcError::Index)?.get(slots),
            ExplicitArguments::Pack(values) | ExplicitArguments::Prepared(values) => {
                values.scoped(slots, index)
            }
        }
    }
}

fn rooted_closure<'gc>(
    slots: &mut Slots<'_, 'gc>,
    root: &RootedValue,
) -> Result<Gc<'gc, ClosureObject>, GcError> {
    let value = root.get(slots.scope())?;
    slots
        .scope()
        .downcast(value.object().ok_or(GcError::Representation)?)
}

fn cold_closure<'gc>(
    slots: &mut Slots<'_, 'gc>,
    source: &Root<ColdObject>,
) -> Result<Gc<'gc, ClosureObject>, GcError> {
    let cold = slots.scope().get(source)?;
    let value = ColdObject::capture(slots.scope(), cold, 0)?;
    slots
        .scope()
        .downcast(value.object().ok_or(GcError::Representation)?)
}

fn interface_receiver<'gc>(
    slots: &mut Slots<'_, 'gc>,
    source: &RootedValue,
) -> Result<ScopedValue<'gc>, GcError> {
    let value = source.get(slots.scope())?;
    let object = slots
        .scope()
        .downcast::<InterfaceObject>(value.object().ok_or(GcError::Representation)?)?;
    InterfaceObject::data(slots.scope(), object)
}

fn method_receiver<'gc>(
    slots: &mut Slots<'_, 'gc>,
    source: &AdmittedApplication,
) -> Result<ScopedValue<'gc>, GcError> {
    let application = source.resolve(slots)?;
    ApplicationObject::receiver(slots.scope(), application)
}
