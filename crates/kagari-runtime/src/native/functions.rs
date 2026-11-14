//! Rust argument converters checked against explicit Kagari declarations.
use crate::{
    native::{
        binding::{Codec, NativeBinding, NativeResult},
        callable::CallableHandle,
        context::NativeCall,
        scalar::NativeScalar,
        types::Type,
        views::ValueHandle,
    },
    value::view::ValueView,
};

pub trait NativeFunction<Arguments, Result> {
    fn binding(self) -> NativeBinding;
}

impl<F, R> NativeFunction<(), R> for F
where
    F: for<'call> Fn(&mut NativeCall<'call>) -> NativeResult<R> + Send + Sync + 'static,
    R: NativeScalar,
{
    fn binding(self) -> NativeBinding {
        let mut binding = NativeBinding::new(
            Vec::<Codec>::new(),
            Codec::Exact(Type::scalar(R::abi_type())),
            move |cx| self(cx).map(R::encode),
        );
        binding.converted_result = true;
        binding
    }
}

/// Inference marker for a value returned with the Native call's lifetime.
#[doc(hidden)]
pub struct ValueResult;

impl<F> NativeFunction<(), ValueResult> for F
where
    F: for<'call> Fn(&mut NativeCall<'call>) -> NativeResult<ValueView<'call>>
        + Send
        + Sync
        + 'static,
{
    fn binding(self) -> NativeBinding {
        NativeBinding::new(Vec::<Codec>::new(), Codec::Value, move |cx| self(cx))
    }
}

macro_rules! function {
    ($($argument:ident:$local:ident:$slot:expr),+) => {
        impl<F, R, $($argument),+> NativeFunction<($($argument,)+), R> for F
        where F: for<'call> Fn(&mut NativeCall<'call>, $($argument),+) -> NativeResult<R> + Send + Sync + 'static,
            R: NativeScalar, $($argument: NativeScalar),+ {
            fn binding(self) -> NativeBinding {
                let mut binding = NativeBinding::new(vec![$(Codec::Exact(Type::scalar($argument::abi_type()))),+],
                    Codec::Exact(Type::scalar(R::abi_type())), move |cx| {
                        $(let $local = $argument::decode(cx.argument($slot)?)?;)+
                        self(cx, $($local),+).map(R::encode)
                    });
                binding.converted_result = true;
                binding
            }
        }

        impl<F, $($argument),+> NativeFunction<($($argument,)+), ValueResult> for F
        where F: for<'call> Fn(&mut NativeCall<'call>, $($argument),+) -> NativeResult<ValueView<'call>>
                + Send + Sync + 'static,
            $($argument: NativeScalar),+ {
            fn binding(self) -> NativeBinding {
                NativeBinding::new(vec![$(Codec::Exact(Type::scalar($argument::abi_type()))),+],
                    Codec::Value, move |cx| {
                        $(let $local = $argument::decode(cx.argument($slot)?)?;)+
                        self(cx, $($local),+)
                    })
            }
        }
    };
}

function!(A:a:0);
function!(A:a:0, B:b:1);
function!(A:a:0, B:b:1, C:c:2);

macro_rules! view_type {
    ($call:lifetime; view $view:ident) => { $view<$call> };
    ($call:lifetime; scalar $scalar:ident) => { $scalar };
}

macro_rules! argument_codec {
    (view $codec:ident) => {
        Codec::$codec
    };
    (scalar $scalar:ident) => {
        Codec::Exact(Type::scalar($scalar::abi_type()))
    };
}

macro_rules! argument_value {
    ($cx:ident, $slot:expr; view $view:ident) => {
        $view::from_argument($cx, $slot)?
    };
    ($cx:ident, $slot:expr; scalar $scalar:ident) => {
        $scalar::decode($cx.argument($slot)?)?
    };
}

macro_rules! view_function {
    ([$($scalar:ident),*]; $($marker:ty => $kind:ident $type:ident, $codec:ident, $local:ident:$slot:expr);+) => {
        impl<F, R, $($scalar),*> NativeFunction<($($marker,)+), R> for F
        where F: for<'call> Fn(&mut NativeCall<'call>, $(view_type!('call; $kind $type)),+) -> NativeResult<R> + Send + Sync + 'static,
            R: NativeScalar, $($scalar: NativeScalar),* {
            fn binding(self) -> NativeBinding {
                let mut binding = NativeBinding::new(
                    vec![$(argument_codec!($kind $codec)),+], Codec::Exact(Type::scalar(R::abi_type())), move |cx| {
                        $(let $local = argument_value!(cx, $slot; $kind $type);)+
                        self(cx, $($local),+).map(R::encode)
                    });
                binding.converted_result = true;
                binding
            }
        }

        impl<F, $($scalar),*> NativeFunction<($($marker,)+), ValueResult> for F
        where F: for<'call> Fn(&mut NativeCall<'call>, $(view_type!('call; $kind $type)),+) -> NativeResult<ValueView<'call>>
                + Send + Sync + 'static,
            $($scalar: NativeScalar),* {
            fn binding(self) -> NativeBinding {
                NativeBinding::new(
                    vec![$(argument_codec!($kind $codec)),+], Codec::Value, move |cx| {
                        $(let $local = argument_value!(cx, $slot; $kind $type);)+
                        self(cx, $($local),+)
                    })
            }
        }
    };
}

macro_rules! single_view_functions {
    ($marker:ident, $view:ident, $codec:ident) => {
        view_function!([]; $marker => view $view, $codec, values:0);
        view_function!([A]; $marker => view $view, $codec, values:0; A => scalar A, A, a:1);
        view_function!([A]; A => scalar A, A, a:0; $marker => view $view, $codec, values:1);
        view_function!([A, B]; $marker => view $view, $codec, values:0; A => scalar A, A, a:1; B => scalar B, B, b:2);
        view_function!([A, B]; A => scalar A, A, a:0; $marker => view $view, $codec, values:1; B => scalar B, B, b:2);
        view_function!([A, B]; A => scalar A, A, a:0; B => scalar B, B, b:1; $marker => view $view, $codec, values:2);
    };
}

/// Inference marker for a function-typed argument.
#[doc(hidden)]
pub struct CallableArgument;

single_view_functions!(CallableArgument, CallableHandle, Callable);

/// Inference marker for a declared generic argument.
#[doc(hidden)]
pub struct ValueArgument;

single_view_functions!(ValueArgument, ValueHandle, Value);
view_function!([]; ValueArgument => view ValueHandle, Value, value:0; CallableArgument => view CallableHandle, Callable, callback:1);
