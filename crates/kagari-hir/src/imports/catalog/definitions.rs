//! Exact references retain a selected exposure without bypassing catalog access checks.
use crate::{
    collections::HashSet,
    hir::ty::DefinitionExposure,
    imports::{
        NamespaceId, ResolvedTarget,
        catalog::{LookupContext, LookupHit, LookupResult, NamespaceCatalog, NamespaceResult},
    },
};
use kagari_common::{
    cancellation::{CancellationToken, Cancelled},
    identity::table::{DefinitionId, DefinitionTable, module::ModuleHandle},
};
use kagari_types::{declaration::names::NameNamespace, visibility::Visibility};
use std::collections::VecDeque;

/// Unchecked exact identity and optional authored/selected route in the current scope.
pub(crate) struct DefinitionLookup<'a> {
    pub declaration: DefinitionId,
    pub definitions: &'a DefinitionTable,
    pub namespace: NameNamespace,
    pub exposure: Option<&'a DefinitionExposure>,
}

pub(crate) struct DefinitionSelection {
    pub hit: LookupHit,
    pub exposure: DefinitionExposure,
}

impl NamespaceCatalog {
    pub(crate) fn resolve_definition(
        &self,
        ctx: &LookupContext<'_>,
        request: DefinitionLookup<'_>,
        matches: impl Fn(&ResolvedTarget) -> bool,
        cancel: &CancellationToken,
    ) -> Result<Option<DefinitionSelection>, Cancelled> {
        cancel.check()?;
        let Ok(view) = request.definitions.resolve(request.declaration) else {
            return Ok(None);
        };
        if self.definitions.module_identity(*ctx.importer).is_err() {
            return Ok(None);
        }
        let Some(module) = self.definitions.lookup_module(view.module()) else {
            return Ok(None);
        };
        // Collected declarations can belong to a newer prefix of this same scope,
        // never a foreign table carrying the same portable module spelling.
        if request.definitions.lookup_module(view.module()) != Some(module) {
            return Ok(None);
        }
        if let Some(exposure) = request.exposure {
            return Ok(self
                .lookup_exposure(ctx, exposure, request.namespace, cancel)?
                .hit()
                .filter(|hit| matches(&hit.target))
                .map(|hit| DefinitionSelection {
                    hit,
                    exposure: exposure.clone(),
                }));
        }
        let defining = DefinitionExposure::defining(view);
        if let LookupResult::Found(hit) =
            self.lookup_exposure(ctx, &defining, request.namespace, cancel)?
            && matches(&hit.target)
        {
            return Ok(Some(DefinitionSelection {
                hit,
                exposure: defining,
            }));
        }
        self.public_exposure(ctx, request.namespace, matches, cancel)
    }

    pub(crate) fn lookup_exposure(
        &self,
        ctx: &LookupContext<'_>,
        exposure: &DefinitionExposure,
        namespace: NameNamespace,
        cancel: &CancellationToken,
    ) -> Result<LookupResult, Cancelled> {
        cancel.check()?;
        if exposure.path.is_empty()
            || exposure
                .path
                .iter()
                .any(|part| part.is_empty() || part.contains("::"))
        {
            return Ok(LookupResult::Missing);
        }
        let Some(module) = self.definitions.lookup_module(&exposure.module) else {
            return Ok(LookupResult::Missing);
        };
        let Some(units) = self.modules.get(&module) else {
            return Ok(LookupResult::Missing);
        };
        if !self.exposure_module_allowed(ctx, module)
            || !self.module_accessible(ctx, &module, cancel)?
        {
            return Ok(LookupResult::Inaccessible(vec![]));
        }
        let [unit] = units.as_slice() else {
            return Ok(LookupResult::Ambiguous(vec![]));
        };
        let root = LookupResult::Found(LookupHit {
            support: HashSet::default(),
            namespace: NameNamespace::Type,
            target: ResolvedTarget::Namespace(NamespaceId::Module(*unit)),
            via: vec![],
        });
        self.resolve_path(ctx, root, &exposure.path, namespace, cancel)
    }

    /// An explicit module dependency also admits its exposed ancestors as lookup
    /// roots. Public re-exports there may expose a target whose defining module is
    /// private; access is still checked on the selected route, not the hidden path.
    pub(super) fn dependency_exposure_root(
        &self,
        importer: &ModuleHandle,
        module: &ModuleHandle,
    ) -> bool {
        let exposed = self.identity(*module);
        self.module_dependencies
            .get(importer)
            .is_some_and(|dependencies| {
                dependencies.iter().any(|dependency| {
                    let target = self.identity(*dependency);
                    target.package == exposed.package && target.path.starts_with(&exposed.path)
                })
            })
    }

    fn exposure_module_allowed(&self, ctx: &LookupContext<'_>, module: ModuleHandle) -> bool {
        let importer = self.identity(*ctx.importer);
        let exposed = self.identity(module);
        importer.package == exposed.package
            || self.dependency_exposure_root(ctx.importer, &module)
            || self.environment.scope(importer).is_some_and(|scope| {
                scope
                    .packages
                    .dependencies
                    .values()
                    .any(|package| package == &exposed.package)
            })
    }

    /// Discover only from the same package roots used by type projection and
    /// namespace-reuse validation. No global alias index or cross-snapshot cache.
    fn public_exposure(
        &self,
        ctx: &LookupContext<'_>,
        namespace: NameNamespace,
        matches: impl Fn(&ResolvedTarget) -> bool,
        cancel: &CancellationToken,
    ) -> Result<Option<DefinitionSelection>, Cancelled> {
        let mut roots = Vec::new();
        self.add_package_namespaces(ctx, &mut roots, cancel)?;
        let mut pending = VecDeque::new();
        for root in roots {
            let ResolvedTarget::Namespace(NamespaceId::Module(unit)) = root else {
                continue;
            };
            let exposure = DefinitionExposure {
                module: self.identity(unit.module).clone(),
                path: vec![],
            };
            pending.push_back((NamespaceId::Module(unit), exposure));
        }
        let mut seen = HashSet::default();
        let mut selected: Option<DefinitionSelection> = None;
        while let Some((ns, exposure)) = pending.pop_front() {
            cancel.check()?;
            // Multiple routes to the same binding/container do not make that
            // exposure ambiguous; retain the first deterministic shortest route.
            if !seen.insert(ns) {
                continue;
            }
            let Some(table) = self.namespaces.get(&ns) else {
                continue;
            };
            let mut names = table.names.entries.keys().collect::<Vec<_>>();
            names.sort();
            for name in names {
                cancel.check()?;
                for category in NameNamespace::ALL {
                    let Some(hit) = self
                        .lookup_member(ctx, &ns, name.as_str(), category, cancel)?
                        .hit()
                    else {
                        continue;
                    };
                    let public = table.names.candidates(name.as_str(), category).is_some_and(
                        |(candidates, _)| {
                            candidates.iter().any(|candidate| {
                                candidate.visibility == Visibility::Public
                                    && candidate.resolution.target() == Some(&hit.target)
                            })
                        },
                    );
                    if !public {
                        continue;
                    }
                    let mut route = exposure.clone();
                    route.path.push(name.as_str().to_owned());
                    if category == namespace && matches(&hit.target) {
                        // Distinct public binding names require an explicit route.
                        if selected.is_some() {
                            return Ok(None);
                        }
                        selected = Some(DefinitionSelection {
                            exposure: route.clone(),
                            hit: hit.clone(),
                        });
                    }
                    if category == NameNamespace::Type
                        && let NamespaceResult::Found(child) =
                            self.namespace_of(ctx, &hit.target, cancel)?
                    {
                        pending.push_back((child, route));
                    }
                }
            }
        }
        Ok(selected)
    }
}
