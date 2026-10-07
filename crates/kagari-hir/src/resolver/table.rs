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

/// Compact module-binding outcome used by the body resolver.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NameResolution {
    /// One selected resolver target.
    Unique(ResolvedName),
    /// Conflicting candidates at the selected precedence tier.
    Ambiguous,
    /// A spelling exists but does not have a unique resolved destination.
    Unresolved,
}

impl NameResolution {
    /// Extracts only a unique target; ambiguous and unresolved results return `None`.
    pub fn target(self) -> Option<ResolvedName> {
        if let Self::Unique(target) = self {
            Some(target)
        } else {
            None
        }
    }
}

/// One module/namespace's tiered bindings, keyed by unqualified local name.
///
/// ```text
/// entries: BTreeMap<LocalName, NameEntry>
/// "sum" -> strong: [Declaration(SourceDeclRef { unit: this, item: Function(f) })]
/// "add" -> strong: [NamedImport(d) -> SourceDeclRef { unit: math, item: Function(g) }]
/// "x"   -> globs:  [GlobImport(d1) -> target, GlobImport(d2) -> target]
/// ```
///
/// Payloads are abbreviated. Local scope location does not imply local target
/// ownership. `unit` identifies the source owner when present; synthetic namespace
/// tables can have none. Function parameters and locals live in separate lexical
/// scopes in [`crate::resolver::resolved::ResolvedNames`].
///
/// [`NameEntry`] explains precedence; [`Self::lookup`] is a local-name operation,
/// not qualified-path traversal or importer-relative visibility filtering.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NameTable {
    /// Source owner for localizing targets; absent for synthetic namespace tables.
    pub(crate) unit: Option<SourceUnit>,
    /// Ordered local spellings and all tiered binding candidates.
    pub(crate) entries: BTreeMap<LocalName, NameEntry>,
    /// Local implementation handles retained for surface/cache comparison.
    impls: Vec<ImplId>,
}

impl NameTable {
    pub(crate) fn for_unit(unit: SourceUnit) -> Self {
        Self {
            unit: Some(unit),
            ..Default::default()
        }
    }

    /// Appends a candidate to its origin-defined tier without choosing a winner.
    pub(crate) fn add(&mut self, name: LocalName, candidate: BindingCandidate) {
        let entry = self.entries.entry(name).or_default();
        match candidate.origin {
            BindingOrigin::GlobImport(_) => entry.globs.push(candidate),
            BindingOrigin::Package(_) | BindingOrigin::Prelude(_) => entry.implicit.push(candidate),
            _ => entry.strong.push(candidate),
        }
    }

    /// Returns the first nonempty precedence tier and whether it is strong.
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

    /// Selects a unique canonical target while retaining equal-target weak origins; duplicate strong entries are ambiguous.
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

    /// Looks up one local spelling and converts a selected target for this table's unit.
    ///
    /// Returns `None` when the spelling is absent/invalid, distinct from
    /// `Some(Unresolved)` when a binding exists without a target. This does not walk
    /// `::` suffixes or filter visibility for a foreign importer.
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

    /// Returns the number of local implementation handles retained by this table.
    pub fn impl_count(&self) -> usize {
        self.impls.len()
    }
}
