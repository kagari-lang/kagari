//! Engine's default selection; every selected module is an ordinary native package.
use crate::native::{api::NativeApi, array_api};

pub fn standard_library() -> NativeApi {
    NativeApi::combine(vec![
        array_api::array::native_api().expect("bundled array API"),
    ])
    .expect("default native packages")
}
