//! Immutable declaration-only inputs for an independent analysis session.

use crate::imports::environment::ImportEnvironment;
use crate::{
    analysis::{AnalysisDatabase, core_source, external::ExternalModule},
    native::render::DeclarationSource,
    typeck::const_budget::ConstLimits,
};
use kagari_common::identity::metadata::DefinitionMetadata;
use kagari_syntax::parser::ParseLimits;
use kagari_types::declaration::module::ModuleDecl;
use std::sync::{Arc, Weak};

/// One immutable preparation inventory, with bundled owners before caller inputs.
/// Captures share the allocation; replay does not go through the additive composition step.
#[derive(Debug, Clone)]
pub(super) struct LibrarySources {
    modules: Arc<[ExternalModule]>,
    core_len: usize,
}

impl Default for LibrarySources {
    fn default() -> Self {
        let modules = core_source::shared_modules();
        Self {
            core_len: modules.len(),
            modules,
        }
    }
}

/// Identity of one immutable library sidecar in its owning analysis database inventory.
/// Cache keys do not retain the inventory or its contracts after library replacement.
#[derive(Debug, Clone)]
pub(super) struct LibrarySourceKey {
    inventory: Weak<[ExternalModule]>,
    slot: usize,
}

impl PartialEq for LibrarySourceKey {
    fn eq(&self, other: &Self) -> bool {
        self.slot == other.slot && Weak::ptr_eq(&self.inventory, &other.inventory)
    }
}

impl Eq for LibrarySourceKey {}

impl LibrarySources {
    pub(super) fn all(&self) -> &[ExternalModule] {
        &self.modules
    }

    pub(super) fn parse_inputs(
        &self,
        caller_limits: ParseLimits,
    ) -> impl Iterator<Item = (LibrarySourceKey, &ExternalModule, ParseLimits)> {
        let inventory = Arc::downgrade(&self.modules);
        self.modules.iter().enumerate().map(move |(slot, input)| {
            let key = LibrarySourceKey {
                inventory: inventory.clone(),
                slot,
            };
            let limits = if slot < self.core_len {
                ParseLimits::default()
            } else {
                caller_limits
            };
            (key, input, limits)
        })
    }

    pub(super) fn set_external(&mut self, external: Vec<ExternalModule>) {
        self.modules = self.modules[..self.core_len]
            .iter()
            .cloned()
            .chain(external)
            .collect();
    }
}

/// Captured providers, their identity scopes, presentation origins and query limits.
///
/// This contains no query caches, mutable identity context or executable callbacks.
/// Restoring creates a fresh context; ordinary declaration import maps each retained
/// provider scope into it. Independent offline exports retain real source and
/// static sidecars, not registration authority; they use ordinary source checking.
#[derive(Debug, Clone)]
pub struct AnalysisInputs {
    source_root: String,
    import_environment: ImportEnvironment,
    native_modules: Vec<Arc<DefinitionMetadata<ModuleDecl>>>,
    native_sources: Option<Vec<DeclarationSource>>,
    libraries: LibrarySources,
    const_limits: ConstLimits,
    parse_limits: ParseLimits,
    max_semantic_diagnostics: usize,
}

impl AnalysisDatabase {
    /// Exercise an altered bundled resource without exposing an alternate core policy.
    #[cfg(test)]
    pub(crate) fn replace_core_fixture(&mut self, fixture: ExternalModule) {
        let bundled = &mut Arc::make_mut(&mut self.libraries.modules)[..self.libraries.core_len];
        let owner = bundled
            .iter_mut()
            .find(|m| m.source.module_identity() == fixture.source.module_identity())
            .expect("fixture must replace an existing core owner");
        *owner = fixture;
        self.invalidate_library_queries();
    }

    /// Captures only immutable inputs, independently of this session's cached facts.
    pub fn inputs(&self) -> Arc<AnalysisInputs> {
        Arc::new(AnalysisInputs {
            source_root: self.source_root.clone(),
            import_environment: self.import_environment.clone(),
            native_modules: self.native_modules.clone(),
            native_sources: self.native_sources.clone(),
            libraries: self.libraries.clone(),
            const_limits: self.const_limits,
            parse_limits: self.parse_limits,
            max_semantic_diagnostics: self.max_semantic_diagnostics,
        })
    }
}

impl AnalysisInputs {
    /// Creates an empty analysis session without IO or provider registration.
    pub fn database(&self) -> Result<AnalysisDatabase, String> {
        let mut database = AnalysisDatabase::new(&self.source_root)?;
        database.import_environment = self.import_environment.clone();
        database.native_modules = self.native_modules.clone();
        database.native_sources = self.native_sources.clone();
        database.libraries = self.libraries.clone();
        database.const_limits = self.const_limits;
        database.parse_limits = self.parse_limits;
        database.max_semantic_diagnostics = self.max_semantic_diagnostics;
        Ok(database)
    }

    /// Explicit import scopes captured with this immutable analysis environment.
    pub fn import_environment(&self) -> &ImportEnvironment {
        &self.import_environment
    }

    pub fn source_root(&self) -> &str {
        &self.source_root
    }

    pub fn native_modules(&self) -> &[Arc<DefinitionMetadata<ModuleDecl>>] {
        &self.native_modules
    }

    pub fn native_sources(&self) -> Option<&[DeclarationSource]> {
        self.native_sources.as_deref()
    }

    /// The bundled authored modules in this snapshot; their file IDs can root a checked seed.
    pub fn core_modules(&self) -> &[ExternalModule] {
        &self.libraries.modules[..self.libraries.core_len]
    }

    /// Caller-supplied offline declarations, excluding the immutable packaged core.
    pub fn external_modules(&self) -> &[ExternalModule] {
        &self.libraries.modules[self.libraries.core_len..]
    }

    /// All authored library inputs, e.g. for rendering Native views with matching aliases.
    pub fn library_modules(&self) -> &[ExternalModule] {
        self.libraries.all()
    }

    pub fn const_limits(&self) -> ConstLimits {
        self.const_limits
    }

    pub fn parse_limits(&self) -> ParseLimits {
        self.parse_limits
    }

    pub fn max_semantic_diagnostics(&self) -> usize {
        self.max_semantic_diagnostics
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::imports::environment::ModuleScope;
    use crate::tests::standard::standard_modules;
    use crate::{
        native::render::declaration_source_with_external, tests::native_fixture::function_module,
    };
    use kagari_common::{
        cancellation::CancellationToken,
        identity::{ModuleIdentity, PackageId},
    };
    use kagari_source::{
        package::PackageScope, source::SourceFile, source_database::SourceSnapshot,
    };
    use kagari_types::{scalar::BuiltinType, ty::Ty};

    #[test]
    fn snapshot_retains_scoped_declarations_and_settings_without_query_state() {
        fn send_sync<T: Send + Sync>() {}
        send_sync::<AnalysisInputs>();
        let cancel = CancellationToken::default();
        let provider = function_module(
            ModuleIdentity {
                package: PackageId("host".into()),
                path: vec![],
            },
            "number",
            vec![],
            Ty::Builtin(BuiltinType::I32),
        );
        let mut providers = standard_modules();
        providers.push(provider.clone());
        for rendered in [false, true] {
            let mut database = AnalysisDatabase::new("/capsule").unwrap();
            if rendered {
                let sources = providers
                    .iter()
                    .map(|provider| {
                        declaration_source_with_external(
                            provider,
                            &providers,
                            database.inputs().library_modules(),
                        )
                        .unwrap()
                    })
                    .collect();
                database
                    .set_native_sources(providers.clone(), sources)
                    .unwrap();
            } else {
                database.set_native_modules(providers.clone());
            }
            let const_limits = ConstLimits {
                max_steps: 1234,
                max_depth: 31,
            };
            let parse_limits = ParseLimits {
                max_nesting: 50,
                ..Default::default()
            };
            database.set_const_limits(const_limits);
            database.set_parse_limits(parse_limits);
            database.set_max_semantic_diagnostics(23);
            let source = Arc::new(SourceFile::new(
                "main.kgr",
                "fn main() -> i32 { host::number() }",
            ));
            let import_environment = ImportEnvironment {
                modules: [(
                    source.module_identity().clone(),
                    ModuleScope {
                        packages: PackageScope {
                            dependencies: [("host".into(), PackageId("host".into()))].into(),
                            selection: "explicit-test".into(),
                        },
                        dependencies: providers
                            .iter()
                            .map(|provider| provider.records().identity.clone())
                            .collect(),
                        preludes: Default::default(),
                    },
                )]
                .into(),
            };
            database.set_import_environment(import_environment.clone());

            let sources = SourceSnapshot::single_file(source.clone());
            let snapshot = database.snapshot(sources.clone(), &cancel).unwrap();
            snapshot.check_program(source.id(), &cancel).unwrap();
            let inputs = snapshot.inputs().clone();
            let fingerprint = database.environment_fingerprint(&cancel).unwrap();
            database.set_native_modules(vec![]);
            database.set_const_limits(Default::default());
            database.set_parse_limits(Default::default());
            database.set_max_semantic_diagnostics(1);
            drop(database);
            assert_eq!(inputs.import_environment(), &import_environment);
            assert_eq!(inputs.const_limits(), const_limits);
            assert_eq!(inputs.parse_limits(), parse_limits);
            assert_eq!(inputs.max_semantic_diagnostics(), 23);
            assert_eq!(inputs.source_root(), "/capsule");

            assert!(Arc::ptr_eq(
                inputs.native_modules().last().unwrap(),
                &provider
            ));
            assert_eq!(inputs.native_sources().is_some(), rendered);
            let mut fresh = inputs.database().unwrap();
            assert_eq!(fresh.environment_fingerprint(&cancel).unwrap(), fingerprint);
            let rebuilt = fresh.snapshot(sources, &cancel).unwrap();
            rebuilt.check_program(source.id(), &cancel).unwrap();
            assert_eq!(rebuilt.physical_sources().files().count(), 1);
        }
    }
}
