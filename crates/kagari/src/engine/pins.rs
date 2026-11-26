//! Callback-free installation evidence retained for emission, never a frontend input.
use crate::{
    CompileResult,
    error::{CompilationPhase, EmbeddingError},
};
use bincode::{serialize, serialize_into};
use blake3::Hasher;
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionPath, ModuleIdentity, mapping::DefinitionMappingError,
        scoped::DefinitionReferences, table::DefinitionTable,
    },
};
use kagari_compiler::source::program::InstallationTraitRequirements;
use kagari_hir::compile_input::program::CheckedCompilation;
use kagari_mir::program::VerifiedMirProgram;
use kagari_runtime::native::module::NativeModule;
use kagari_types::declaration::{TraitDef, wire::declarations::TraitDefRecord};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Default)]
pub(super) struct InstallationPins {
    by_importer: BTreeMap<ModuleIdentity, BTreeMap<DefinitionPath, Vec<u8>>>,
    fingerprint: [u8; 32],
}

impl InstallationPins {
    pub(super) fn capture(modules: &[NativeModule]) -> CompileResult<Self> {
        let mut pins = Self::default();
        let cancel = CancellationToken::default();
        for module in modules {
            for path in module.required_source_traits() {
                let expected = module
                    .required_source_trait_contract(path)
                    .map_err(|error| EmbeddingError::Registration { error })?;
                let bytes = abi_bytes(expected.records(), expected.definitions(), &cancel)?;
                pins.by_importer
                    .entry(module.declaration().identity.clone())
                    .or_default()
                    .insert(path.clone(), bytes);
            }
        }
        if !pins.by_importer.is_empty() {
            let mut hash = Hasher::new_derive_key("kagari installation trait pins v1");
            serialize_into(&mut hash, &pins.by_importer).map_err(emission_error)?;
            pins.fingerprint = *hash.finalize().as_bytes();
        }
        Ok(pins)
    }

    pub(super) fn fingerprint(&self) -> &[u8; 32] {
        &self.fingerprint
    }

    /// Select by importer membership in the checked closure, not by trait use.
    /// Pass only identities: the compiler derives the ABI from the checked owner.
    pub(super) fn requirements(
        &self,
        checked: &CheckedCompilation<'_>,
    ) -> InstallationTraitRequirements {
        InstallationTraitRequirements {
            by_importer: checked
                .modules()
                .iter()
                .filter_map(|module| {
                    let importer = module.source().module_identity();
                    let pins = self.by_importer.get(importer)?;
                    Some((importer.clone(), pins.keys().cloned().collect()))
                })
                .collect(),
        }
    }

    pub(super) fn validate(
        &self,
        mir: &VerifiedMirProgram,
        cancel: &CancellationToken,
    ) -> CompileResult<()> {
        for module in mir.modules() {
            let Some(pins) = self.by_importer.get(&module.identity) else {
                continue;
            };
            let defs = mir.definitions();
            let mut matched = BTreeSet::new();
            for reference in &module.abi.trait_references {
                cancel.check().map_err(|_| EmbeddingError::Cancelled)?;
                let path = defs
                    .resolve(reference.declaration)
                    .map_err(emission_error)?
                    .to_path();
                if let Some(expected) = pins.get(&path) {
                    if abi_bytes(&reference.abi, defs, cancel)? != *expected {
                        return Err(emission_error(format!(
                            "source trait references differ: {} requires the exact public authored trait {path:?}",
                            module.identity
                        )));
                    }
                    matched.insert(path);
                }
            }
            if let Some(missing) = pins.keys().find(|p| !matched.contains(*p)) {
                return Err(emission_error(format!(
                    "{} missing mandatory source trait reference {missing:?}",
                    module.identity
                )));
            }
        }
        Ok(())
    }
}

// Include the referenced identity table: matching file-local slots alone would
// accept a different nominal type in a method signature. Compare bytes, not hashes.
fn abi_bytes(
    abi: &TraitDef,
    definitions: &DefinitionTable,
    cancel: &CancellationToken,
) -> CompileResult<Vec<u8>> {
    let convert = |error| match error {
        DefinitionMappingError::Cancelled => EmbeddingError::Cancelled,
        error => emission_error(error),
    };
    let mut ids = BTreeSet::new();
    abi.visit_definition_ids(
        &mut |id| {
            ids.insert(*id);
            Ok(())
        },
        cancel,
    )
    .map_err(convert)?;
    let encoding = definitions.encode(ids).map_err(emission_error)?;
    let record = TraitDefRecord::encode(abi, &encoding, cancel).map_err(convert)?;
    serialize(&(encoding.table, record)).map_err(emission_error)
}

fn emission_error(error: impl ToString) -> EmbeddingError {
    EmbeddingError::Compilation {
        phase: CompilationPhase::MirLowering,
        message: error.to_string(),
    }
}
