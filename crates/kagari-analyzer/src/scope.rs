//! Temporary visibility projection over retained resolver history until HR06.
use kagari_hir::{
    hir::ids::HirOwner,
    resolver::resolved::{ResolvedName, ResolvedNames, ScopeBinding},
};
use std::{
    cmp::Reverse,
    collections::{HashMap, HashSet},
};

/// Returns visible lexical bindings at a byte offset, sorted by name.
///
/// Selects the smallest containing scope (latest entry breaks equal-range ties),
/// then walks parents. Later visible bindings shadow earlier/outer spellings.
/// Module-level declarations are not included; offsets outside all scopes yield none.
pub(crate) fn visible_bindings(names: &ResolvedNames, offset: usize) -> Vec<&ScopeBinding> {
    let mut scope = names
        .scopes()
        .iter()
        .enumerate()
        .filter(|(_, scope)| scope.span.start <= offset && offset < scope.span.end)
        .min_by_key(|(id, scope)| (scope.span.end - scope.span.start, Reverse(*id)))
        .map(|(id, _)| id);
    let owner = scope.map(|id| names.scopes()[id].owner);
    let mut seen = HashSet::new();
    let mut visible = HashMap::new();
    while let Some(id) = scope {
        for binding in names.scopes()[id].bindings.iter().rev() {
            if binding.visible_from <= offset && seen.insert(binding.name.as_str()) {
                let accessible = match binding.resolved {
                    ResolvedName::Local(id) => Some(id.owner()) == owner.map(HirOwner::Body),
                    ResolvedName::Param(id) => Some(id.owner()) == owner.map(HirOwner::Body),
                    _ => true,
                };
                if accessible {
                    visible.insert(binding.name.as_str(), binding);
                }
            }
        }
        scope = names.scopes()[id].parent;
    }
    let mut visible = visible.into_values().collect::<Vec<_>>();
    visible.sort_by(|a, b| a.name.cmp(&b.name));
    visible
}
