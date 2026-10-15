//! Explicit manual paths use the module's schema and existing checked path service.
use crate::{
    error::RuntimeError,
    host::HostPathAdapter,
    native::{binding::NativeResult, builder::ModuleBuilder},
};
use kagari_common::{cancellation::CancellationToken, identity::metadata::DefinitionMetadata};
use kagari_types::declaration::path::HostPathDeclaration;

impl ModuleBuilder {
    /// Declare a manual path owned by a local external root.
    ///
    /// Adopt identities from the supplied scope. The declaration carries no Rust
    /// pointer or callback; `bind_path` supplies the checked storage operations.
    /// Reject duplicate contracts now; `finish` checks schema closure and ownership.
    pub fn define_path(
        &mut self,
        declaration: DefinitionMetadata<HostPathDeclaration>,
    ) -> NativeResult<DefinitionMetadata<HostPathDeclaration>> {
        let declaration = declaration
            .import_into(self.context(), &CancellationToken::default())
            .map_err(conflict)?;
        if self.declaration.paths.contains(declaration.records()) {
            return Err(conflict("duplicate manual path declaration"));
        }
        self.declaration.paths.push(declaration.records().clone());
        Ok(declaration)
    }

    /// Bind read and prepared-write operations to one explicitly declared path.
    ///
    /// Path operations retain their existing once-only preparation, commit and
    /// dirty-path reporting. They are not an alternate method invocation route.
    pub fn bind_path(
        &mut self,
        declaration: &DefinitionMetadata<HostPathDeclaration>,
        adapter: HostPathAdapter,
    ) -> NativeResult<()> {
        let declaration = declaration
            .import_into(self.context(), &CancellationToken::default())
            .map_err(conflict)?
            .into_records();
        if !self.declaration.paths.contains(&declaration) {
            return Err(conflict("manual path is not declared by this module"));
        }
        if self
            .path_bindings
            .iter()
            .any(|(path, _)| *path == declaration)
        {
            return Err(conflict("duplicate manual path binding"));
        }
        adapter.check_access(declaration.access)?;
        self.path_bindings.push((declaration, adapter));
        Ok(())
    }
}

fn conflict(cause: impl ToString) -> RuntimeError {
    RuntimeError::metadata_conflict(cause.to_string())
}
