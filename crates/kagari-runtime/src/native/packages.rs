//! Engine's default selection; every selected module is an ordinary native package.
use crate::{NativeApi, native::array_api};

pub fn standard_library() -> NativeApi {
    NativeApi::combine(vec![array_api::api().expect("bundled array API")])
        .expect("default native packages")
}
