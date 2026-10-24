//! Immutable declaration-only inputs for an independent analysis session.

use crate::{
    analysis::AnalysisDatabase, native::render::DeclarationSource,
    typeck::const_budget::ConstLimits,
};
use kagari_common::identity::metadata::DefinitionMetadata;
use kagari_syntax::parser::ParseLimits;
use kagari_types::declaration::module::ModuleDecl;
use std::sync::Arc;

/// Captured providers, their identity scopes, presentation origins and query limits.
///
/// This contains no query caches, mutable identity context or executable callbacks.
/// Restoring creates a fresh context; ordinary declaration import maps each retained
/// provider scope into it. Native and offline Host declarations use the same path.
#[derive(Debug, Clone)]
pub struct AnalysisInputs {
    source_root: String,
    native_modules: Vec<Arc<DefinitionMetadata<ModuleDecl>>>,
    native_sources: Option<Vec<DeclarationSource>>,
    const_limits: ConstLimits,
    parse_limits: ParseLimits,
    max_semantic_diagnostics: usize,
}

impl AnalysisDatabase {
    /// Captures only immutable inputs, independently of this session's cached facts.
    pub fn inputs(&self) -> Arc<AnalysisInputs> {
        Arc::new(AnalysisInputs {
            source_root: self.source_root.clone(),
            native_modules: self.native_modules.clone(),
            native_sources: self.native_sources.clone(),
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
        database.native_modules = self.native_modules.clone();
        database.native_sources = self.native_sources.clone();
        database.const_limits = self.const_limits;
        database.parse_limits = self.parse_limits;
        database.max_semantic_diagnostics = self.max_semantic_diagnostics;
        Ok(database)
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
    use crate::{native::render::declaration_source, tests::native::function_module};
    use kagari_common::{
        cancellation::CancellationToken,
        identity::{ModuleIdentity, PackageId},
    };
    use kagari_source::{source::SourceFile, source_database::SourceSnapshot};
    use kagari_stdlib::catalog as foundation_catalog;
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
        let mut providers = foundation_catalog::shared().unwrap();
        providers.push(provider.clone());
        for rendered in [false, true] {
            let mut database = AnalysisDatabase::new("/capsule").unwrap();
            if rendered {
                let sources = providers
                    .iter()
                    .map(|provider| declaration_source(provider, &providers).unwrap())
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
