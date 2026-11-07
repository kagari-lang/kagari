//! Bounded cache lifetime and input inventories exposed to the session owner.

use crate::analysis::AnalysisDatabase;
use kagari_source::source::SourceFile;
use std::sync::Arc;

impl AnalysisDatabase {
    /// Number of interned definition records, including retired declarations.
    /// The context owner can replace a long-lived database at its measured bound;
    /// explicit old snapshots retain their original table and remain valid.
    pub fn definition_count(&self) -> usize {
        self.definitions.snapshot().len()
    }

    /// Interned-node allowance derived from the currently published declaration
    /// inventory, including module path prefixes and implementation owners.
    /// The factor and fixed margin cover scoped/generated identity parents while
    /// allowing large live projects; retired names cannot accumulate indefinitely.
    pub fn definition_retention_limit(&self) -> usize {
        let live = self.declaration_cache.as_ref().map_or(0, |snapshot| {
            snapshot
                .files()
                .map(|file| {
                    file.declarations().iter().count()
                        + file.source().module_identity().path.len()
                        + 1
                        + file.declared.lowered.module.impls.len()
                })
                .sum::<usize>()
        });
        live.saturating_mul(4).saturating_add(4096).max(65_536)
    }

    /// Source owners retained by every published cache, including selected history.
    /// Cloned source records share text and line-index allocations; callers should
    /// deduplicate by the source owner's allocation identity when charging budgets.
    pub fn retained_sources(&self) -> Vec<Arc<SourceFile>> {
        let mut sources = Vec::new();
        let mut retain = |source: &SourceFile| {
            sources.push(Arc::new(source.clone()));
            sources.extend(source.copied_sources().cloned().map(Arc::new));
        };
        if let Some(snapshot) = &self.declaration_cache {
            for file in snapshot.files() {
                retain(file.source());
            }
        }
        if let Some(snapshot) = &self.signature_cache {
            for file in snapshot.declaration_snapshot().files() {
                retain(file.source());
            }
        }
        for file in self.files.values() {
            retain(file.source());
        }
        for result in self.body_cache.values() {
            retain(result.source());
            for file in result.signature_snapshot().declaration_snapshot().files() {
                retain(file.source());
            }
        }
        if let Some(files) = self.native_files.get() {
            for (_, lowered) in files {
                retain(&lowered.source);
            }
        }
        sources
    }
}
