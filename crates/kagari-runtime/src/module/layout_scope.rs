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
use kagari_types::ty::{GenericParam, Ty};
use std::{num::NonZeroUsize, sync::Arc};

#[derive(Debug)]
pub(crate) struct LayoutScope {
    owner: LoadedModule,
    id: Option<NonZeroUsize>,
    bindings: Arc<TypeBindings>,
    arguments: Arc<[Arc<TypeIdentity>]>,
}

impl LayoutScope {
    pub(crate) fn bindings(&self) -> &Arc<TypeBindings> {
        &self.bindings
    }

    pub(super) fn id_for(&self, owner: &LoadedModule) -> Option<NonZeroUsize> {
        Arc::ptr_eq(&self.owner.program, &owner.program)
            .then_some(self.id)
            .flatten()
    }

    pub(super) fn accepts(
        &self,
        declaration: DefinitionId,
        arguments: &[Ty<DefinitionId>],
    ) -> bool {
        self.arguments.len() == arguments.len()
            && arguments.iter().enumerate().all(|(position, ty)| {
                self.bindings
                    .argument(&declaration, position)
                    .is_some_and(|argument| argument.ty() == ty)
            })
    }

    pub(super) fn arguments(&self) -> &Arc<[Arc<TypeIdentity>]> {
        &self.arguments
    }
}

#[derive(Debug, Default)]
pub(super) struct LayoutScopes {
    entries: DescriptorIndex<DefinitionId, Arc<[Arc<TypeIdentity>]>, Arc<LayoutScope>>,
    next: usize,
}

impl Runtime {
    /// Layout callers check template compatibility before installing this scope.
    /// Retired provenance remains readable; it never grants executable admission.
    pub(crate) fn prepare_layout_scope(
        &self,
        owner: &LoadedModule,
        declaration: DefinitionId,
        arguments: &[TypeArgument],
    ) -> Result<Option<Arc<LayoutScope>>, RuntimeError> {
        if !owner.belongs_to(self.host.owner()) {
            return Err(RuntimeError::module_validation("foreign layout scope"));
        }
        for argument in arguments {
            argument.validate(self)?;
            argument.prepare_admission(self, owner);
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
            .and_then(|record| record.program_layouts())
            .map(|layouts| &mut layouts.scopes);
        if let Some(scope) = scopes
            .as_ref()
            .and_then(|scopes| scopes.entries.get(&declaration, identities.as_slice()))
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
        let bindings = Arc::new(TypeBindings::new(
            self.definition_context(),
            parameters,
            arguments.to_vec(),
        )?);
        let mut scope = LayoutScope {
            owner: root,
            id: None,
            bindings,
            arguments: identities.into(),
        };
        if let Some(scopes) = scopes {
            // IDs are never recycled, even after optional retention is evicted.
            // Exhaustion loses reuse, not correctness or detached readability.
            if let Some(next) = scopes.next.checked_add(1) {
                scopes.next = next;
                scope.id = NonZeroUsize::new(next);
            }
            let scope = Arc::new(scope);
            let _ = scopes
                .entries
                .insert(declaration, scope.arguments.clone(), scope.clone());
            return Ok(Some(scope));
        }
        Ok(Some(Arc::new(scope)))
    }
}
