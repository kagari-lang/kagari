//! Portable provider/settings identity owned by the analysis input environment.

use crate::{
    analysis::{AnalysisDatabase, error::AnalysisError},
    typeck::const_budget::ConstLimits,
};
use blake3::Hasher;
use kagari_common::{cancellation::CancellationToken, identity::mapping::DefinitionMappingError};
use kagari_syntax::parser::ParseLimits;

impl AnalysisDatabase {
    /// Identifies native/Host declarations and query limits without checking source.
    ///
    /// Includes complete canonical provider records rather than process IDs or only
    /// rendered signatures. Native provider registration order is irrelevant. Source
    /// bytes/selections, lowering options and executable content belong to their
    /// separate owners. Tooling locations and runtime callbacks/state are excluded.
    /// This key is not semantic validation or executable loading evidence.
    ///
    /// Reuses the encoding while these immutable inputs/settings remain installed;
    /// every environment setter invalidates it, independently of source revisions.
    /// Returns cancellation or malformed provider identity/type encoding errors.
    pub fn environment_fingerprint(
        &self,
        cancel: &CancellationToken,
    ) -> Result<String, AnalysisError> {
        cancel.check()?;
        if let Some(fingerprint) = self.environment_fingerprint.get() {
            return Ok(fingerprint.clone());
        }
        let mut hash = Hasher::new_derive_key("kagari analysis environment v1");
        let mut providers = self.native_modules.iter().collect::<Vec<_>>();
        providers.sort_by(|a, b| a.records().identity.cmp(&b.records().identity));
        hash.update(&(providers.len() as u64).to_le_bytes());
        for provider in providers {
            let bytes = provider
                .records()
                .compiler_input_bytes(provider.definitions(), cancel)?;
            hash_bytes(&mut hash, &bytes, cancel)?;
        }
        let host = self
            .hosts
            .compiler_input_bytes()
            .map_err(DefinitionMappingError::from)?;
        hash_bytes(&mut hash, &host, cancel)?;
        let ConstLimits {
            max_steps,
            max_depth,
        } = self.const_limits;
        let ParseLimits {
            max_diagnostics,
            max_nesting,
            max_tree_depth,
        } = self.parse_limits;
        for limit in [
            max_steps,
            max_depth,
            max_diagnostics,
            max_nesting,
            max_tree_depth,
            self.max_semantic_diagnostics,
        ] {
            hash.update(&(limit as u64).to_le_bytes());
        }
        cancel.check()?;
        let fingerprint = hash.finalize().to_hex().to_string();
        let _ = self.environment_fingerprint.set(fingerprint.clone());
        Ok(fingerprint)
    }
}

fn hash_bytes(
    hash: &mut Hasher,
    bytes: &[u8],
    cancel: &CancellationToken,
) -> Result<(), AnalysisError> {
    hash.update(&(bytes.len() as u64).to_le_bytes());
    for chunk in bytes.chunks(64 * 1024) {
        cancel.check()?;
        hash.update(chunk);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::{
        analysis::AnalysisDatabase, tests::native::function_module,
        typeck::const_budget::ConstLimits,
    };
    use kagari_common::{
        cancellation::CancellationToken,
        identity::{
            ModuleIdentity, PackageId, map::DefinitionContext, metadata::DefinitionMetadata,
        },
    };
    use kagari_syntax::parser::ParseLimits;
    use kagari_types::{declaration::module::ModuleDecl, scalar::BuiltinType, ty::Ty};
    use std::sync::Arc;

    fn provider(name: &str) -> Arc<DefinitionMetadata<ModuleDecl>> {
        let context = DefinitionContext::new().unwrap();
        Arc::new(
            DefinitionMetadata::checked(
                context.snapshot(),
                ModuleDecl::new(ModuleIdentity::single_file(name)),
                &Default::default(),
            )
            .unwrap(),
        )
    }

    fn native_inputs(result: BuiltinType) -> Vec<Arc<DefinitionMetadata<ModuleDecl>>> {
        vec![
            provider("a"),
            provider("b"),
            function_module(
                ModuleIdentity {
                    package: PackageId("example".into()),
                    path: vec![],
                },
                "number",
                vec![],
                Ty::Builtin(result),
            ),
        ]
    }

    #[test]
    fn environment_identity_tracks_owned_contracts_and_settings() {
        let token = CancellationToken::default();
        let mut db = AnalysisDatabase::default();
        let empty = db.environment_fingerprint(&token).unwrap();
        db.set_native_modules(vec![provider("a"), provider("b")]);
        let native = db.environment_fingerprint(&token).unwrap();
        assert_ne!(native, empty);
        db.set_native_modules(vec![provider("b"), provider("a")]);
        assert_eq!(native, db.environment_fingerprint(&token).unwrap());
        db.set_native_modules(native_inputs(BuiltinType::I32));
        let callable = db.environment_fingerprint(&token).unwrap();
        assert_ne!(callable, native);
        db.set_native_modules(native_inputs(BuiltinType::I32));
        assert_eq!(
            callable,
            db.environment_fingerprint(&token).unwrap(),
            "independent definition contexts are not portable inputs"
        );
        db.set_native_modules(native_inputs(BuiltinType::I64));
        assert_ne!(callable, db.environment_fingerprint(&token).unwrap());
        db.set_native_modules(vec![provider("a"), provider("b")]);
        assert_eq!(native, db.environment_fingerprint(&token).unwrap());
        db.set_const_limits(ConstLimits {
            max_steps: 7,
            ..Default::default()
        });
        assert_ne!(native, db.environment_fingerprint(&token).unwrap());
        db.set_const_limits(Default::default());
        assert_eq!(native, db.environment_fingerprint(&token).unwrap());
        db.set_parse_limits(ParseLimits {
            max_nesting: 8,
            ..Default::default()
        });
        assert_ne!(native, db.environment_fingerprint(&token).unwrap());
        db.set_parse_limits(Default::default());
        assert_eq!(native, db.environment_fingerprint(&token).unwrap());
        db.set_max_semantic_diagnostics(13);
        assert_ne!(native, db.environment_fingerprint(&token).unwrap());
        let clone = db.clone();
        db.set_native_modules(Vec::new());
        assert_ne!(
            clone.environment_fingerprint(&token).unwrap(),
            db.environment_fingerprint(&token).unwrap()
        );
        token.cancel();
        assert!(
            clone.environment_fingerprint(&token).is_err(),
            "cached identity still observes cancellation"
        );
    }
}
