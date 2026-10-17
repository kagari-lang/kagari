//! Explicit Default bodies; no constructor is inferred from a result type or method name.
use crate::bindings::option;
use kagari_runtime::native::{
    binding::{Codec, NativeBinding},
    scalar::NativeScalar,
    types::Type,
};
use kagari_types::scalar::BuiltinType;

pub(super) fn binding(name: &str) -> Option<NativeBinding> {
    Some(match name {
        "$foundation_default_Unit" => constant(()),
        "$foundation_default_Bool" => constant(false),
        "$foundation_default_I8" => constant(0_i8),
        "$foundation_default_I16" => constant(0_i16),
        "$foundation_default_I32" => constant(0_i32),
        "$foundation_default_I64" => constant(0_i64),
        "$foundation_default_ISize" => constant(0_isize),
        "$foundation_default_U8" => constant(0_u8),
        "$foundation_default_U16" => constant(0_u16),
        "$foundation_default_U32" => constant(0_u32),
        "$foundation_default_U64" => constant(0_u64),
        "$foundation_default_USize" => constant(0_usize),
        "$foundation_default_F32" => constant(0.0_f32),
        "$foundation_default_F64" => constant(0.0_f64),
        "$foundation_default_String" => NativeBinding::new(
            vec![],
            Codec::Exact(Type::scalar(BuiltinType::String)),
            |cx| cx.encode_result(String::new()),
        ),
        "$foundation_default_Vec" => {
            NativeBinding::new(vec![], Codec::Value, |cx| cx.allocate_result())
        }
        "$foundation_default_Option" => {
            NativeBinding::new(vec![], Codec::Value, |cx| option(cx, None))
        }
        _ => return None,
    })
}

fn constant<T: NativeScalar>(value: T) -> NativeBinding {
    let value = value.encode();
    NativeBinding::new(
        vec![],
        Codec::Exact(Type::scalar(T::abi_type())),
        move |_| Ok(value),
    )
}
