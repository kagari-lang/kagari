//! Validated structured inputs; lowering-local IDs are allocated fresh per request.
use super::attach;
use crate::{lower::LoweredModule, native::paths::LibraryPaths};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{ModuleIdentity, metadata::DefinitionMetadata},
};
use kagari_source::{
    source::SourceFile,
    source_database::{SourceDatabase, SourceLayer},
};
use kagari_types::declaration::{
    module::{DeclarationError, ModuleDecl},
    ownership::ReceiverOwners,
};
use std::{fmt::Write, sync::Arc};

#[derive(Debug, Clone)]
struct PreparedRegistration {
    metadata: Arc<DefinitionMetadata<ModuleDecl>>,
    key: Arc<SourceFile>,
}

#[derive(Debug, Clone)]
pub(crate) struct NativeFiles {
    prepared: Vec<PreparedRegistration>,
    paths: Arc<LibraryPaths>,
}

impl NativeFiles {
    /// Providers have already been adopted into the analysis definition table.
    pub(crate) fn new(
        modules: Vec<Arc<DefinitionMetadata<ModuleDecl>>>,
        paths: Arc<LibraryPaths>,
        source_root: &str,
        cancel: &CancellationToken,
    ) -> Result<Self, DeclarationError> {
        check_cancel(cancel)?;
        let owners = ReceiverOwners::from_types(
            modules
                .iter()
                .map(|module| module.records())
                .flat_map(|module| module.types.iter().map(|ty| (&module.identity, ty))),
        )?;
        let mut sources = SourceDatabase::new(source_root)
            .map_err(|error| DeclarationError(error.to_string()))?;
        let mut prepared = Vec::with_capacity(modules.len());
        for metadata in modules {
            check_cancel(cancel)?;
            let definition = metadata.records();
            let definitions = metadata.definitions();
            definition.validate(&|receiver| owners.owner(receiver, definitions), definitions)?;
            // An empty file supplies a graph key, not a source surface or parser input.
            let uri = registration_key(&definition.identity);
            sources
                .bind_module(&uri, definition.identity.clone())
                .map_err(|error| DeclarationError(error.to_string()))?;
            let id = sources
                .set(&uri, String::new(), SourceLayer::Base)
                .map_err(|error| DeclarationError(error.to_string()))?;
            let key = sources
                .snapshot()
                .file(id)
                .expect("registered file key")
                .clone();
            prepared.push(PreparedRegistration { metadata, key });
        }
        check_cancel(cancel)?;
        Ok(Self { prepared, paths })
    }

    pub(crate) fn attach(
        &self,
        cancel: &CancellationToken,
    ) -> Result<Vec<Arc<LoweredModule>>, DeclarationError> {
        let mut modules = Vec::with_capacity(self.prepared.len());
        for input in &self.prepared {
            check_cancel(cancel)?;
            modules.push(Arc::new(attach(
                &input.metadata,
                &self.paths,
                input.key.clone(),
                cancel,
            )?));
        }
        check_cancel(cancel)?;
        Ok(modules)
    }

    pub(crate) fn registrations(&self) -> impl Iterator<Item = &DefinitionMetadata<ModuleDecl>> {
        self.prepared.iter().map(|p| p.metadata.as_ref())
    }

    pub(crate) fn matches_paths(&self, paths: &LibraryPaths) -> bool {
        self.paths.as_ref() == paths
    }
}

// Package IDs are opaque; only the validated module path may introduce separators.
fn registration_key(owner: &ModuleIdentity) -> String {
    let mut key = String::from("kagari-registration://");
    for byte in owner.package.0.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            key.push(char::from(byte));
        } else {
            write!(key, "%{byte:02X}").expect("writing to a String cannot fail");
        }
    }
    key.push('/');
    key.push_str(&owner.path.join("/"));
    key
}

fn check_cancel(cancel: &CancellationToken) -> Result<(), DeclarationError> {
    cancel
        .check()
        .map_err(|_| DeclarationError("native declaration analysis cancelled".into()))
}
