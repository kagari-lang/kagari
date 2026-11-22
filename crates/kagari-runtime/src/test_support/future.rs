//! Minimal provider for the language-owned Future type, without stdlib.
use crate::native::{builder::ModuleBuilder, module::NativeModule, storage::NativeStorage};

pub(crate) fn provider() -> NativeModule {
    let mut module = ModuleBuilder::new("kagari-core::future", &Default::default()).unwrap();
    let mut future = module.define_type("Future").unwrap();
    future.type_parameter("T").unwrap();
    future.native_storage(NativeStorage::future()).unwrap();
    future.finish().unwrap();
    module.finish().unwrap()
}
