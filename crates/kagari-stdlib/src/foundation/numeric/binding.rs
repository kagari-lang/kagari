//! Exact scalar adapters shared by numeric method declarations.
use kagari_runtime::native::{
    binding::{Codec, NativeBinding},
    scalar::NativeScalar,
    types::Type,
};

#[cfg(test)]
mod tests;

pub(super) fn unary<A: NativeScalar, R: NativeScalar>(operation: fn(A) -> R) -> NativeBinding {
    unsafe {
        NativeBinding::new_transport(
            vec![Codec::Exact(Type::scalar(A::abi_type()))],
            Codec::Exact(Type::scalar(R::abi_type())),
            // SAFETY: Exact scalar codecs are checked before entry; no managed edge escapes decoding.
            move |cx| Ok(operation(A::decode(cx.argument_transport(0)?)?).encode()),
        )
    }
}

pub(super) fn binary<T: NativeScalar, A: NativeScalar>(operation: fn(T, A) -> T) -> NativeBinding {
    let scalar = Type::scalar(T::abi_type());
    unsafe {
        NativeBinding::new_transport(
            vec![
                Codec::Exact(scalar.clone()),
                Codec::Exact(Type::scalar(A::abi_type())),
            ],
            Codec::Exact(scalar),
            move |cx| {
                // SAFETY: Exact scalar codecs are checked before entry; no managed edge escapes decoding.
                let value = T::decode(cx.argument_transport(0)?)?;
                // SAFETY: Exact scalar codecs are checked before entry; no managed edge escapes decoding.
                let other = A::decode(cx.argument_transport(1)?)?;
                Ok(operation(value, other).encode())
            },
        )
    }
}
