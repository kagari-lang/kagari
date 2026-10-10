//! Shared preparation of immutable aggregate parameter scopes, without executable roots.
#[cfg(feature = "execution-diagnostics")]
use crate::diagnostics::{self, Event};
use crate::{
    Runtime,
    error::RuntimeError,
    frame::types::{arguments::TypeArgument, bindings::TypeBindings, compatibility::TypeIdentity},
    module::{LoadedModule, descriptor_index::DescriptorIndex},
};
use kagari_common::identity::table::DefinitionId;
use kagari_types::ty::GenericParam;
use std::sync::Arc;

pub(super) type LayoutScopes =
    DescriptorIndex<DefinitionId, Arc<[Arc<TypeIdentity>]>, Arc<TypeBindings>>;

impl Runtime {
    /// Layout callers check template compatibility before installing this scope.
    /// Retired provenance remains readable; it never grants executable admission.
    pub(crate) fn prepare_layout_scope(
        &self,
        owner: &LoadedModule,
        declaration: DefinitionId,
        arguments: &[TypeArgument],
    ) -> Result<Option<Arc<TypeBindings>>, RuntimeError> {
        if !owner.belongs_to(self.host.owner()) {
            return Err(RuntimeError::module_validation("foreign layout scope"));
        }
        for argument in arguments {
            argument.validate(self)?;
        }
        if !arguments.iter().any(TypeArgument::has_origin) {
            return Ok(None);
        }
        let identities = arguments
            .iter()
            .map(|argument| {
                argument
                    .identity(owner)
                    .ok_or_else(|| RuntimeError::module_validation("layout argument identity"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let root = owner.program_root();
        let mut records = self.modules.inner.try_borrow_mut().ok();
        let scopes = records
            .as_deref_mut()
            .and_then(|records| records.resolve_mut(&root))
            .map(|record| &mut record.layouts.scopes);
        if let Some(scope) = scopes
            .as_ref()
            .and_then(|scopes| scopes.get(&declaration, identities.as_slice()))
        {
            return Ok(Some(scope.clone()));
        }
        #[cfg(feature = "execution-diagnostics")]
        diagnostics::record(Event::LayoutScopePreparation);
        let parameters = (0..arguments.len())
            .map(|position| GenericParam {
                owner: declaration,
                position,
            })
            .collect();
        let scope = Arc::new(TypeBindings::new(
            self.definition_context(),
            parameters,
            arguments.to_vec(),
        )?);
        if let Some(scopes) = scopes {
            // Optional bounded retention must not turn pure type provenance into
            // a program/environment lease. Detached reads also use this path.
            let _ = scopes.insert(declaration, identities.into(), scope.clone());
        }
        Ok(Some(scope))
    }
}
