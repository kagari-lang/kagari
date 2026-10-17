//! Standard provider preparation shared by independent contract cases.
use kagari_hir::analysis::AnalysisDatabase;
use kagari_runtime::native::installation::NativeInstallation;
use kagari_source::source_database::SourceDatabase;
use std::sync::{Arc, Mutex, OnceLock};

pub(super) fn analysis() -> AnalysisDatabase {
    static BASELINE: OnceLock<Mutex<AnalysisDatabase>> = OnceLock::new();
    BASELINE
        .get_or_init(|| {
            let mut analysis = AnalysisDatabase::default();
            analysis.set_native_modules(
                kagari_stdlib::modules()
                    .unwrap()
                    .iter()
                    .map(|module| Arc::new(module.to_declaration()))
                    .collect(),
            );
            let snapshot = analysis
                .snapshot(SourceDatabase::default().snapshot(), &Default::default())
                .unwrap();
            for declaration in snapshot.declaration_snapshot().files() {
                assert!(
                    snapshot
                        .file(declaration.source().id())
                        .unwrap()
                        .result()
                        .diagnostics()
                        .is_empty()
                );
            }
            Mutex::new(analysis)
        })
        .lock()
        .unwrap()
        .clone()
}

pub(super) fn installation() -> &'static NativeInstallation {
    static INSTALLATION: OnceLock<NativeInstallation> = OnceLock::new();
    INSTALLATION
        .get_or_init(|| NativeInstallation::prepare(&kagari_stdlib::modules().unwrap()).unwrap())
}
