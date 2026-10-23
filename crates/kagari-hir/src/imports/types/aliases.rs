//! Declaration-scoped alias preparation shared by every importer in a snapshot.

use crate::{
    DeclaredAnalysis,
    declarations::{DeclarationId, Declarations},
    hir::{
        ids::TypeRefId,
        item::{Module, alias::TypeAlias},
        ty::TypeKind,
    },
    imports::{SourceItem, types::TypeCatalog},
    resolver::resolved::ResolvedName,
    typeck::{GenericBounds, aliases},
    types::{GenericParameterType, TypeId, TypeSubstitution},
};
use kagari_common::{
    cancellation::{CancellationToken, Cancelled},
    identity::table::DefinitionId,
};
use kagari_types::{declaration::names::NameNamespace, visibility::Visibility};
use std::collections::{HashMap, HashSet};

/// A transparent target template with canonical declaration-owned binders.
///
/// This is source-semantic metadata, not an executable export or nominal type.
/// Consumers substitute `parameters` into `target`; the alias definition itself
/// never appears as a nominal declaration in that target. Failed definitions
/// retain an error target and a diagnostic reason for recovery/navigation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalAlias {
    /// Parameters in source declaration order, identified by owner and position.
    pub parameters: Vec<GenericParameterType>,
    /// Fully expanded target before substituting this alias's parameters.
    pub target: TypeId,
    /// Checked requirements on the alias inputs, including unused parameters.
    pub bounds: GenericBounds,
    /// Definition failure; absent for a successfully prepared template.
    pub error: Option<String>,
}

impl CanonicalAlias {
    /// Applies exactly the declared arguments, bounding expansion before allocation.
    pub(crate) fn apply(
        &self,
        arguments: &[TypeId],
        budget: &mut aliases::ExpansionBudget,
        cancel: &CancellationToken,
    ) -> TypeId {
        if self.parameters.len() != arguments.len()
            || self.error.is_some()
            || arguments.iter().any(TypeId::is_unresolved)
        {
            return TypeId::Error;
        }
        let substitution: TypeSubstitution = self
            .parameters
            .iter()
            .cloned()
            .zip(arguments.iter().cloned())
            .collect();
        if budget.check(&self.target, &substitution, cancel).is_none() {
            return TypeId::Error;
        }
        self.target.instantiate(&substitution)
    }
}

impl TypeCatalog<'_> {
    pub(super) fn prepare_aliases(&self, cancel: &CancellationToken) -> Result<(), Cancelled> {
        let mut owners = HashMap::new();
        for module in self.modules.values() {
            for alias in &module.lowered.module.type_aliases {
                cancel.check()?;
                if let Some(id) = module
                    .declarations
                    .definition(ResolvedName::TypeAlias(alias.id))
                {
                    owners.insert(*id, (*module, alias));
                }
            }
        }
        // Classify dependency depth independently of DFS/cache order. Each round
        // admits exactly one declaration level; unresolved vertices after 64
        // rounds are cyclic or depend on a path longer than the declared limit.
        let mut dependencies = HashMap::new();
        for (&id, &(module, alias)) in &owners {
            let mut edges = HashSet::new();
            for reference in alias_references(&module.lowered.module, alias, cancel)? {
                let (TypeKind::Named(name) | TypeKind::Generic { name, .. }) =
                    &module.lowered.module.type_ref(reference).kind
                else {
                    continue;
                };
                if alias
                    .generic_params
                    .iter()
                    .any(|parameter| parameter.name == *name)
                {
                    continue;
                }
                if let Some(dependency) = self.alias_dependency(module, name, cancel) {
                    edges.insert(dependency);
                }
            }
            dependencies.insert(id, edges);
        }
        let mut admitted = HashSet::new();
        for _ in 0..64 {
            cancel.check()?;
            let ready = dependencies
                .iter()
                .filter(|(id, edges)| {
                    !admitted.contains(*id) && edges.iter().all(|id| admitted.contains(id))
                })
                .map(|(&id, _)| id)
                .collect::<Vec<_>>();
            if ready.is_empty() {
                break;
            }
            admitted.extend(ready);
        }
        for (&id, &(module, alias)) in &owners {
            if !admitted.contains(&id) {
                self.aliases.borrow_mut().insert(id, CanonicalAlias {
                    parameters: module.declarations.parameters_of(&id),
                    target: TypeId::Error,
                    bounds: Default::default(),
                    error: Some(format!("cyclic type alias dependency or expansion exceeds 64 declaration levels through `{}`", alias.name)),
                });
            }
        }
        let mut pending = owners.keys().copied().collect::<Vec<_>>();
        pending.sort();
        for id in pending {
            self.prepare_alias(id, &owners, &mut HashSet::new(), cancel)?;
        }
        Ok(())
    }

    fn prepare_alias(
        &self,
        id: DefinitionId,
        owners: &HashMap<DefinitionId, (&DeclaredAnalysis, &TypeAlias)>,
        active: &mut HashSet<DefinitionId>,
        cancel: &CancellationToken,
    ) -> Result<(), Cancelled> {
        cancel.check()?;
        if self.aliases.borrow().contains_key(&id) {
            return Ok(());
        }
        let Some(&(module, alias)) = owners.get(&id) else {
            return Ok(());
        };
        let mut result = CanonicalAlias {
            parameters: module.declarations.parameters_of(&id),
            target: TypeId::Error,
            bounds: Default::default(),
            error: None,
        };
        if active.contains(&id) {
            result.error = Some(format!(
                "cyclic type alias expansion through `{}`",
                alias.name
            ));
        } else if active.len() >= 64 {
            result.error = Some("type alias expansion exceeds 64 declaration levels".into());
        } else {
            active.insert(id);
            for reference in alias_references(&module.lowered.module, alias, cancel)? {
                cancel.check()?;
                let (TypeKind::Named(name) | TypeKind::Generic { name, .. }) =
                    &module.lowered.module.type_ref(reference).kind
                else {
                    continue;
                };
                if alias
                    .generic_params
                    .iter()
                    .any(|parameter| parameter.name == *name)
                {
                    continue;
                }
                let dependency = self.alias_dependency(module, name, cancel);
                if let Some(dependency) = dependency {
                    self.prepare_alias(dependency, owners, active, cancel)?;
                    if let Some(error) = self
                        .aliases
                        .borrow()
                        .get(&dependency)
                        .and_then(|alias| alias.error.as_ref())
                    {
                        result.error = Some(error.clone());
                    }
                }
                if !self.alias_visibility_allowed(module, alias.visibility, name, cancel) {
                    result.error = Some(format!(
                        "target `{name}` is less visible than alias `{}`",
                        alias.name
                    ));
                }
            }
            if result.error.is_none() {
                let mut declarations = module.declarations.clone();
                declarations.imported_types = self.bindings(module.names.facts(), cancel)?;
                declarations.publish_definitions(declarations.context().snapshot());
                result = aliases::prepare(&module.lowered, alias, &declarations, cancel);
            }
            active.remove(&id);
        }
        self.aliases.borrow_mut().insert(id, result);
        Ok(())
    }

    fn alias_dependency(
        &self,
        module: &DeclaredAnalysis,
        name: &str,
        cancel: &CancellationToken,
    ) -> Option<DefinitionId> {
        let resolved = module
            .names
            .facts
            .catalog
            .resolve_name(&module.names.facts.items, name, NameNamespace::Type, cancel)?
            .target
            .resolved(module.names.facts.items.unit.as_ref());
        match resolved {
            ResolvedName::TypeAlias(local) => module
                .declarations
                .definition(ResolvedName::TypeAlias(local))
                .copied(),
            ResolvedName::Source(source) if matches!(source.item, SourceItem::TypeAlias(_)) => self
                .modules
                .get(&source.unit.file)
                .filter(|owner| {
                    source
                        .unit
                        .matches(&owner.lowered, &owner.names.facts.catalog.definitions)
                })
                .and_then(|owner| owner.declarations.definition(source.item.local()?).copied()),
            _ => None,
        }
    }

    fn alias_visibility_allowed(
        &self,
        module: &DeclaredAnalysis,
        visibility: Visibility,
        name: &str,
        cancel: &CancellationToken,
    ) -> bool {
        let Some(resolved) = module
            .names
            .facts
            .catalog
            .resolve_name(&module.names.facts.items, name, NameNamespace::Type, cancel)
            .map(|hit| hit.target.resolved(module.names.facts.items.unit.as_ref()))
        else {
            return true;
        };
        let (owner, resolved) = match resolved {
            ResolvedName::Source(source) => {
                let Some(owner) = self.modules.get(&source.unit.file) else {
                    return false;
                };
                let Some(local) = source.item.local() else {
                    return false;
                };
                (*owner, local)
            }
            local => (module, local),
        };
        let lowered = &owner.lowered.module;
        let target_visibility = match resolved {
            ResolvedName::TypeAlias(id) => lowered.type_aliases[id.index()].visibility,
            ResolvedName::Struct(id) => lowered.structs[id.index()].visibility,
            ResolvedName::Enum(id) => lowered.enums[id.index()].visibility,
            ResolvedName::Trait(id) => lowered.traits[id.index()].visibility,
            ResolvedName::OpaqueType(id) => lowered.opaque_types[id.index()].visibility,
            _ => return true,
        };
        target_visibility.allows_reexport(
            owner.lowered.source.module_identity(),
            visibility,
            module.lowered.source.module_identity(),
        )
    }
}

/// Iterates target and bound syntax without recursively expanding declarations.
pub(crate) fn alias_references(
    module: &Module,
    alias: &TypeAlias,
    cancel: &CancellationToken,
) -> Result<Vec<TypeRefId>, Cancelled> {
    let mut pending = alias.target.into_iter().collect::<Vec<_>>();
    pending.extend(
        alias
            .generic_params
            .iter()
            .flat_map(|param| &param.bounds)
            .map(|bound| bound.ty),
    );
    for bound in &alias.bounds {
        pending.push(bound.target_ref);
        pending.extend(bound.traits.iter().map(|bound| bound.ty));
    }
    let mut result = Vec::new();
    while let Some(id) = pending.pop() {
        cancel.check()?;
        result.push(id);
        match &module.type_ref(id).kind {
            TypeKind::Named(_) => {}
            TypeKind::Generic { args, bindings, .. } => {
                pending.extend(args);
                pending.extend(bindings.iter().map(|(_, ty)| ty));
            }
            TypeKind::Projection {
                arguments,
                receiver,
                trait_ref,
                ..
            } => {
                pending.extend(arguments);
                pending.extend([receiver, trait_ref]);
            }
            TypeKind::Tuple(elements) => pending.extend(elements),
            TypeKind::Array(element) => pending.push(*element),
            TypeKind::Function { params, result } => {
                pending.extend(params);
                pending.push(*result);
            }
        }
    }
    Ok(result)
}

/// Finds alias metadata without resolving its target in the caller's scope.
pub(crate) fn resolved_alias<'a>(
    declarations: &'a Declarations,
    name: &ResolvedName,
) -> Option<(DefinitionId, &'a CanonicalAlias)> {
    let id = declarations.definition(name.clone()).or_else(|| {
        let DeclarationId::Definition(id) = &declarations
            .imported_types()
            .resolved(name.clone())?
            .declaration
            .id
        else {
            return None;
        };
        Some(id)
    })?;
    declarations
        .imported_types()
        .alias(id)
        .map(|alias| (*id, alias))
}
