//! Immutable build-input snapshots and selected-root invalidation, without publication.

use crate::{CompileResult, engine::KagariEngine, error::EmbeddingError};
use kagari_common::cancellation::CancellationToken;
use kagari_hir::analysis::inputs::AnalysisInputs;
use kagari_package::{identity::PackageFamily, project::ProjectInputs, snapshot::ProgramInputs};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

/// Captured sources and the compilation environment expected for one Program.
/// This is input identity, not successful source checking or executable evidence.
#[derive(Debug, Clone)]
pub struct PackageBuildInput {
    source: ProgramInputs,
    fingerprint: String,
    environment: Arc<AnalysisInputs>,
}

impl PackageBuildInput {
    pub(super) fn capture(
        engine: &KagariEngine,
        source: &ProgramInputs,
        cancel: &CancellationToken,
    ) -> CompileResult<Self> {
        Ok(Self {
            fingerprint: engine.package_fingerprint(source, cancel)?,
            source: source.clone(),
            environment: engine.analysis.borrow().inputs(),
        })
    }

    /// Declaration-only environment captured alongside this package input identity.
    pub fn environment(&self) -> &Arc<AnalysisInputs> {
        &self.environment
    }

    pub fn sources(&self) -> &ProgramInputs {
        &self.source
    }

    /// Complete source/provider/query identity; output options and bytes remain separate.
    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }
}

/// Compiler inputs for the selected roots of one captured project.
///
/// Each root's fingerprint contains its full transitive source closure and importer
/// aliases. Comparing snapshots propagates dependency changes to all selected
/// dependent roots, including changes hidden by unchanged signatures (generic bodies,
/// constants and ordinary implementations). This uses the immutable closure keys;
/// there is no second mutable reverse-dependency cache to keep synchronized.
///
/// A host associates these root families with its own installations. Multiple
/// installations of a family still have independent generations. This type never
/// compiles, loads, unloads or publishes them, and grants no multi-Program atomicity.
#[derive(Debug, Clone)]
pub struct PackageBuildSet {
    programs: BTreeMap<PackageFamily, PackageBuildInput>,
}

/// Selected root families requiring work relative to an earlier input snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageChanges {
    /// New or changed roots in the current snapshot, with exact inputs in `programs()`.
    pub rebuild: BTreeSet<PackageFamily>,
    /// Previously selected families absent now. No installation is automatically unloaded.
    pub removed: BTreeSet<PackageFamily>,
}

impl PackageBuildSet {
    /// Captures the engine's provider/settings identity for every selected Program.
    ///
    /// Returns cancellation or malformed provider inputs. Does not check source
    /// bodies. Keep the engine environment unchanged while compiling candidates;
    /// each `CheckedModule::package_fingerprint` records what was actually checked.
    /// Only advance a host's installed baseline after its explicit reload/publication
    /// succeeds; failures leave existing installed generations usable.
    pub fn capture(
        engine: &KagariEngine,
        project: &ProjectInputs,
        cancel: &CancellationToken,
    ) -> CompileResult<Self> {
        let mut programs = BTreeMap::new();
        for source in project.programs().values() {
            let input = PackageBuildInput::capture(engine, source, cancel)?;
            if programs
                .insert(source.root().family.clone(), input)
                .is_some()
            {
                return Err(EmbeddingError::Source {
                    message: "duplicate selected root family".into(),
                });
            }
        }
        if cancel.check().is_err() {
            return Err(EmbeddingError::Cancelled);
        }
        Ok(Self { programs })
    }

    pub fn programs(&self) -> &BTreeMap<PackageFamily, PackageBuildInput> {
        &self.programs
    }

    /// Compares exact closures/provider settings; an unchanged sibling never adds imports.
    /// Selection changes are distinguished from rebuilding a still-selected family.
    pub fn changes_since(&self, previous: &Self) -> PackageChanges {
        PackageChanges {
            rebuild: self
                .programs
                .iter()
                .filter(|(family, input)| {
                    previous
                        .programs
                        .get(*family)
                        .is_none_or(|old| old.fingerprint != input.fingerprint)
                })
                .map(|(family, _)| family.clone())
                .collect(),
            removed: previous
                .programs
                .keys()
                .filter(|family| !self.programs.contains_key(*family))
                .cloned()
                .collect(),
        }
    }
}
