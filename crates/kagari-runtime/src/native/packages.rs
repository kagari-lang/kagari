//! Engine's default selection; every selected module is an ordinary native package.
use crate::native::{api::NativeApi, array_api::array, math_api, ops_api::ops};

pub fn standard_library() -> NativeApi {
    let ops = ops::native_api().expect("bundled operator API");
    let array = array::native_api(&ops.catalog()).expect("bundled array API");
    NativeApi::combine(vec![
        ops,
        array,
        math_api::math::native_api().expect("bundled math API"),
    ])
    .expect("default native packages")
}
