//! An application-owned Future role for generic Runtime lifecycle tests.
use crate::native::{builder::ModuleBuilder, module::NativeModule, storage::NativeStorage};

pub(crate) fn provider() -> NativeModule {
    let mut module = ModuleBuilder::new("fixture::lifecycle", &Default::default()).unwrap();
    let mut pending = module.define_type("Pending").unwrap();
    pending.type_parameter("T").unwrap();
    pending.native_storage(NativeStorage::future()).unwrap();
    pending.finish().unwrap();
    module.finish().unwrap()
}
