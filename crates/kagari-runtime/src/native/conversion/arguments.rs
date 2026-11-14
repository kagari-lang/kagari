//! The outer Rust tuple is an argument list; its elements each convert one value.
use crate::{
    error::RuntimeError,
    execution::roots::{SessionId, Slots, ValueBank, ValueLocation, queued::QueuedArguments},
    frame::types::arguments::TypeArgument,
    host::HostRegistryId,
    module::{ModuleKey, execution::layout::Location},
    native::{
        binding::NativeResult,
        catalog::DeclarationCatalog,
        conversion::{FromKagari, IntoKagari, KagariType, context::ConversionContext},
        types::Type,
    },
    value::{
        carriers::ScopedValue,
        kind::ObjectKind,
        scalar::{self, domain},
        view::ValueView,
    },
};
use kagari_gc::error::GcError;
use std::marker::PhantomData;

/// A non-owning parameter view. Prepared Native inputs read the caller's banks;
/// converted packs use one contiguous local prefix. Slices share that storage.
#[derive(Debug, Clone, Copy)]
pub struct ArgumentValues<'call> {
    storage: ArgumentStorage<'call>,
    offset: usize,
    count: usize,
    lifetime: PhantomData<fn(&'call ()) -> &'call ()>,
}

#[derive(Debug, Clone, Copy)]
enum ArgumentStorage<'call> {
    Queued(&'call QueuedArguments),
    Locals {
        execution: SessionId,
        start: usize,
    },
    Prepared {
        execution: SessionId,
        managed: usize,
        scalars: usize,
        locations: &'call [Location],
    },
    Scalars(&'call [ValueView<'static>]),
}

impl<'call> ArgumentValues<'call> {
    pub(crate) fn queued(values: &'call QueuedArguments) -> Self {
        Self {
            storage: ArgumentStorage::Queued(values),
            offset: 0,
            count: values.len(),
            lifetime: PhantomData,
        }
    }

    pub(crate) fn new(execution: SessionId, start: usize, count: usize) -> Self {
        Self {
            storage: ArgumentStorage::Locals { execution, start },
            offset: 0,
            count,
            lifetime: PhantomData,
        }
    }

    /// Only admitted frame/control owners provide bank bases and checked facts.
    /// The caller keeps that frame alive throughout this nonescaping call view.
    pub(crate) fn prepared(
        execution: SessionId,
        managed: usize,
        scalars: usize,
        locations: &'call [Location],
    ) -> Self {
        Self {
            storage: ArgumentStorage::Prepared {
                execution,
                managed,
                scalars,
                locations,
            },
            offset: 0,
            count: locations.len(),
            lifetime: PhantomData,
        }
    }

    /// The sealed scalar pack owner lends its Rust stack buffer synchronously.
    /// Admission below independently rejects managed/ephemeral carriers.
    pub(crate) fn scalars(values: &'call [ValueView<'static>]) -> Self {
        Self {
            storage: ArgumentStorage::Scalars(values),
            offset: 0,
            count: values.len(),
            lifetime: PhantomData,
        }
    }

    pub(crate) fn reborrow<'short>(self) -> ArgumentValues<'short>
    where
        'call: 'short,
    {
        ArgumentValues {
            storage: self.storage,
            offset: self.offset,
            count: self.count,
            lifetime: PhantomData,
        }
    }

    pub(crate) fn contiguous(self) -> Option<(SessionId, usize, usize)> {
        let ArgumentStorage::Locals { execution, start } = self.storage else {
            return None;
        };
        Some((execution, start.checked_add(self.offset)?, self.count))
    }

    /// Bounds/source admission is separate from actual value/type admission.
    /// Empty packs still validate the owning source rather than silently pass.
    pub(crate) fn check_bounds(self, slots: &mut Slots<'_, '_>) -> Result<(), GcError> {
        let end = self.offset.checked_add(self.count).ok_or(GcError::Index)?;
        match self.storage {
            ArgumentStorage::Queued(values) => {
                values.check_bounds(slots)?;
                if end > values.len() {
                    return Err(GcError::Index);
                }
            }
            ArgumentStorage::Locals { execution, start } => {
                let end = start.checked_add(end).ok_or(GcError::Index)?;
                if end > slots.len(execution, ValueBank::Local)? {
                    return Err(GcError::Index);
                }
            }
            ArgumentStorage::Prepared {
                execution,
                locations,
                ..
            } => {
                slots.storage(execution)?;
                if end > locations.len() {
                    return Err(GcError::Index);
                }
            }
            ArgumentStorage::Scalars(values) => {
                if end > values.len() {
                    return Err(GcError::Index);
                }
            }
        }
        Ok(())
    }

    pub(crate) fn location(self, index: usize) -> Option<ValueLocation> {
        if index >= self.count {
            return None;
        }
        let index = self.offset.checked_add(index)?;
        match self.storage {
            ArgumentStorage::Queued(values) => values.location(index),
            ArgumentStorage::Locals { execution, start } => Some(ValueLocation {
                execution,
                bank: ValueBank::Local,
                slot: start.checked_add(index)?,
            }),
            ArgumentStorage::Prepared {
                execution,
                managed,
                locations,
                ..
            } => {
                let location = locations.get(index)?;
                if !location.operand.managed() {
                    return None;
                }
                Some(ValueLocation {
                    execution,
                    bank: ValueBank::Managed,
                    slot: managed.checked_add(location.operand.index())?,
                })
            }
            ArgumentStorage::Scalars(_) => None,
        }
    }

    pub(crate) fn scoped<'gc>(
        self,
        slots: &mut Slots<'_, 'gc>,
        index: usize,
    ) -> Result<ScopedValue<'gc>, GcError> {
        if index >= self.count {
            return Err(GcError::Index);
        }
        let input = self.offset.checked_add(index).ok_or(GcError::Index)?;
        match self.storage {
            ArgumentStorage::Queued(values) => values.get(slots, input),
            ArgumentStorage::Locals { .. } => {
                slots.get(self.location(index).ok_or(GcError::Index)?)
            }
            ArgumentStorage::Prepared {
                execution,
                scalars,
                locations,
                ..
            } => {
                let location = *locations.get(input).ok_or(GcError::Index)?;
                if location.operand.is_unit() {
                    slots.storage(execution)?;
                    return location
                        .admits(ScopedValue::unit())
                        .then_some(ScopedValue::unit())
                        .ok_or(GcError::Representation);
                }
                if location.operand.managed() {
                    let source = self.location(index).ok_or(GcError::Index)?;
                    let value = slots.get(source)?;
                    let borrowed = location.borrowed_receiver
                        && matches!(
                            value.kind(),
                            Some(ObjectKind::Ephemeral | ObjectKind::HostPath)
                        );
                    if !borrowed && !value.has_representation(location.representation) {
                        return Err(GcError::Representation);
                    }
                    return Ok(value);
                }
                let slot = scalars
                    .checked_add(location.operand.index())
                    .ok_or(GcError::Index)?;
                let storage = slots.storage(execution)?;
                let bits = *storage.scalars.get(slot).ok_or(GcError::Index)?;
                if !storage
                    .initialized
                    .get(slot)
                    .copied()
                    .ok_or(GcError::Index)?
                    || !domain::admits_payload(location.representation, location.semantic, bits)
                {
                    return Err(GcError::Representation);
                }
                scalar::decode(location.representation, bits).ok_or(GcError::Representation)
            }
            ArgumentStorage::Scalars(values) => {
                let value = *values.get(input).ok_or(GcError::Index)?;
                if value.kind().is_some() {
                    return Err(GcError::Representation);
                }
                value.get(slots)
            }
        }
    }

    pub(crate) fn view<'view>(
        self,
        slots: &mut Slots<'_, '_>,
        index: usize,
    ) -> Result<ValueView<'view>, GcError> {
        let value = self.scoped(slots, index)?;
        ValueView::protected(value, self.location(index))
    }

    pub fn len(self) -> usize {
        self.count
    }

    pub fn is_empty(self) -> bool {
        self.count == 0
    }

    pub fn get<'context>(
        self,
        cx: &mut ConversionContext<'context>,
        index: usize,
    ) -> NativeResult<ValueView<'context>>
    where
        'call: 'context,
    {
        cx.argument_value(self, index)
    }

    /// Select a checked tail without preparing another pack or root owner.
    pub fn tail(mut self, start: usize) -> NativeResult<Self> {
        if start > self.count {
            return Err(RuntimeError::module_validation(
                "argument tail is out of bounds",
            ));
        }
        self.offset = self
            .offset
            .checked_add(start)
            .ok_or_else(|| RuntimeError::module_validation("argument coordinate overflow"))?;
        self.count -= start;
        Ok(self)
    }
}

/// A checked Rust argument mapping borrowed from its fixed signature. Native
/// bindings/applications own preparation; dynamic conversion callers prepare at
/// their actual type boundary before any value converter executes. A mapping
/// belongs to one Runtime and lexical module; using it with another conversion
/// owner fails before any custom converter executes.
pub struct PreparedArguments<'a, A> {
    expected: &'a [TypeArgument],
    owner: (HostRegistryId, ModuleKey),
    marker: PhantomData<fn() -> A>,
}

impl<'a, A: KagariArguments> PreparedArguments<'a, A> {
    /// Validate the complete Rust pack against this conversion owner's signature.
    pub fn new(cx: &ConversionContext<'_>, expected: &'a [TypeArgument]) -> NativeResult<Self> {
        A::check_types(cx, expected)?;
        Ok(Self::from_binding(cx, expected))
    }

    /// Only a monomorphized binding paired with A's successful preparation, or
    /// its checked subpack adapter, may consume this constructor.
    pub(crate) fn from_binding(cx: &ConversionContext<'_>, expected: &'a [TypeArgument]) -> Self {
        Self {
            expected,
            owner: (cx.runtime().host.owner(), cx.owner().key()),
            marker: PhantomData,
        }
    }

    fn validate(&self, cx: &ConversionContext<'_>) -> NativeResult<()> {
        if self.owner != (cx.runtime().host.owner(), cx.owner().key()) {
            return Err(RuntimeError::module_validation(
                "foreign prepared argument mapping",
            ));
        }
        Ok(())
    }

    pub fn types(&self) -> &'a [TypeArgument] {
        self.expected
    }
}

impl<A: IntoKagariArguments> PreparedArguments<'_, A> {
    /// Encode actual values using the checked pack; preserve per-value effects.
    pub fn encode<'call>(
        &self,
        cx: &mut ConversionContext<'call>,
        arguments: A,
    ) -> NativeResult<ArgumentValues<'call>> {
        self.validate(cx)?;
        cx.check_elements(self.expected.len())?;
        cx.encode_argument_pack(|cx| arguments.encode_arguments(cx, self))
    }
}

impl<A: FromKagariArguments> PreparedArguments<'_, A> {
    /// Validate all input values before decoding the first custom converter.
    pub fn decode<'source, 'call>(
        &self,
        cx: &mut ConversionContext<'call>,
        values: ArgumentValues<'source>,
    ) -> NativeResult<A>
    where
        'source: 'call,
    {
        let values = values.reborrow();
        self.validate(cx)?;
        cx.validate_arguments(values)?;
        check_arity(self.expected, values.len())?;
        cx.check_elements(values.len())?;
        for (index, ty) in self.expected.iter().enumerate() {
            let value = values.get(cx, index)?;
            cx.check_value(ty, &value)?;
        }
        cx.argument_scope(|cx| A::decode_arguments(cx, self, values))
    }

    /// Native invocation admitted every input, including its declared receiver
    /// policy, before the first converter. Keep that one complete-pack decision.
    pub(crate) fn decode_admitted<'source, 'call>(
        &self,
        cx: &mut ConversionContext<'call>,
        values: ArgumentValues<'source>,
    ) -> NativeResult<A>
    where
        'source: 'call,
    {
        let values = values.reborrow();
        self.validate(cx)?;
        cx.validate_arguments(values)?;
        check_arity(self.expected, values.len())?;
        cx.argument_scope(|cx| A::decode_arguments(cx, self, values))
    }

    /// The outer binding admitted the full immutable pack before any converter.
    pub(crate) fn decode_tail<'source, 'call>(
        &self,
        cx: &mut ConversionContext<'call>,
        values: ArgumentValues<'source>,
    ) -> NativeResult<A>
    where
        'source: 'call,
    {
        let values = values.reborrow();
        self.validate(cx)?;
        cx.validate_arguments(values)?;
        check_arity(self.expected, values.len())?;
        A::decode_arguments(cx, self, values)
    }
}

pub trait KagariArguments {
    fn argument_types(catalog: &DeclarationCatalog) -> NativeResult<Vec<Type>>;

    fn check_types(cx: &ConversionContext<'_>, expected: &[TypeArgument]) -> NativeResult<()> {
        let types = Self::argument_types(&cx.runtime().native_entries.catalog)?;
        check_arity(expected, types.len())?;
        for (expected, ty) in expected.iter().zip(types) {
            cx.check_declared_type(expected, ty)?;
        }
        Ok(())
    }
}

pub trait IntoKagariArguments: KagariArguments + Sized {
    /// Encode into the caller's protected source suffix. The prepared mapping
    /// admits Rust types before conversion effects; failed packs retire together.
    fn encode_arguments<'call>(
        self,
        cx: &mut ConversionContext<'call>,
        prepared: &PreparedArguments<'_, Self>,
    ) -> NativeResult<ArgumentValues<'call>>;
}

pub trait FromKagariArguments: KagariArguments + Sized {
    /// Decode an admitted immutable prefix. The invocation owner checked every
    /// actual value before the first converter; subpacks reuse that admission.
    fn decode_arguments<'call>(
        cx: &mut ConversionContext<'call>,
        prepared: &PreparedArguments<'_, Self>,
        values: ArgumentValues<'call>,
    ) -> NativeResult<Self>;
}

fn check_arity(expected: &[TypeArgument], actual: usize) -> NativeResult<()> {
    if expected.len() != actual {
        return Err(RuntimeError::module_validation(
            "typed argument count differs from its declaration",
        ));
    }
    Ok(())
}

macro_rules! arguments {
    ($count:expr; $($ty:ident:$slot:tt),*) => {
        impl<$($ty: KagariType),*> KagariArguments for ($($ty,)*) {
            fn argument_types(_catalog: &DeclarationCatalog) -> NativeResult<Vec<Type>> {
                Ok(vec![$($ty::kagari_type(_catalog)?),*])
            }

            fn check_types(_cx: &ConversionContext<'_>, expected: &[TypeArgument]) -> NativeResult<()> {
                check_arity(expected, $count)?;
                $(_cx.check_type::<$ty>(&expected[$slot])?;)*
                Ok(())
            }
        }

        impl<$($ty: IntoKagari),*> IntoKagariArguments for ($($ty,)*) {
            fn encode_arguments<'call>(self, cx: &mut ConversionContext<'call>, prepared: &PreparedArguments<'_, Self>) -> NativeResult<ArgumentValues<'call>> {
                let expected = prepared.types();
                check_arity(expected, $count)?;
                let values = [$(cx.encode_prepared(&expected[$slot], self.$slot)?),*];
                cx.argument_values(&values)
            }
        }

        impl<$($ty: FromKagari),*> FromKagariArguments for ($($ty,)*) {
            fn decode_arguments<'call>(_cx: &mut ConversionContext<'call>, prepared: &PreparedArguments<'_, Self>, values: ArgumentValues<'call>) -> NativeResult<Self> {
                let expected = prepared.types();
                check_arity(expected, $count)?;
                if values.len() != $count { return Err(RuntimeError::module_validation("typed input argument count")); }
                Ok(($({
                    let value = values.get(_cx, $slot)?;
                    _cx.decode_admitted::<$ty>(&expected[$slot], &value)?
                },)*))
            }
        }
    };
}

arguments!(0;);
arguments!(1; A:0);
arguments!(2; A:0, B:1);
arguments!(3; A:0, B:1, C:2);
arguments!(4; A:0, B:1, C:2, D:3);
arguments!(5; A:0, B:1, C:2, D:3, E:4);
arguments!(6; A:0, B:1, C:2, D:3, E:4, F:5);
arguments!(7; A:0, B:1, C:2, D:3, E:4, F:5, G:6);
arguments!(8; A:0, B:1, C:2, D:3, E:4, F:5, G:6, H:7);
arguments!(9; A:0, B:1, C:2, D:3, E:4, F:5, G:6, H:7, I:8);
arguments!(10; A:0, B:1, C:2, D:3, E:4, F:5, G:6, H:7, I:8, J:9);
arguments!(11; A:0, B:1, C:2, D:3, E:4, F:5, G:6, H:7, I:8, J:9, K:10);
arguments!(12; A:0, B:1, C:2, D:3, E:4, F:5, G:6, H:7, I:8, J:9, K:10, L:11);
