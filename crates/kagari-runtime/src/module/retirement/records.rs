//! Detach metadata after managed retirement, then dispose outside all store loans.
use crate::module::{ModuleKey, ModuleStore, state::cells::CellId};
use kagari_gc::error::GcError;
use std::{
    collections::HashSet,
    panic::{AssertUnwindSafe, catch_unwind},
};

impl ModuleStore {
    /// Return exact detached member keys, not a synthetic heap reachability count.
    /// Failed preparation restores notices using their existing release credits.
    pub(crate) fn dispose_retired(&self, scan_abandoned: bool) -> Result<Vec<ModuleKey>, GcError> {
        let batch = self.retirements.take()?;
        if batch.keys.is_empty() && !scan_abandoned {
            batch.commit();
            return Ok(Vec::new());
        }
        let mut records = self
            .inner
            .try_borrow_mut()
            .map_err(|_| GcError::Storage("module records borrowed during managed retirement"))?;
        let current = self
            .current
            .try_borrow()
            .map_err(|_| GcError::Storage("publications borrowed during managed retirement"))?;
        let mut programs = HashSet::new();
        programs.try_reserve(batch.keys.len().saturating_add(if scan_abandoned {
            records.staged.len()
        } else {
            0
        }))?;
        programs.extend(batch.keys.iter().copied());
        if scan_abandoned {
            // Before a release ticket is admitted, failed metadata preparation
            // has no managed owner. Its dead admission lease is authoritative.
            // Managed revisions always wait for the actual retirement notice.
            for (key, lease) in &records.staged {
                if lease.strong_count() == 0
                    && records
                        .records
                        .get(key)
                        .is_some_and(|record| !record.managed_owner)
                {
                    programs.insert(*key);
                }
            }
        }
        for key in &programs {
            if let Some(record) = records.records.get(key)
                && let Some(publication) = current.get(&record.module.installation_id())
            {
                let root = &publication.program.modules[publication.program.root.index()];
                if root.id == key.id && root.epoch == key.epoch {
                    return Err(GcError::Storage("published revision was retired"));
                }
            }
        }
        let count = records
            .records
            .values()
            .filter(|record| programs.contains(&record.module.program_key()))
            .count();
        let mut keys = Vec::new();
        keys.try_reserve_exact(count)?;
        keys.extend(records.records.iter().filter_map(|(key, record)| {
            programs
                .contains(&record.module.program_key())
                .then_some(*key)
        }));
        let mut retained_cells = HashSet::<CellId>::new();
        retained_cells.try_reserve(records.cells.len())?;
        for record in records
            .records
            .values()
            .filter(|record| !programs.contains(&record.module.program_key()))
        {
            retained_cells.extend(record.bindings.iter().copied());
        }
        let mut cell_keys = Vec::new();
        cell_keys.try_reserve_exact(records.cells.len().saturating_sub(retained_cells.len()))?;
        cell_keys.extend(
            records
                .cells
                .keys()
                .filter(|key| !retained_cells.contains(key))
                .copied(),
        );
        let mut retired = Vec::new();
        retired.try_reserve_exact(keys.len())?;
        let mut cells = Vec::new();
        cells.try_reserve_exact(cell_keys.len())?;
        // All allocation/admission above precedes this no-fail detachment.
        for key in &keys {
            records.retentions.remove(key);
            records.staged.remove(key);
            retired.push(
                records
                    .records
                    .remove(key)
                    .expect("prepared retired record"),
            );
        }
        for key in cell_keys {
            cells.push(records.cells.remove(&key).expect("prepared retired cell"));
        }
        batch.commit();
        drop(current);
        drop(records);
        let mut panicked = false;
        for record in retired {
            panicked |= catch_unwind(AssertUnwindSafe(|| drop(record))).is_err();
        }
        for cell in cells {
            panicked |= catch_unwind(AssertUnwindSafe(|| drop(cell))).is_err();
        }
        if panicked {
            return Err(GcError::Provider("retired program destructor panicked"));
        }
        Ok(keys)
    }
}
