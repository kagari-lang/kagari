//! Engine's default selection; every selected module is an ordinary native package.
use crate::native::{
    api::NativeApi, array_api::array, catalog::NativeCatalog, cmp_api::cmp, debug_api::debug,
    math_api, numeric_api::numeric, ops_api::ops, option_api::option, result_api::result,
    string_api::string,
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
        debug::native_api().expect("bundled debug API"),
        math_api::math::native_api().expect("bundled math API"),
        numeric::native_api().expect("bundled numeric API"),
        option::native_api().expect("bundled optional API"),
        result::native_api().expect("bundled result API"),
        string::native_api().expect("bundled string API"),
    ])
    .expect("default native packages")
}
