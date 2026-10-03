//! Contextual indexes retain short keys and materialize paths only at boundaries.
use crate::identity::{
    DefinitionPath,
    table::{
        DefinitionId, DefinitionRemap, DefinitionTable, DefinitionTableBuilder,
        DefinitionTableError,
    },
};
use crate::{
    cancellation::CancellationToken,
    identity::mapping::{DefinitionMapper, DefinitionMappingError, DefinitionRecord},
};
use std::{
    collections::{BTreeMap, HashSet},
    sync::{Arc, Mutex},
};

/// Clones share one append-only writer. Unlike cloning a builder, this cannot
/// assign the same scoped index to different declarations on independent branches.
#[derive(Debug, Clone)]
pub struct DefinitionContext(Arc<Mutex<DefinitionTableBuilder>>);

impl DefinitionContext {
    pub fn new() -> Result<Self, DefinitionTableError> {
        Ok(Self(Arc::new(Mutex::new(DefinitionTableBuilder::new()?))))
    }

    pub fn snapshot(&self) -> DefinitionTable {
        self.0.lock().expect("definition context poisoned").freeze()
    }

    pub fn intern(&self, path: &DefinitionPath) -> Result<DefinitionId, DefinitionTableError> {
        self.0
            .lock()
            .expect("definition context poisoned")
            .intern_path(path)
    }

    pub fn lookup(&self, path: &DefinitionPath) -> Option<DefinitionId> {
        self.snapshot().lookup(path)
    }

    pub fn import(
        &self,
        source: &DefinitionTable,
        ids: impl IntoIterator<Item = DefinitionId>,
    ) -> Result<DefinitionRemap, DefinitionTableError> {
        self.0
            .lock()
            .expect("definition context poisoned")
            .import(source, ids)
    }
}

/// A metadata index in one explicit context. Path-taking operations are authoring
/// and linking boundaries; stored keys, clones and same-context searches use IDs.
#[derive(Debug, Clone)]
pub struct DefinitionMap<T> {
    context: DefinitionContext,
    entries: BTreeMap<DefinitionId, T>,
}

impl<T: PartialEq> PartialEq for DefinitionMap<T> {
    fn eq(&self, other: &Self) -> bool {
        if self.context.snapshot().id() == other.context.snapshot().id() {
            self.entries == other.entries
        } else {
            self.iter().eq(other.iter())
        }
    }
}

impl<T: Eq> Eq for DefinitionMap<T> {}

impl<T> Default for DefinitionMap<T> {
    fn default() -> Self {
        Self::new(DefinitionContext::new().expect("definition context identity exhausted"))
    }
}

impl<T> DefinitionMap<T> {
    /// Transform record representation while retaining this explicit index context.
    pub fn map_values<U, E>(
        &self,
        mut transform: impl FnMut(&T) -> Result<U, E>,
    ) -> Result<DefinitionMap<U>, E> {
        let entries = self
            .entries
            .iter()
            .map(|(id, value)| Ok((*id, transform(value)?)))
            .collect::<Result<_, E>>()?;
        Ok(DefinitionMap {
            context: self.context.clone(),
            entries,
        })
    }
    pub fn new(context: DefinitionContext) -> Self {
        Self {
            context,
            entries: BTreeMap::new(),
        }
    }

    pub fn context(&self) -> &DefinitionContext {
        &self.context
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    pub fn retain(&mut self, mut keep: impl FnMut(&DefinitionPath, &T) -> bool) {
        let definitions = self.context.snapshot();
        self.entries.retain(|id, value| {
            let path = definitions
                .resolve(*id)
                .expect("index owns its definition")
                .to_path();
            keep(&path, value)
        });
    }

    pub fn get(&self, path: &DefinitionPath) -> Option<&T> {
        self.entries.get(&self.context.lookup(path)?)
    }

    pub fn get_id(&self, id: DefinitionId) -> Option<&T> {
        self.entries.get(&id)
    }

    pub fn contains_key(&self, path: &DefinitionPath) -> bool {
        self.get(path).is_some()
    }

    pub fn insert(
        &mut self,
        path: DefinitionPath,
        value: T,
    ) -> Result<Option<T>, DefinitionTableError> {
        let id = self.context.intern(&path)?;
        Ok(self.entries.insert(id, value))
    }

    pub fn values(&self) -> impl Iterator<Item = &T> {
        self.entries.values()
    }

    pub fn ids(&self) -> impl Iterator<Item = DefinitionId> + '_ {
        self.entries.keys().copied()
    }

    pub fn entries(&self) -> impl Iterator<Item = (DefinitionId, &T)> + '_ {
        self.entries.iter().map(|(id, value)| (*id, value))
    }

    pub fn keys(&self) -> impl Iterator<Item = DefinitionPath> + '_ {
        self.iter().map(|(path, _)| path)
    }

    /// Exact path ordering is preserved for authoring/proof consumers. ID index
    /// order is deliberately not a portable or semantic ordering contract.
    pub fn iter(&self) -> impl Iterator<Item = (DefinitionPath, &T)> + '_ {
        let definitions = self.context.snapshot();
        let mut entries: Vec<_> = self
            .entries
            .iter()
            .map(|(id, value)| {
                (
                    definitions
                        .resolve(*id)
                        .expect("index owns its definition")
                        .to_path(),
                    value,
                )
            })
            .collect();
        entries.sort_by(|(left, _), (right, _)| left.cmp(right));
        entries.into_iter()
    }

    fn remap_from(&self, source: &Self) -> Result<DefinitionRemap, DefinitionTableError> {
        self.context
            .import(&source.context.snapshot(), source.ids())
    }

    pub fn union_len(&self, source: &Self) -> Result<usize, DefinitionTableError> {
        let remap = self.remap_from(source)?;
        let mut count = self.len();
        for id in source.ids() {
            if !self.entries.contains_key(&remap.map(id)?) {
                count += 1;
            }
        }
        Ok(count)
    }

    pub fn is_subset_of(&self, target: &Self) -> Result<bool, DefinitionTableError>
    where
        T: PartialEq,
    {
        let remap = target.remap_from(self)?;
        for (id, value) in &self.entries {
            if target.entries.get(&remap.map(*id)?) != Some(value) {
                return Ok(false);
            }
        }
        Ok(true)
    }

    pub fn remove_keys(&mut self, source: &Self) -> Result<(), DefinitionTableError> {
        let remap = self.remap_from(source)?;
        for id in source.ids() {
            self.entries.remove(&remap.map(id)?);
        }
        Ok(())
    }

    /// Imports each source identity/ancestor once without cloning owned paths.
    /// Reject conflicts before publishing any value into this index. Interned
    /// names themselves carry no authority and may remain after a rejected merge.
    pub fn merge(&mut self, source: &Self) -> Result<bool, DefinitionTableError>
    where
        T: Clone + PartialEq,
    {
        let remap = self.remap_from(source)?;
        for (id, value) in &source.entries {
            if self
                .entries
                .get(&remap.map(*id)?)
                .is_some_and(|previous| previous != value)
            {
                return Ok(false);
            }
        }
        for (id, value) in &source.entries {
            self.entries
                .entry(remap.map(*id)?)
                .or_insert_with(|| value.clone());
        }
        Ok(true)
    }
}

impl<T: DefinitionRecord<DefinitionId, Rebind<DefinitionId> = T> + Clone> DefinitionMap<T> {
    /// Import both keys and every value reference once. Key-only import is not
    /// sufficient for metadata whose binders and nominal members are scoped IDs.
    pub fn import_records(
        &self,
        context: &DefinitionContext,
        cancel: &CancellationToken,
    ) -> Result<Self, DefinitionMappingError> {
        cancel
            .check()
            .map_err(|_| DefinitionMappingError::Cancelled)?;
        let definitions = self.context.snapshot();
        let mut references = self.ids().collect::<HashSet<_>>();
        for record in self.values() {
            record.visit_definitions(
                &mut |id| {
                    references.insert(*id);
                    Ok(())
                },
                cancel,
            )?;
        }
        let remap = context.import(&definitions, references)?;
        let entries = self
            .entries
            .iter()
            .map(|(id, value)| {
                let value = value.map_identities(&mut DefinitionMapper::new(
                    &mut |id| remap.map(*id).map_err(Into::into),
                    cancel,
                ))?;
                Ok((remap.map(*id)?, value))
            })
            .collect::<Result<_, DefinitionMappingError>>()?;
        Ok(Self {
            context: context.clone(),
            entries,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host_interface::value_type::HostValueType;
    use crate::identity::{DefinitionKind, DefinitionPathSegment, ModuleIdentity};

    fn path(name: &str) -> DefinitionPath {
        DefinitionPath {
            module: ModuleIdentity::single_file("index.kgr"),
            path: vec![DefinitionPathSegment {
                kind: DefinitionKind::Function,
                name: name.into(),
                occurrence: 0,
            }],
        }
    }

    #[test]
    fn independent_indexes_import_exact_identity_and_reject_foreign_handles() {
        let mut first = DefinitionMap::default();
        first.insert(path("run"), 10).unwrap();
        let first_id = first.ids().next().unwrap();
        let mut second = DefinitionMap::default();
        second.insert(path("other"), 20).unwrap();
        assert!(second.get_id(first_id).is_none());
        assert!(second.merge(&first).unwrap());
        assert_eq!(second.get(&path("run")), Some(&10));
        assert!(second.get_id(first_id).is_none());
        assert_eq!(
            second.keys().collect::<Vec<_>>(),
            vec![path("other"), path("run")]
        );
        first.insert(path("other"), 99).unwrap();
        first.insert(path("rejected"), 30).unwrap();
        assert!(!second.merge(&first).unwrap());
        assert_eq!(second.get(&path("other")), Some(&20));
        assert!(second.get(&path("rejected")).is_none());
    }

    #[test]
    fn branches_share_one_writer_and_retained_snapshots_keep_a_checked_prefix() {
        let mut first = DefinitionMap::default();
        first.insert(path("run"), 10).unwrap();
        let snapshot = first.context().snapshot();
        let mut branch = first.clone();
        branch.insert(path("next"), 20).unwrap();
        first.insert(path("different"), 30).unwrap();
        let next = branch.context.lookup(&path("next")).unwrap();
        let different = first.context.lookup(&path("different")).unwrap();
        assert_ne!(next, different);
        assert!(snapshot.resolve(next).is_err());
        assert_eq!(
            branch
                .context
                .snapshot()
                .resolve(different)
                .unwrap()
                .to_path(),
            path("different")
        );
        assert!(first.get_id(next).is_none());
        assert!(branch.get_id(different).is_none());
        drop(first);
        assert_eq!(branch.get_id(next), Some(&20));
    }

    #[test]
    fn record_import_remaps_values_and_rejects_foreign_references_and_cancellation() {
        let mut source = DefinitionMap::default();
        let original = source.context().intern(&path("Player")).unwrap();
        source
            .insert(
                path("run"),
                HostValueType::Tuple(vec![
                    HostValueType::Opaque(original),
                    HostValueType::Opaque(original),
                ]),
            )
            .unwrap();
        let target = DefinitionContext::new().unwrap();
        target.intern(&path("unrelated")).unwrap();
        let cancel = CancellationToken::default();
        let imported = source.import_records(&target, &cancel).unwrap();
        let mapped = target.lookup(&path("Player")).unwrap();
        assert_ne!(original, mapped);
        assert_eq!(
            imported.get(&path("run")),
            Some(&HostValueType::Tuple(vec![
                HostValueType::Opaque(mapped),
                HostValueType::Opaque(mapped),
            ]))
        );
        assert!(imported.get_id(source.ids().next().unwrap()).is_none());
        assert_eq!(
            target.snapshot().resolve(mapped).unwrap().to_path(),
            path("Player")
        );
        source
            .insert(path("invalid"), HostValueType::Opaque(mapped))
            .unwrap();
        assert!(matches!(
            source.import_records(&target, &cancel),
            Err(DefinitionMappingError::Identity(
                DefinitionTableError::ForeignTable
            ))
        ));
        cancel.cancel();
        let empty: DefinitionMap<HostValueType<DefinitionId>> = DefinitionMap::default();
        assert!(matches!(
            empty.import_records(&target, &cancel),
            Err(DefinitionMappingError::Cancelled)
        ));
    }
}
