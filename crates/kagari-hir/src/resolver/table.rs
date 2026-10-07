//! Tiered module bindings; lexical bindings remain in body scopes.
use crate::{
    hir::ids::ImplId,
    imports::{
        BindingCandidate, BindingOrigin, LocalName, NameEntry, SourceUnit,
        catalog::{LookupHit, LookupResult},
    },
    resolver::resolved::ResolvedName,
};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NameResolution {
    Unique(ResolvedName),
    Ambiguous,
    Unresolved,
}

impl NameResolution {
    pub fn target(self) -> Option<ResolvedName> {
        if let Self::Unique(target) = self {
            Some(target)
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NameTable {
    pub(crate) unit: Option<SourceUnit>,
    pub(crate) entries: BTreeMap<LocalName, NameEntry>,
    impls: Vec<ImplId>,
}

impl NameTable {
    pub(crate) fn for_unit(unit: SourceUnit) -> Self {
        Self {
            unit: Some(unit),
            ..Default::default()
        }
    }

    pub(crate) fn add(&mut self, name: LocalName, candidate: BindingCandidate) {
        let entry = self.entries.entry(name).or_default();
        match candidate.origin {
            BindingOrigin::GlobImport(_) => entry.globs.push(candidate),
            BindingOrigin::Package(_) | BindingOrigin::Prelude(_) => entry.implicit.push(candidate),
            _ => entry.strong.push(candidate),
        }
    }

    pub(crate) fn candidates(&self, name: &str) -> Option<(&[BindingCandidate], bool)> {
        let entry = self.entries.get(&LocalName::new(name)?)?;
        if !entry.strong.is_empty() {
            Some((&entry.strong, true))
        } else if !entry.globs.is_empty() {
            Some((&entry.globs, false))
        } else {
            Some((&entry.implicit, false))
        }
    }

    pub(crate) fn select(candidates: &[BindingCandidate], strong: bool) -> LookupResult {
        if candidates.is_empty() {
            return LookupResult::Missing;
        }
        if strong && candidates.len() > 1 {
            return LookupResult::Ambiguous(candidates.to_vec());
        }
        let Some(target) = &candidates[0].target else {
            return LookupResult::Unresolved;
        };
        if candidates.iter().any(|c| c.target.as_ref() != Some(target)) {
            return LookupResult::Ambiguous(candidates.to_vec());
        }
        LookupResult::Found(LookupHit {
            target: target.clone(),
            via: candidates.iter().map(|c| c.origin.clone()).collect(),
        })
    }

    pub(crate) fn hit(&self, name: &str) -> LookupResult {
        self.candidates(name)
            .map_or(LookupResult::Missing, |(c, strong)| Self::select(c, strong))
    }

    pub fn lookup(&self, name: &str) -> Option<NameResolution> {
        self.candidates(name)
            .map(|(c, strong)| match Self::select(c, strong) {
                LookupResult::Found(hit) => {
                    NameResolution::Unique(hit.target.resolved(self.unit.as_ref()))
                }
                LookupResult::Ambiguous(_) => NameResolution::Ambiguous,
                _ => NameResolution::Unresolved,
            })
    }

    pub(crate) fn insert_impl(&mut self, id: ImplId) {
        self.impls.push(id);
    }

    pub fn impl_count(&self) -> usize {
        self.impls.len()
    }
}
