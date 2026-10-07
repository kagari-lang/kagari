//! A checked, snapshot-owned source dependency closure for compilation and linking.

use crate::typeck::TypedFunction;
use std::collections::HashMap;

use {
    kagari_common::identity::{
        ModuleIdentity, mapping::DefinitionMappingError, table::DefinitionId,
    },
    kagari_source::{
        diagnostic::{Diagnostic, Severity},
        identity::{FileId, Revision},
    },
};

use {
    crate::{
        CheckedAnalysis,
        analysis::AnalysisSnapshot,
        imports::{ModuleOrderError, SourceDeclRef},
    },
    kagari_common::cancellation::CancellationToken,
};

/// An error diagnostic paired with the source file revision that produced it.
#[derive(Debug, Clone)]
pub struct ProgramDiagnostic {
    /// Physical or synthetic file owning the diagnostic.
    pub file: FileId,
    /// Analyzed revision of that file.
    pub revision: Revision,
    /// Source diagnostic retained from analysis.
    pub diagnostic: Diagnostic,
}

/// Failure to form an error-free checked source dependency closure.
#[derive(Debug)]
pub enum ProgramCheckError {
    /// The root or a reachable graph node has no file analysis in the snapshot.
    MissingFile(FileId),
    /// The import graph cannot provide a valid reachable module closure.
    Graph(ModuleOrderError),
    /// Reachable file analyses contain error-severity diagnostics.
    Diagnostics(Vec<ProgramDiagnostic>),
    /// Cooperative cancellation interrupted closure validation.
    Cancelled,
    /// Checked facts could not be adopted into their definition identity tables.
    Identity(DefinitionMappingError),
}

/// An error-free, immutable source dependency closure for compilation and linking.
///
/// ```text
/// CheckedProgram
///   root: FileId --by_file index--> modules: Vec<CheckedAnalysis>
///   dependencies: ModuleIdentity -> direct imported module identities
/// SourceDeclRef --file + complete SourceUnit check--> module -> TypedFunction
/// ```
///
/// Built by [`AnalysisSnapshot::check_program`]. Module ordering is deterministic,
/// not a topological execution order; cycles and diamonds contain each member once.
/// Members cannot be replaced with facts from another snapshot after checking.
/// The next compiler stage consumes checked HIR; this value is not executable code.
#[derive(Debug, Clone)]
pub struct CheckedProgram {
    root: FileId,
    modules: Vec<CheckedAnalysis>,
    by_file: HashMap<FileId, usize>,
    dependencies: HashMap<ModuleIdentity, Vec<ModuleIdentity>>,
}

impl CheckedProgram {
    /// Borrows the checked root requested when this closure was built.
    pub fn root(&self) -> &CheckedAnalysis {
        &self.modules[self.by_file[&self.root]]
    }

    /// Deterministic order. Diamonds and cycles contain each module once.
    pub fn modules(&self) -> &[CheckedAnalysis] {
        &self.modules
    }

    /// Returns direct dependencies of a member module; `None` means the module is outside this closure.
    pub fn dependencies(&self, module: &ModuleIdentity) -> Option<&[ModuleIdentity]> {
        self.dependencies.get(module).map(Vec::as_slice)
    }

    /// Looks up a canonical source function inside this checked closure.
    ///
    /// Checks the complete source unit (file, revision, arena and module identity) before
    /// using the function index. Returns `None` for a stale/foreign unit, a non-function
    /// reference or a function absent from the checked module.
    pub fn source_function(
        &self,
        id: &SourceDeclRef,
    ) -> Option<(&CheckedAnalysis, &TypedFunction<DefinitionId>)> {
        let module = &self.modules[*self.by_file.get(&id.unit.file)?];
        if !id.unit.matches(&module.lowered) {
            return None;
        }
        let function = module
            .typed
            .functions
            .iter()
            .find(|function| Some(function.id) == id.function())?;
        Some((module, function))
    }
}

impl AnalysisSnapshot {
    /// Validates and captures the root's reachable source dependency closure.
    ///
    /// Reads already prepared file analysis; it neither executes scripts nor verifies MIR.
    /// Every returned member is error-free and retains this snapshot's checked facts.
    ///
    /// # Errors
    ///
    /// Returns [`ProgramCheckError`] for a missing file, invalid graph, error diagnostics,
    /// cancellation or invalid definition metadata. Warnings alone do not reject a program.
    ///
    /// # Example: follow an imported alias to its source function
    ///
    /// ```
    /// use kagari_common::identity::{ModuleIdentity, PackageId};
    /// use kagari_hir::{analysis::AnalysisDatabase, imports::ResolvedTarget};
    /// use kagari_source::source_database::{SourceDatabase, SourceLayer};
    ///
    /// let mut sources = SourceDatabase::default();
    /// sources.bind_module("math.kgr", ModuleIdentity {
    ///     package: PackageId("pkg".into()), path: vec!["math".into()],
    /// }).unwrap();
    /// let math = sources.set("math.kgr",
    ///     "pub fn sum(x: i32) -> i32 { x + 1 }".into(), SourceLayer::Base).unwrap();
    /// sources.bind_module("api.kgr", ModuleIdentity {
    ///     package: PackageId("pkg".into()), path: vec!["api".into()],
    /// }).unwrap();
    /// sources.set("api.kgr", "pub use pkg::math::*;".into(), SourceLayer::Base).unwrap();
    /// sources.bind_module("app.kgr", ModuleIdentity {
    ///     package: PackageId("pkg".into()), path: vec!["app".into()],
    /// }).unwrap();
    /// let app = sources.set("app.kgr",
    ///     "use pkg::api::sum as add; fn main() -> i32 { add(41) }".into(),
    ///     SourceLayer::Base).unwrap();
    /// let mut database = AnalysisDatabase::default();
    /// database.set_native_modules(kagari_stdlib::catalog::shared());
    /// let snapshot = database.snapshot(sources.snapshot(), &Default::default()).unwrap();
    /// let analysis = snapshot.file(app).unwrap();
    /// let target = analysis.result().facts().names.imports.directives[0]
    ///     .resolution.target().unwrap();
    /// let ResolvedTarget::Source(target) = target else { panic!("expected a source function") };
    /// assert_eq!(target.unit.file, math);
    /// let program = snapshot.check_program(app, &Default::default()).unwrap();
    /// let (module, function) = program.source_function(target).unwrap();
    /// assert_eq!(module.lowered.source.id(), math);
    /// assert_eq!(module.lowered.module.functions[function.id.index()].name, "sum");
    /// ```
    pub fn check_program(
        &self,
        root: FileId,
        cancel: &CancellationToken,
    ) -> Result<CheckedProgram, ProgramCheckError> {
        cancel.check().map_err(|_| ProgramCheckError::Cancelled)?;
        let root_source = self
            .file(root)
            .ok_or(ProgramCheckError::MissingFile(root))?;
        let order = self
            .module_graph()
            .reachable_order(root_source.source().module_identity(), cancel)
            .map_err(|error| match error {
                ModuleOrderError::Cancelled => ProgramCheckError::Cancelled,
                error => ProgramCheckError::Graph(error),
            })?;
        let mut modules = Vec::with_capacity(order.len());
        let mut by_file = HashMap::new();
        let mut dependencies = HashMap::new();
        let mut diagnostics = Vec::new();
        for identity in order {
            cancel.check().map_err(|_| ProgramCheckError::Cancelled)?;
            let node = self
                .module_graph()
                .node(&identity)
                .expect("ordered graph node");
            let file = self
                .file(node.file)
                .ok_or(ProgramCheckError::MissingFile(node.file))?;
            for diagnostic in file.result().diagnostics() {
                cancel.check().map_err(|_| ProgramCheckError::Cancelled)?;
                if diagnostic.severity == Severity::Error {
                    diagnostics.push(ProgramDiagnostic {
                        file: node.file,
                        revision: file.source().revision(),
                        diagnostic: diagnostic.clone(),
                    });
                }
            }
            by_file.insert(node.file, modules.len());
            dependencies.insert(identity, node.dependencies().to_vec());
            // Source imports are valid here: every reachable module belongs to this
            // immutable snapshot and is checked before the program is returned.
            if !file
                .result()
                .diagnostics()
                .iter()
                .any(|diagnostic| diagnostic.severity == Severity::Error)
            {
                modules.push(
                    CheckedAnalysis::adopt_scoped(
                        file.result().facts(),
                        file.definitions(),
                        cancel,
                    )
                    .map_err(|error| match error {
                        DefinitionMappingError::Cancelled => ProgramCheckError::Cancelled,
                        error => ProgramCheckError::Identity(error),
                    })?,
                );
            }
        }
        cancel.check().map_err(|_| ProgramCheckError::Cancelled)?;
        if !diagnostics.is_empty() {
            return Err(ProgramCheckError::Diagnostics(diagnostics));
        }
        Ok(CheckedProgram {
            root,
            modules,
            by_file,
            dependencies,
        })
    }
}
