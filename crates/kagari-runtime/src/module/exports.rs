//! Public-path lookup uses retained verification evidence and pins the supplying Program.

use crate::module::{LoadedModule, VerifiedProgram};
use kagari_bytecode::program::exports::ExportTarget;
use kagari_common::identity::table::DefinitionId;
use kagari_contract::{exports::ProgramExports, ids::FunctionRef};
use kagari_types::callable::Signature;

impl VerifiedProgram {
    pub(crate) fn export_graph(&self) -> &ProgramExports {
        &self.exports
    }

    pub(crate) fn export_signature(&self, id: DefinitionId) -> Option<Signature> {
        let target = self.export_targets.get(&id)?;
        let function = &self.modules[target.module.index()].functions[target.function.index()];
        Some(Signature {
            params: function
                .metadata
                .semantic
                .params
                .values()
                .cloned()
                .collect(),
            result: function.metadata.semantic.result.clone()?,
        })
    }

    /// Resolve a public path relative to the Program root into its verified slots.
    /// Private, missing and overlong paths return `None`; aliases may target dependencies.
    pub fn exported_function(&self, path: &str) -> Option<ExportTarget> {
        let root = &self.modules[self.root.index()].identity;
        let id = self.exports.function(root, path)?;
        self.export_targets.get(&id).copied()
    }

    /// Return the separate zero-argument root `main` capability, if present.
    /// A private entry is never added to public-path lookup.
    pub fn entry_function(&self) -> Option<ExportTarget> {
        self.export_targets.get(&self.exports.entry?).copied()
    }
}

impl LoadedModule {
    /// Resolve a public Program-root path into a handle for the exact target generation.
    /// This remains rooted at the Program even when called on a dependency member.
    /// Handles kept across collection use the ordinary runtime retention APIs.
    /// Execution subsequently applies the ordinary runtime ownership and call checks.
    pub fn exported_function(&self, path: &str) -> Option<(Self, FunctionRef)> {
        let target = self.program.code.exported_function(path)?;
        Some((self.member(target.module)?, target.function))
    }

    /// Select the root's separately verified script entry for an explicit CLI-style call.
    /// Handles kept across collection use the ordinary runtime retention APIs.
    /// Execution subsequently applies the ordinary runtime ownership and call checks.
    pub fn entry_function(&self) -> Option<(Self, FunctionRef)> {
        let target = self.program.code.entry_function()?;
        Some((self.member(target.module)?, target.function))
    }
}
