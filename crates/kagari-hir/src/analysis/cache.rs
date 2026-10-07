//! Validate complete file and body inputs before retaining or remapping facts.
use crate::{
    AnalyzedModule, PreparedAnalysis,
    analysis::{FileAnalysis, signature_queries::BodyEnvironment},
};
use kagari_common::cancellation::CancellationToken;

impl FileAnalysis {
    /// Requires identical source revision/arena and semantic dependencies before sharing the whole file result.
    pub(super) fn can_retain(
        &self,
        prepared: &PreparedAnalysis,
        previous: &AnalyzedModule,
        environment: &BodyEnvironment,
        cancel: &CancellationToken,
    ) -> bool {
        let old = self.result.records().facts();
        let names = &prepared.names.facts;
        self.source.revision() == prepared.lowered.source.revision()
            && old.lowered.module.body.arena() == prepared.lowered.module.body.arena()
            && old.names.hosts.revision() == names.hosts.revision()
            && old.names.imports == names.imports
            && old
                .names
                .catalog
                .same_namespaces(&names.catalog, &names.items, &names.hosts, cancel)
                .unwrap_or(false)
            && previous.imported_functions == environment.imported_functions
            && previous.aggregates == environment.aggregates
            && previous.declarations.imported_types == prepared.declarations.imported_types
    }

    /// Checks semantic compatibility before attempting per-body remapping into a new arena.
    ///
    /// Allows changed source revisions, but requires matching module identity, host revision,
    /// import bindings, reachable namespaces, imported types/callables and aggregate contracts.
    /// The body reuse layer must still compare and remap individual body contents.
    pub(super) fn can_reuse_body(
        &self,
        prepared: &PreparedAnalysis,
        previous: &AnalyzedModule,
        environment: &BodyEnvironment,
        cancel: &CancellationToken,
    ) -> bool {
        let old = self.result.records().facts();
        let names = &prepared.names.facts;
        old.names.hosts.revision() == names.hosts.revision()
            && old.names.imports.same_bindings(&names.imports)
            && old
                .names
                .catalog
                .same_reuse_namespaces(
                    &names.catalog,
                    &old.names.items,
                    &names.items,
                    &names.hosts,
                    cancel,
                )
                .unwrap_or(false)
            && previous.imported_functions == environment.imported_functions
            && previous.aggregates.same_contracts(&environment.aggregates)
            && previous.declarations.imported_types == prepared.declarations.imported_types
            && self.source.module_identity() == prepared.lowered.source.module_identity()
    }
}
