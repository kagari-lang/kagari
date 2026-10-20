//! Standard provider preparation shared by independent contract cases.
use crate::{engine::KagariEngine, test_support::execution};
use kagari_runtime::{
    host::{HostCallContext, HostError},
    native::{builder::ModuleBuilder, installation::NativeInstallation},
    value::Value,
};
use std::sync::{Mutex, OnceLock};

pub(super) fn engine(vector: bool) -> KagariEngine {
    if !vector {
        return execution::engine(Default::default());
    }
    static VECTOR: OnceLock<Mutex<KagariEngine>> = OnceLock::new();
    VECTOR
        .get_or_init(|| {
            let mut builder = KagariEngine::builder().unwrap();
            let mut module = ModuleBuilder::from_declaration(
                super::observe_vector(),
                builder.declarations(),
                Default::default(),
            )
            .unwrap();
            let vector = module.declared_function("vector").unwrap();
            module
                .bind(vector, |_: &HostCallContext<'_>, _: &[Value]| {
                    Err(HostError::new("compilation-only provider must not execute"))
                })
                .unwrap();
            builder.install(module.finish().unwrap()).unwrap();
            Mutex::new(builder.build().unwrap())
        })
        .lock()
        .unwrap()
        .new_session(Default::default())
}

pub(super) fn installation() -> &'static NativeInstallation {
    static INSTALLATION: OnceLock<NativeInstallation> = OnceLock::new();
    INSTALLATION
        .get_or_init(|| NativeInstallation::prepare(&kagari_stdlib::modules().unwrap()).unwrap())
}
