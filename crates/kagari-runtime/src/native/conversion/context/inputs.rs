//! Initialization consumes actual Host ownership or already protected source views.
use crate::{
    execution::roots::Slots,
    native::conversion::arguments::ArgumentValues,
    value::{
        carriers::{RootedValue, ScopedValue},
        view::ValueView,
    },
};
use kagari_gc::error::GcError;

pub(crate) enum ValueInput<'root, 'call> {
    Root(&'root RootedValue),
    View(ValueView<'call>),
    Pack(ArgumentValues<'call>, usize),
}

impl ValueInput<'_, '_> {
    pub(crate) fn get<'gc>(self, slots: &mut Slots<'_, 'gc>) -> Result<ScopedValue<'gc>, GcError> {
        match self {
            Self::Root(root) => root.get(slots.scope()),
            Self::View(view) => view.get(slots),
            Self::Pack(pack, index) => pack.scoped(slots, index),
        }
    }
}
