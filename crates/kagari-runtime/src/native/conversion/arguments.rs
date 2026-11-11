//! The outer Rust tuple is an argument list; its elements each convert one value.
use crate::{
    error::RuntimeError,
    frame::types::arguments::TypeArgument,
    gc::roots::RootSet,
    host::HostRegistryId,
    module::ModuleKey,
    native::{
        binding::NativeResult,
        catalog::DeclarationCatalog,
        conversion::{FromKagari, IntoKagari, KagariType, context::ConversionContext},
        types::Type,
    },
    value::Value,
};
use std::marker::PhantomData;

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
    pub fn encode(&self, cx: &mut ConversionContext<'_>, arguments: A) -> NativeResult<RootSet> {
        self.validate(cx)?;
        arguments.encode_arguments(cx, self)
    }
}

impl<A: FromKagariArguments> PreparedArguments<'_, A> {
    /// Validate all input values before decoding the first custom converter.
    pub fn decode(&self, cx: &mut ConversionContext<'_>, values: &[Value]) -> NativeResult<A> {
        self.validate(cx)?;
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
    /// Encode a completely mapped pack. Actual values and custom conversion
    /// effects remain checked in the supplied conversion scope.
    fn encode_arguments(
        self,
        cx: &mut ConversionContext<'_>,
        prepared: &PreparedArguments<'_, Self>,
    ) -> NativeResult<RootSet>;
}

pub trait FromKagariArguments: KagariArguments + Sized {
    /// Decode an already mapped pack, validating every actual value before the
    /// first user converter. Retain the complete snapshot through conversion.
    fn decode_arguments(
        cx: &mut ConversionContext<'_>,
        prepared: &PreparedArguments<'_, Self>,
        values: &[Value],
    ) -> NativeResult<Self>;
}

/// Own the argument roots until the backend result has been retained or decoded.
pub(crate) fn encode_arguments<A: IntoKagariArguments>(
    cx: &mut ConversionContext<'_>,
    prepared: PreparedArguments<'_, A>,
    arguments: A,
) -> NativeResult<(RootSet, Vec<Value>)> {
    let count = prepared.types().len();
    let roots = prepared
        .encode(cx, arguments)
        .map_err(|error| cx.runtime().resources().admit_execution_error(error))?;
    let mut values = Vec::new();
    values
        .try_reserve_exact(count)
        .map_err(|_| cx.runtime().resources().limit("typed call arguments"))?;
    for slot in 0..count {
        values.push(
            roots
                .get(cx.runtime().gc(), slot)
                .ok_or_else(|| RuntimeError::module_validation("typed call argument root"))?,
        );
    }
    Ok((roots, values))
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
            fn encode_arguments(self, cx: &mut ConversionContext<'_>, prepared: &PreparedArguments<'_, Self>) -> NativeResult<RootSet> {
                let expected = prepared.types();
                check_arity(expected, $count)?;
                cx.check_elements($count)?;
                cx.argument_scope(|cx| {
                    let values = vec![$(cx.encode_prepared(&expected[$slot], self.$slot)?),*];
                    cx.runtime().gc().root_execution_values(values)
                        .ok_or_else(|| RuntimeError::module_validation("typed argument roots"))
                })
            }
        }

        impl<$($ty: FromKagari),*> FromKagariArguments for ($($ty,)*) {
            fn decode_arguments(cx: &mut ConversionContext<'_>, prepared: &PreparedArguments<'_, Self>, values: &[Value]) -> NativeResult<Self> {
                let expected = prepared.types();
                check_arity(expected, $count)?;
                if values.len() != $count { return Err(RuntimeError::module_validation("typed input argument count")); }
                cx.check_elements($count)?;
                for (ty, value) in expected.iter().zip(values) { cx.check_value(ty, value)?; }
                let roots = cx.runtime().gc().root_execution_values(values.to_vec())
                    .ok_or_else(|| RuntimeError::module_validation("typed input argument roots"))?;
                let result = cx.argument_scope(|_cx| {
                    Ok(($({
                        let value = roots.get(_cx.runtime().gc(), $slot)
                            .ok_or_else(|| RuntimeError::module_validation("typed input argument slot"))?;
                        _cx.decode_prepared::<$ty>(&expected[$slot], &value)?
                    },)*))
                });
                drop(roots);
                result
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
