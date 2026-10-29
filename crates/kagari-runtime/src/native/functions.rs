//! Rust argument converters checked against explicit Kagari declarations.
use crate::{
    native::{
        adapter::{Flat, NativeAdapter},
        binding::{Codec, NativeBinding, NativeResult},
        callable::CallableHandle,
        catalog::DeclarationCatalog,
        context::CallContext,
        returns::NativeOutput,
        scalar::NativeScalar,
        types::Type,
        views::ValueHandle,
    },
    value::Value,
};
use kagari_types::declaration::FnDecl;

/// Low-level flat argument adaptation; raw results require provider admission.
pub trait NativeFunction<Arguments, Result> {
    /// Prepare one callback without executing it.
    ///
    /// # Safety
    /// Raw-result providers must protect captured edges and keep managed results
    /// live until caller publication. Source values crossing callbacks require
    /// independent protection. Scalar-only callbacks have no managed result edge.
    unsafe fn binding_transport(self) -> NativeBinding;
}

/// An explicitly admitted flat callback returning raw backend transport.
///
/// Ordinary scalar callbacks bind directly. Owning conversions use NativeContext
/// and IntoKagari. This wrapper reuses flat argument adaptation without exposing a
/// safe raw-result registration route.
///
/// Raw-returning functions cannot use the safe flat adapter:
///
/// ```compile_fail
/// use kagari_runtime::{
///     native::{adapter::{Flat, NativeAdapter}, context::CallContext},
///     value::Value,
/// };
/// fn accept(callback: impl NativeAdapter<Flat<()>, Value>) {}
/// accept(|_: &mut CallContext<'_>| Ok(Value::Unit));
/// ```
pub struct NativeCallback<F> {
    callback: F,
}

impl<F> NativeCallback<F> {
    /// Admit a provider that returns raw Value results.
    ///
    /// # Safety
    /// Keep managed captures rooted for their entire use. Protect extracted
    /// arguments/results across collection and reentry, release storage loans
    /// before callbacks, and return live values for immediate caller publication.
    /// Scoped host capabilities still require their original host scope.
    pub unsafe fn new(callback: F) -> Self {
        Self { callback }
    }
}

impl<F, A> NativeAdapter<Flat<A>, Value> for NativeCallback<F>
where
    F: NativeFunction<A, Value>,
{
    fn prepare(self, _: &FnDecl, _: &DeclarationCatalog) -> NativeResult<NativeBinding> {
        // SAFETY: Construction admitted the provider's raw-result lifetime contract.
        Ok(unsafe { self.callback.binding_transport() })
    }
}

impl<F, R> NativeFunction<(), R> for F
where
    F: for<'call> Fn(&mut CallContext<'call>) -> NativeResult<R> + Send + Sync + 'static,
    R: NativeOutput,
{
    unsafe fn binding_transport(self) -> NativeBinding {
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
            unsafe fn binding_transport(self) -> NativeBinding {
                let mut binding = NativeBinding::new(vec![$(Codec::Exact(Type::scalar($argument::abi_type()))),+],
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
        where F: for<'call> Fn(&mut CallContext<'call>, $(view_type!('call; $kind $type)),+) -> NativeResult<R> + Send + Sync + 'static,
            R: NativeOutput, $($scalar: NativeScalar),* {
            unsafe fn binding_transport(self) -> NativeBinding {
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

/// Inference marker for a function-typed argument.
#[doc(hidden)]
pub struct CallableArgument;

single_view_functions!(CallableArgument, CallableHandle, Callable);

/// Inference marker for a declared generic argument.
#[doc(hidden)]
pub struct ValueArgument;

single_view_functions!(ValueArgument, ValueHandle, Value);
view_function!([]; ValueArgument => view ValueHandle, Value, value:0; CallableArgument => view CallableHandle, Callable, callback:1);
