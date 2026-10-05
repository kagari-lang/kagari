//! Rust argument converters checked against explicit Kagari declarations.
use crate::native::{
    binding::{Codec, NativeBinding, NativeResult},
    callable::CallableHandle,
    context::CallContext,
    returns::NativeOutput,
    scalar::NativeScalar,
    views::{SequenceHandle, SequenceMutHandle, ValueHandle},
};

pub trait NativeFunction<Arguments, Result> {
    fn binding(self) -> NativeBinding;
}

impl<F, R> NativeFunction<(), R> for F
where
    F: for<'call> Fn(&mut CallContext<'call>) -> NativeResult<R> + Send + Sync + 'static,
    R: NativeOutput,
{
    fn binding(self) -> NativeBinding {
        let mut binding = NativeBinding::new(Vec::<Codec>::new(), R::codec(), move |cx| {
            self(cx).map(R::encode)
        });
        binding.converted_result = R::SCALAR;
        binding
    }
}

macro_rules! function {
    ($($argument:ident:$local:ident:$slot:expr),+) => {
        impl<F, R, $($argument),+> NativeFunction<($($argument,)+), R> for F
        where F: for<'call> Fn(&mut CallContext<'call>, $($argument),+) -> NativeResult<R> + Send + Sync + 'static,
            R: NativeOutput, $($argument: NativeScalar),+ {
            fn binding(self) -> NativeBinding {
                let mut binding = NativeBinding::new(vec![$(Codec::Scalar($argument::abi_type())),+],
                    R::codec(), move |cx| {
                        $(let $local = $argument::decode(cx.argument($slot)?)?;)+
                        self(cx, $($local),+).map(R::encode)
                    });
                binding.converted_result = R::SCALAR;
                binding
            }
        }
    };
}

function!(A:a:0);
function!(A:a:0, B:b:1);
function!(A:a:0, B:b:1, C:c:2);

/// Inference marker for a call-scoped sequence parameter, never a runtime value.
#[doc(hidden)]
pub struct SequenceArgument;

/// Inference marker for a mutable call-scoped sequence parameter.
#[doc(hidden)]
pub struct SequenceMutArgument;

macro_rules! view_type {
    ($call:lifetime; view $view:ident) => { $view<$call> };
    ($call:lifetime; scalar $scalar:ident) => { $scalar };
}

macro_rules! argument_codec {
    (view $codec:ident) => {
        Codec::$codec
    };
    (scalar $scalar:ident) => {
        Codec::Scalar($scalar::abi_type())
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
        where F: for<'call> Fn(&mut CallContext<'call>, $(view_type!('call; $kind $type)),+) -> NativeResult<R> + Send + Sync + 'static,
            R: NativeOutput, $($scalar: NativeScalar),* {
            fn binding(self) -> NativeBinding {
                let mut binding = NativeBinding::new(
                    vec![$(argument_codec!($kind $codec)),+], R::codec(), move |cx| {
                        $(let $local = argument_value!(cx, $slot; $kind $type);)+
                        self(cx, $($local),+).map(R::encode)
                    });
                binding.converted_result = R::SCALAR;
                binding
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

single_view_functions!(SequenceArgument, SequenceHandle, Sequence);
single_view_functions!(SequenceMutArgument, SequenceMutHandle, MutableSequence);

/// Inference marker for a function-typed argument.
#[doc(hidden)]
pub struct CallableArgument;

single_view_functions!(CallableArgument, CallableHandle, Callable);

/// Inference marker for a declared generic argument.
#[doc(hidden)]
pub struct ValueArgument;

single_view_functions!(ValueArgument, ValueHandle, Value);
view_function!([]; SequenceArgument => view SequenceHandle, Sequence, values:0; CallableArgument => view CallableHandle, Callable, callback:1);
view_function!([]; SequenceMutArgument => view SequenceMutHandle, MutableSequence, values:0; CallableArgument => view CallableHandle, Callable, callback:1);
view_function!([]; ValueArgument => view ValueHandle, Value, value:0; CallableArgument => view CallableHandle, Callable, callback:1);
