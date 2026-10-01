//! Engine's default selection; every selected module is an ordinary native package.
use crate::native::{
    api::NativeApi, array_api::array, catalog::NativeCatalog, cmp_api::cmp, math_api, ops_api::ops,
};

pub fn standard_library() -> NativeApi {
    let ops = ops::native_api().expect("bundled operator API");
    let cmp = cmp::native_api().expect("bundled comparison API");
    let catalog = NativeCatalog::from_apis(&[&ops, &cmp]).expect("bundled array dependencies");
    let array = array::native_api(&catalog).expect("bundled array API");
    NativeApi::combine(vec![
        ops,
        array,
        cmp,
        math_api::math::native_api().expect("bundled math API"),
    ])
    .expect("default native packages")
}
