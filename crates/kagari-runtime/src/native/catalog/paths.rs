//! Explicit path capabilities consume the same installed external schema closure.
use crate::{error::RuntimeError, native::catalog::DeclarationCatalog};
use kagari_types::declaration::{TypeDeclaration, TypeDefKind};

impl DeclarationCatalog {
    pub(crate) fn validate_paths(&self) -> Result<(), RuntimeError> {
        let definitions = self.definitions();
        let schemas = self
            .types
            .entries()
            .filter(|(_, definition)| matches!(definition.kind, TypeDefKind::Host(_)))
            .map(|(declaration, definition)| TypeDeclaration {
                declaration,
                definition: definition.clone(),
            })
            .collect::<Vec<_>>();
        for (root, paths) in self.paths.entries() {
            for path in paths {
                if path.root != root {
                    return Err(RuntimeError::metadata_conflict(
                        "path catalog root differs from its declaration",
                    ));
                }
                path.contract(&schemas, &definitions)
                    .map_err(|error| RuntimeError::metadata_conflict(error.to_string()))?;
            }
        }
        Ok(())
    }
}
