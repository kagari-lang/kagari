//! Final installation-local targets. No entry owns executable storage or a predecessor.
use crate::{
    error::RuntimeError,
    module::{
        ExecutableModule, LinkedModule, ModuleKey, ModuleStore,
        dispatch::{
            CallRoute, PublicationDispatch,
            facts::RouteSelection,
            identity::{PublicationDefinitions, PublicationMapping},
            invalid,
        },
    },
};
use kagari_bytecode::module::CallableTarget;
use kagari_common::cancellation::CancellationToken;
use std::{
    collections::HashMap,
    sync::{Arc, Weak},
};

type RouteKey = (ModuleKey, CallableTarget);

#[derive(Debug)]
pub(crate) struct PreparedSelection {
    pub(super) owner: ModuleKey,
    pub(super) target: CallableTarget,
    pub(super) mapping: Arc<PublicationMapping>,
}

/// Liveness belongs to actual bindings and the supplying whole program. The
/// publication registry holds a Weak reference, so saved host bindings survive
/// code collection without making expired redirect metadata immortal.
#[derive(Debug)]
pub(super) struct RouteIdentity {
    pub(super) scope: Arc<PublicationDefinitions>,
    pub(super) origin: ModuleKey,
    pub(super) target: CallableTarget,
    pub(super) selection: Arc<RouteSelection>,
    pub(super) initial: Arc<PreparedSelection>,
}

#[derive(Debug, Clone)]
struct PublishedRoute {
    identity: Weak<RouteIdentity>,
    selected: Arc<PreparedSelection>,
}

#[derive(Debug, Default)]
pub(in crate::module) struct PublishedRoutes {
    entries: HashMap<RouteKey, PublishedRoute>,
}

impl PublicationDispatch {
    pub(in crate::module) fn bind_members(
        &mut self,
        modules: &[LinkedModule],
    ) -> Result<(), RuntimeError> {
        for (placement, record) in self.tables.facts.iter() {
            let module = &modules[placement.0.index()];
            let origin = ModuleKey {
                id: module.id,
                epoch: module.epoch,
            };
            let mapping = PublicationMapping::new(
                self.scope.clone(),
                self.scope.clone(),
                record.binders.iter().copied(),
            )?;
            let initial = Arc::new(PreparedSelection {
                owner: origin,
                target: placement.1,
                mapping: Arc::new(mapping),
            });
            self.routes.insert(
                *placement,
                CallRoute {
                    inner: Arc::new(RouteIdentity {
                        scope: self.scope.clone(),
                        origin,
                        target: placement.1,
                        selection: record.selection.clone(),
                        initial,
                    }),
                },
            );
        }
        Ok(())
    }
}

impl PublishedRoutes {
    pub(in crate::module) fn prepare(
        &self,
        candidate: &ExecutableModule,
    ) -> Result<Self, RuntimeError> {
        let mut entries = HashMap::new();
        entries
            .try_reserve(self.entries.len() + candidate.program.dispatch.routes.len())
            .map_err(|_| RuntimeError::resource_limit("publication call bindings"))?;
        for (key, entry) in &self.entries {
            let Some(route) = entry.identity.upgrade() else {
                continue;
            };
            let selected = if route.scope.publication_token().installation_id()
                == candidate.installation_id()
            {
                prepare_target(candidate, &route)?
            } else {
                entry.selected.clone()
            };
            entries.insert(
                *key,
                PublishedRoute {
                    identity: entry.identity.clone(),
                    selected,
                },
            );
        }
        for route in candidate.program.dispatch.routes.values() {
            entries.insert(
                route.key(),
                PublishedRoute {
                    identity: Arc::downgrade(&route.inner),
                    selected: route.inner.initial.clone(),
                },
            );
        }
        Ok(Self { entries })
    }

    pub(crate) fn selection(
        &self,
        route: &CallRoute,
    ) -> Result<Arc<PreparedSelection>, RuntimeError> {
        self.entries
            .get(&route.key())
            .filter(|entry| Weak::ptr_eq(&entry.identity, &Arc::downgrade(&route.inner)))
            .map(|entry| entry.selected.clone())
            .ok_or_else(|| invalid("missing admitted current call binding"))
    }
}

fn prepare_target(
    candidate: &ExecutableModule,
    route: &RouteIdentity,
) -> Result<Arc<PreparedSelection>, RuntimeError> {
    if matches!(route.selection.as_ref(), RouteSelection::BodyOwned) {
        return Ok(route.initial.clone());
    }
    let dispatch = &candidate.program.dispatch;
    let (slot, target) = dispatch.tables.select(
        &dispatch.scope,
        &route.scope,
        &route.selection,
        &CancellationToken::default(),
    )?;
    let owner = candidate
        .member(slot)
        .ok_or_else(|| invalid("prepared call target module"))?;
    let record = dispatch
        .target(slot, target)
        .ok_or_else(|| invalid("prepared call contract"))?;
    let mapping = PublicationMapping::new(
        route.scope.clone(),
        dispatch.scope.clone(),
        record.binders.iter().copied(),
    )?;
    Ok(Arc::new(PreparedSelection {
        owner: owner.key(),
        target,
        mapping: Arc::new(mapping),
    }))
}

impl CallRoute {
    fn key(&self) -> RouteKey {
        (self.inner.origin, self.inner.target)
    }

    pub(crate) fn initial_selection(&self) -> Arc<PreparedSelection> {
        self.inner.initial.clone()
    }
}

impl ModuleStore {
    pub(crate) fn prepared_route(
        &self,
        route: &CallRoute,
    ) -> Result<Arc<PreparedSelection>, RuntimeError> {
        self.inner
            .try_borrow()
            .map_err(|_| invalid("call binding store is borrowed"))?
            .routes
            .selection(route)
    }
}
