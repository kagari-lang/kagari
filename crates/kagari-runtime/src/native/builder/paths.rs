//! Explicit manual paths use the module's schema and existing checked path service.
use crate::{
    error::RuntimeError,
    host::HostPathAdapter,
    native::{
        binding::{BindingEntry, Codec, NativeBinding, NativeResult},
        builder::ModuleBuilder,
        types::FunctionRef,
    },
};
use kagari_common::{cancellation::CancellationToken, identity::metadata::DefinitionMetadata};
use kagari_types::declaration::path::{
    HostPathDeclaration, NativePathOperation, NativePathOperationKind,
};

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

    /// Bind a declared Native member or function to one declared path. Its first
    /// argument is the owned root handle, followed by indices in segment order
    /// and, for Set/Add, the RHS. The path service alone acquires the root loan.
    /// `finish` checks the signature and underlying storage adapter. Loading an
    /// artifact separately checks its requirement against this installation.
    pub fn bind_path_operation(
        &mut self,
        function: FunctionRef,
        path: &DefinitionMetadata<HostPathDeclaration>,
        kind: NativePathOperationKind,
    ) -> NativeResult<()> {
        let path = path
            .import_into(self.context(), &Default::default())
            .map_err(conflict)?
            .into_records();
        if !self.declaration.paths.contains(&path) {
            return Err(conflict(
                "native operation requires a locally declared path",
            ));
        }
        let id = function.id();
        let declaration = self
            .declaration
            .native_declarations(&self.providers.definitions())
            .map_err(conflict)?
            .into_iter()
            .find(|decl| decl.declaration == id)
            .ok_or_else(|| conflict("unknown native path operation declaration"))?;
        let operation = NativePathOperation { path, kind };
        if self.bindings.contains_key(&id)
            || declaration
                .path_operation
                .as_ref()
                .is_some_and(|existing| existing != &operation)
        {
            return Err(conflict(
                "duplicate or conflicting native path operation binding",
            ));
        }
        let policy = declaration
            .function
            .host_policy
            .ok_or_else(|| conflict("native path operation requires an explicit host policy"))?;
        self.bindings.insert(
            id,
            NativeBinding {
                arguments: vec![Codec::Value; declaration.function.params.len()].into(),
                result: Codec::Value,
                host_policy: Some(policy),
                entry: BindingEntry::PathOperation,
                converted_result: true,
                receiver_access: None,
                requirement_owner: None,
                prepare_mapping: None,
            },
        );
        self.declaration.path_operations.insert(id, operation);
        Ok(())
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
