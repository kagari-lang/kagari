//! Standard provider preparation shared by independent contract cases.
use crate::{
    engine::{
        KagariEngine,
        source::{CheckedModule, analysis::SourceAnalysis},
    },
    test_support::execution,
};
use kagari_common::cancellation::CancellationToken;
use kagari_hir::analysis::AnalysisDatabase;
use kagari_runtime::{
    host::{HostCallContext, HostError},
    native::{builder::ModuleBuilder, installation::NativeInstallation},
    value::Value,
};
use kagari_source::{identity::FileId, source_database::SourceSnapshot};
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

pub(super) fn check_publication(sources: SourceSnapshot, root: FileId) -> CheckedModule {
    // This scalar publication fixture needs no native declarations; check the
    // explicit source closure independently of the standard provider baseline.
    let mut analysis = AnalysisDatabase::new("/language-contract").unwrap();
    let cancel = CancellationToken::default();
    let snapshot = analysis.snapshot(sources, &cancel).unwrap();
    SourceAnalysis::new(snapshot, root, None)
        .unwrap()
        .check(&cancel)
        .unwrap()
}

pub(super) fn installation() -> &'static NativeInstallation {
    static INSTALLATION: OnceLock<NativeInstallation> = OnceLock::new();
    INSTALLATION
        .get_or_init(|| NativeInstallation::prepare(&kagari_stdlib::modules().unwrap()).unwrap())
}
