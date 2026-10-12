//! Storage views follow installed implementation headers and declared parents.
use crate::{
    aggregates::AggregateCatalog,
    typeck::{inference, table::match_implementation},
    types::{NominalType, TypeId, TypeSubstitution},
};
use {kagari_common::cancellation::CancellationToken, kagari_types::collection::CollectionAccess};

impl AggregateCatalog {
    pub(crate) fn storage_views(
        &self,
        ty: &TypeId,
        cancel: &CancellationToken,
    ) -> Option<Vec<NominalType>> {
        cancel.check().ok()?;
        let mut roots = vec![];
        if let TypeId::Trait(interface) = ty {
            roots.push(interface.clone());
        } else {
            for implementation in self
                .implementations()
                .filter(|implementation| implementation.engine_owned)
            {
                cancel.check().ok()?;
                let mut substitution = TypeSubstitution::default();
                if inference::infer(
                    &implementation.for_type,
                    ty,
                    &implementation.generic_params,
                    &mut substitution,
                    cancel,
                    None,
                )
                .is_err()
                {
                    continue;
                }
                let interface = implementation.trait_type.instantiate(&substitution);
                if match_implementation(
                    &implementation.trait_type,
                    &interface,
                    &implementation.for_type,
                    ty,
                    &implementation.generic_params,
                )
                .is_some()
                {
                    roots.push(interface);
                }
            }
        }
        let mut views = vec![];
        for root in roots {
            for interface in self.trait_closure(&root, ty, cancel).ok()? {
                cancel.check().ok()?;
                if self.trait_(&interface.declaration).is_some_and(|contract| {
                    contract.storage_access == Some(CollectionAccess::ReadOnly)
                }) && !views.contains(&interface)
                {
                    views.push(interface);
                }
            }
        }
        Some(views)
    }

    pub(crate) fn has_storage_view(&self, ty: &TypeId, cancel: &CancellationToken) -> bool {
        self.storage_views(ty, cancel)
            .is_some_and(|views| !views.is_empty())
    }
}
