//! Composite schemas reuse ordinary conversion type constructors and registration.
use super::{KagariSchema, Serde};
use crate::native::{
    binding::NativeResult, catalog::DeclarationCatalog, conversion::KagariType,
    schema::TypeRegistration, types::Type,
};
use std::marker::PhantomData;

struct SchemaValue<T>(PhantomData<T>);

impl<T: KagariSchema> KagariType for SchemaValue<T> {
    fn kagari_type(catalog: &DeclarationCatalog) -> NativeResult<Type> {
        T::schema_type(catalog)
    }

    fn register_type(registration: &mut TypeRegistration<'_>) -> NativeResult<Type> {
        registration.dependency::<Serde<T>>()
    }
}

macro_rules! scalar_schema {
    ($($ty:ty),+) => { $(
        impl KagariSchema for $ty {
            fn schema_type(catalog: &DeclarationCatalog) -> NativeResult<Type> {
                <$ty as KagariType>::kagari_type(catalog)
            }

            fn register_schema(registration: &mut TypeRegistration<'_>) -> NativeResult<Type> {
                <$ty as KagariType>::register_type(registration)
            }
        }
    )+ };
}

scalar_schema!(
    (),
    bool,
    i8,
    i16,
    i32,
    i64,
    isize,
    u8,
    u16,
    u32,
    u64,
    usize,
    f32,
    f64,
    String
);

macro_rules! composite_schema {
    ($container:ident<$($ty:ident),+>) => {
        impl<$($ty: KagariSchema),+> KagariSchema for $container<$($ty),+> {
            fn schema_type(catalog: &DeclarationCatalog) -> NativeResult<Type> {
                <$container<$(SchemaValue<$ty>),+>>::kagari_type(catalog)
            }

            fn register_schema(registration: &mut TypeRegistration<'_>) -> NativeResult<Type> {
                <$container<$(SchemaValue<$ty>),+>>::register_type(registration)
            }
        }
    };
}

composite_schema!(Vec<T>);
composite_schema!(Option<T>);
composite_schema!(Result<T, E>);

macro_rules! tuple_schema {
    ($($ty:ident),+) => {
        impl<$($ty: KagariSchema),+> KagariSchema for ($($ty,)+) {
            fn schema_type(catalog: &DeclarationCatalog) -> NativeResult<Type> {
                <($(SchemaValue<$ty>,)+)>::kagari_type(catalog)
            }

            fn register_schema(registration: &mut TypeRegistration<'_>) -> NativeResult<Type> {
                <($(SchemaValue<$ty>,)+)>::register_type(registration)
            }
        }
    };
}

tuple_schema!(A);
tuple_schema!(A, B);
tuple_schema!(A, B, C);
tuple_schema!(A, B, C, D);
tuple_schema!(A, B, C, D, E);
tuple_schema!(A, B, C, D, E, F);
tuple_schema!(A, B, C, D, E, F, G);
tuple_schema!(A, B, C, D, E, F, G, H);
tuple_schema!(A, B, C, D, E, F, G, H, I);
tuple_schema!(A, B, C, D, E, F, G, H, I, J);
tuple_schema!(A, B, C, D, E, F, G, H, I, J, K);
tuple_schema!(A, B, C, D, E, F, G, H, I, J, K, L);
