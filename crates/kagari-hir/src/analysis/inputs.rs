//! Immutable declaration-only inputs for an independent analysis session.

use crate::imports::environment::ImportEnvironment;
use crate::{analysis::AnalysisDatabase, typeck::const_budget::ConstLimits};
use kagari_common::identity::metadata::DefinitionMetadata;
use kagari_source::source::SourceFile;
use kagari_syntax::parser::ParseLimits;
use kagari_types::declaration::module::ModuleDecl;
use std::sync::Arc;

/// Immutable core documents, scoped registrations, import selections and query limits.
/// Restoring starts with a fresh identity context. User and editor documents are
/// supplied separately in the request's `SourceSnapshot`, not replayed from here.
#[derive(Debug, Clone)]
pub struct AnalysisInputs {
    source_root: String,
    import_environment: ImportEnvironment,
    native_modules: Vec<Arc<DefinitionMetadata<ModuleDecl>>>,
    core: Arc<[Arc<SourceFile>]>,
    const_limits: ConstLimits,
    parse_limits: ParseLimits,
    max_semantic_diagnostics: usize,
}

impl AnalysisDatabase {
    /// Exercise an altered bundled resource without exposing an alternate core policy.
    #[cfg(test)]
    pub(crate) fn replace_core_fixture(&mut self, fixture: Arc<SourceFile>) {
        let bundled = Arc::make_mut(&mut self.core);
        let owner = bundled
            .iter_mut()
            .find(|m| m.module_identity() == fixture.module_identity())
            .expect("fixture must replace an existing core owner");
        *owner = fixture;
        self.environment_fingerprint.take();
    }

    /// Captures only immutable inputs, independently of any returned results.
    pub fn inputs(&self) -> Arc<AnalysisInputs> {
        Arc::new(AnalysisInputs {
            source_root: self.source_root.clone(),
            import_environment: self.import_environment.clone(),
            native_modules: self.native_modules.clone(),
            core: self.core.clone(),
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
        database.core = self.core.clone();
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

    /// Bundled authored documents, separate from the physical request snapshot.
    pub fn core_modules(&self) -> &[Arc<SourceFile>] {
        &self.core
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
    use crate::tests::native_fixture::function_module;
    use crate::tests::standard::standard_modules;
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
        let mut database = AnalysisDatabase::new("/capsule").unwrap();
        database.set_native_modules(providers.clone());
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
        let mut fresh = inputs.database().unwrap();
        assert_eq!(fresh.environment_fingerprint(&cancel).unwrap(), fingerprint);
        let rebuilt = fresh.snapshot(sources, &cancel).unwrap();
        rebuilt.check_program(source.id(), &cancel).unwrap();
        assert_eq!(rebuilt.physical_sources().files().count(), 1);
    }
}
