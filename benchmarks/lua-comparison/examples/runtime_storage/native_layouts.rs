//! Real native payload layouts exercise ordinary registration and aligned destruction.
use kagari::engine::{EngineConfig, KagariEngine};
use kagari_runtime::{
    native::{
        binding::{NativeBinding, NativeResult},
        builder::ModuleBuilder,
        declarations::FunctionDecl,
        storage::{NativePayload, NativeStorage},
    },
    resource::HeapCharge,
    value::Value,
};
use std::mem::{align_of, size_of};

macro_rules! payload {
    ($name:ident, $size:literal, $align:literal) => {
        #[derive(Debug)]
        #[repr(C, align($align))]
        struct $name([u8; $size]);

        impl NativePayload for $name {
            fn trace<'a>(&'a self, _: &mut dyn FnMut(&'a Value)) {
                assert_eq!((self as *const Self as usize) % align_of::<Self>(), 0);
                assert_eq!((self.0[0], self.0[$size - 1]), (42, 42));
            }

            fn charge(&self) -> NativeResult<HeapCharge> {
                Ok(HeapCharge::new(1, 0))
            }
        }

        impl Drop for $name {
            fn drop(&mut self) {
                assert_eq!((self as *const Self as usize) % align_of::<Self>(), 0);
                assert_eq!((self.0[0], self.0[$size - 1]), (42, 42));
            }
        }
    };
}

payload!(S8, 8, 8);
payload!(S24, 24, 8);
payload!(S48, 48, 16);
payload!(S1024, 1024, 16);
payload!(S1056, 1056, 16);
payload!(A64, 64, 64);

pub(super) fn layouts() -> [(&'static str, usize, usize); 6] {
    [
        ("S8", size_of::<S8>(), align_of::<S8>()),
        ("S24", size_of::<S24>(), align_of::<S24>()),
        ("S48", size_of::<S48>(), align_of::<S48>()),
        ("S1024", size_of::<S1024>(), align_of::<S1024>()),
        ("S1056", size_of::<S1056>(), align_of::<S1056>()),
        ("A64", size_of::<A64>(), align_of::<A64>()),
    ]
}

fn register<T: NativePayload>(
    module: &mut ModuleBuilder,
    name: &str,
    factory: impl Fn() -> T + Send + Sync + 'static,
) {
    let mut declaration = module.define_type(name).unwrap();
    declaration
        .native_storage(NativeStorage::new(move |_| Ok(factory())))
        .unwrap();
    let ty = declaration.finish().unwrap();
    let new = module
        .define_function(FunctionDecl::new(format!("new_{name}")).returns(ty.apply([]).unwrap()))
        .unwrap();
    module
        .bind(
            new,
            NativeBinding::new(vec![], ty.codec(), |cx| cx.allocate_result()),
        )
        .unwrap();
}

pub(super) fn engine() -> KagariEngine {
    let mut builder = KagariEngine::builder().unwrap();
    let mut config = EngineConfig::default();
    config.default_runtime.gc.collection_threshold = None;
    builder.config(config);
    let mut module = ModuleBuilder::new("probe::layouts", builder.declarations()).unwrap();
    register(&mut module, "S8", || S8([42; 8]));
    register(&mut module, "S24", || S24([42; 24]));
    register(&mut module, "S48", || S48([42; 48]));
    register(&mut module, "S1024", || S1024([42; 1024]));
    register(&mut module, "S1056", || S1056([42; 1056]));
    register(&mut module, "A64", || A64([42; 64]));
    builder.install(module.finish().unwrap()).unwrap();
    builder.build().unwrap()
}
