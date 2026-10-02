//! Ordinary Rust outputs preserve exact scalar contracts or a checked value boundary.
use crate::{
    native::{binding::Codec, scalar::NativeScalar},
    value::Value,
};

mod sealed {
    use crate::{native::scalar::NativeScalar, value::Value};
    pub trait Output {}
    impl<T: NativeScalar> Output for T {}
    impl Output for Value {}
}
pub trait NativeOutput: sealed::Output + Sized + 'static {
    const SCALAR: bool;
    fn codec() -> Codec;
    fn encode(self) -> Value;
}
impl<T: NativeScalar> NativeOutput for T {
    const SCALAR: bool = true;
    fn codec() -> Codec {
        Codec::Scalar(T::abi_type())
    }
    fn encode(self) -> Value {
        NativeScalar::encode(self)
    }
}
impl NativeOutput for Value {
    const SCALAR: bool = false;
    fn codec() -> Codec {
        Codec::Value
    }
    fn encode(self) -> Value {
        self
    }
}
