//! Exact scalar adapters shared by numeric method declarations.
use kagari_runtime::native::{
    binding::{Codec, NativeBinding},
    scalar::NativeScalar,
    types::Type,
};

#[cfg(test)]
mod tests;

pub(super) fn unary<A: NativeScalar, R: NativeScalar>(operation: fn(A) -> R) -> NativeBinding {
    NativeBinding::new(
        vec![Codec::Exact(Type::scalar(A::abi_type()))],
        Codec::Exact(Type::scalar(R::abi_type())),
        move |cx| Ok(operation(A::decode(cx.argument(0)?)?).encode()),
    )
}

pub(super) fn binary<T: NativeScalar, A: NativeScalar>(operation: fn(T, A) -> T) -> NativeBinding {
    let scalar = Type::scalar(T::abi_type());
    NativeBinding::new(
        vec![
            Codec::Exact(scalar.clone()),
            Codec::Exact(Type::scalar(A::abi_type())),
        ],
        Codec::Exact(scalar),
        move |cx| {
            let value = T::decode(cx.argument(0)?)?;
            let other = A::decode(cx.argument(1)?)?;
            Ok(operation(value, other).encode())
        },
    )
}
