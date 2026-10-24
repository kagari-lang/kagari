use std::{fs, path::Path, sync::Arc};

use crate::{
    AnalysisResult, AnalyzedModule,
    analysis::{AnalysisDatabase, AnalysisSnapshot, FileAnalysis},
    tests::ui_diagnostics::{project_records, read_expectations},
};
use kagari_source::{
    diagnostic::{Diagnostic, Severity},
    identity::FileId,
    presentation::DiagnosticRecord,
    source::SourceFile,
    source_database::SourceSnapshot,
};
use kagari_stdlib::catalog;

#[derive(Clone, Copy)]
pub(super) enum Environment {
    Bare,
    Standard,
}

pub(super) struct Fixture {
    captured: SourceSnapshot,
    snapshot: AnalysisSnapshot,
    root: FileId,
}

impl Fixture {
    pub(super) fn new(text: &str, logical_name: &str, environment: Environment) -> Self {
        let source = Arc::new(SourceFile::new(logical_name, text));
        let root = source.id();
        let captured = SourceSnapshot::single_file(source);
        let mut database = AnalysisDatabase::default();
        if matches!(environment, Environment::Standard) {
            database
                .set_native_modules(catalog::shared().expect("installed standard declarations"));
        }
        let snapshot = database
            .snapshot(captured.clone(), &Default::default())
            .expect("UI analysis preparation must succeed, not become a source diagnostic");
        let fixture = Self {
            captured,
            snapshot,
            root,
        };
        assert_eq!(fixture.file().source().id(), root);
        fixture
    }

    pub(super) fn file(&self) -> &FileAnalysis {
        self.snapshot.file(self.root).expect("captured UI file")
    }

    pub(super) fn result(&self) -> &AnalysisResult<AnalyzedModule> {
        self.file().result()
    }

    pub(super) fn records(&self) -> Result<Vec<DiagnosticRecord>, String> {
        bind_records(
            self.result().diagnostics(),
            self.file().source(),
            &self.captured,
        )
    }

    pub(super) fn compare(&self, path: &Path) {
        let expected_path = path.with_extension("diagnostics.json");
        let text = fs::read_to_string(&expected_path)
            .unwrap_or_else(|error| panic!("{}: {error}", expected_path.display()));
        let sources = [self.file().source()];
        let expected = read_expectations(&text, &sources)
            .unwrap_or_else(|error| panic!("{}: {error}", expected_path.display()));
        let records = self.records().expect("source-bound final HIR diagnostics");
        let received = project_records(&records, &sources).expect("captured UI source revision");
        assert_eq!(received, expected, "{}", path.display());
        assert!(
            self.result()
                .diagnostics()
                .iter()
                .any(|diagnostic| diagnostic.severity == Severity::Error),
            "rejection fixture produced no error: {}",
            path.display()
        );
    }
}

pub(super) fn bind_records(
    diagnostics: &[Diagnostic],
    source: &SourceFile,
    captured: &SourceSnapshot,
) -> Result<Vec<DiagnosticRecord>, String> {
    let owner = captured
        .file(source.id())
        .ok_or("uncaptured HIR source identity")?;
    if owner.revision() != source.revision() {
        return Err("stale HIR source revision".into());
    }
    diagnostics
        .iter()
        .map(|diagnostic| {
            // The production adapter intentionally drops invalid raw ranges. Bind and
            // validate against the retained owner first, never an anonymous fallback.
            let bound = diagnostic
                .span
                .map(|range| {
                    let span = source
                        .span(range)
                        .ok_or_else(|| format!("invalid raw HIR span: {range:?}"))?;
                    if !captured.contains(span) || source.local_range(span) != Some(range) {
                        return Err(format!("uncaptured raw HIR span: {span:?}"));
                    }
                    Ok(span)
                })
                .transpose()?;
            let record = DiagnosticRecord::from_diagnostic(diagnostic.clone(), source);
            if record.span != bound {
                return Err("HIR diagnostic lost its source-bound location".into());
            }
            Ok(record)
        })
        .collect()
}
