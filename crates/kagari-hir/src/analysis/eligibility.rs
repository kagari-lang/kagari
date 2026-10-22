//! HIR-owned evidence for optional semantic consumers of recoverable snapshots.

use crate::{
    analysis::FileAnalysis,
    hir::ids::{FunctionId, StaticId},
};

/// Whether a body's bindings and types completed under a valid shared environment.
/// This evidence is independent of the retained diagnostic count. It does not
/// authorize compilation of an erroneous dependency closure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyEligibility {
    Unavailable,
    Complete,
}

impl FileAnalysis {
    /// Whether shared parser/declaration/signature inputs are valid for naming queries.
    pub fn declarations_eligible(&self) -> bool {
        self.shared_valid
    }

    /// Queries a runtime initializer from this file's lowering, independently of diagnostic truncation.
    pub fn static_initializer_eligibility(&self, id: StaticId) -> BodyEligibility {
        if self.shared_valid
            && self
                .result()
                .facts()
                .typed
                .valid_static_initializers
                .contains(&id)
        {
            BodyEligibility::Complete
        } else {
            BodyEligibility::Unavailable
        }
    }

    /// Queries a function ID from this file's lowering; absent/bodyless IDs fail.
    /// Shared declaration/signature or parser failures conservatively invalidate
    /// all bodies in the snapshot. Local body errors invalidate only their owner.
    pub fn body_eligibility(&self, function: FunctionId) -> BodyEligibility {
        if self.shared_valid && self.result().facts().typed.valid_bodies.contains(&function) {
            BodyEligibility::Complete
        } else {
            BodyEligibility::Unavailable
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::test_analysis;
    use kagari_source::source_database::{SourceDatabase, SourceLayer};

    #[test]
    fn positive_body_evidence_survives_truncation_but_not_shared_signature_errors() {
        for limit in [0, 1, 100] {
            for (broken, healthy_eligible) in [
                ("fn broken()->i32{true}", true),
                ("fn broken(x:Missing){}", false),
            ] {
                let mut source = SourceDatabase::default();
                let file = source
                    .set(
                        "eligibility.kgr",
                        format!("{broken}\nfn healthy()->i32{{val unused=10;42}}"),
                        SourceLayer::Base,
                    )
                    .unwrap();
                let mut db = test_analysis();
                db.set_max_semantic_diagnostics(limit);
                let snapshot = db.snapshot(source.snapshot(), &Default::default()).unwrap();
                let analysis = snapshot.file(file).unwrap();
                let healthy = analysis
                    .result()
                    .facts()
                    .lowered
                    .module
                    .functions
                    .iter()
                    .find(|function| function.name == "healthy")
                    .unwrap();
                assert_eq!(
                    analysis.body_eligibility(healthy.id) == BodyEligibility::Complete,
                    healthy_eligible
                );
                assert!(snapshot.check_program(file, &Default::default()).is_err());
                assert!(
                    analysis
                        .result()
                        .diagnostics()
                        .iter()
                        .any(|diagnostic| diagnostic.severity
                            == kagari_source::diagnostic::Severity::Error)
                );
            }
        }
    }
}
