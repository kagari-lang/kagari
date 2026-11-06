use crate::{error::GcError, table::Table};
use std::collections::BTreeMap;

#[test]
fn radix_growth_holes_and_reuse_preserve_ids_across_multiple_levels() -> Result<(), GcError> {
    let mut table = Table::default();
    let mut expected = BTreeMap::new();
    for value in 0..40000 {
        let id = table.prepare()?;
        table.insert(id, value);
        assert_eq!(expected.insert(id, value), None);
    }
    let high = *expected.last_key_value().unwrap().0;
    assert_eq!(table.get(u32::MAX), None);
    assert_eq!(table.remove(u32::MAX), None);
    for id in (0..high).rev().step_by(3) {
        assert_eq!(table.remove(id), expected.remove(&id));
        assert_eq!(table.remove(id), None);
    }
    for value in 40000..50000 {
        let id = table.prepare()?;
        table.insert(id, value);
        assert_eq!(expected.insert(id, value), None);
        *table.get_mut(id).unwrap() += 1;
        *expected.get_mut(&id).unwrap() += 1;
    }
    for (&id, value) in &expected {
        assert_eq!(table.get(id), Some(value));
    }
    for (&id, &value) in &expected {
        if id != high {
            assert_eq!(table.remove(id), Some(value));
        }
    }
    assert_eq!(table.len(), 1);
    assert_eq!(table.get(high), expected.get(&high));
    assert!(table.storage().0 <= 256);
    let reused = table.prepare()?;
    assert!(reused < high);
    table.insert(reused, -1);
    assert_eq!(table.remove(reused), Some(-1));
    assert_eq!(table.remove(high), expected.remove(&high));
    assert_eq!(table.storage(), (0, 0));
    // A cancelled admission can release its small prepared buffer at maintenance.
    table.prepare()?;
    table.trim();
    assert_eq!(table.storage(), (0, 0));
    Ok(())
}

// Construct one sparse high-ID path without allocating billions of entries.
fn sparse_last(shift: u32) -> Result<Table<u32>, GcError> {
    if shift == 0 {
        let mut table = Table::default();
        table.slots.resize(256, None);
        table.insert(255, u32::MAX);
        return Ok(table);
    }
    let mut table = Table::branch(shift)?;
    table.children[63] = Some(Box::new(sparse_last(if shift == 8 {
        0
    } else {
        shift - 6
    })?));
    table.len = 1;
    Ok(table)
}

#[test]
fn full_width_identity_has_no_truncation_or_alias_with_low_ids() -> Result<(), GcError> {
    let mut table = sparse_last(26)?;
    assert_eq!(table.get(u32::MAX), Some(&u32::MAX));
    assert_eq!(table.get(u32::MAX - 256), None);
    assert_eq!(table.get(255), None);
    let id = table.prepare()?;
    assert_eq!(id, 0);
    table.insert(id, 7);
    assert_eq!(table.get(0), Some(&7));
    assert_eq!(table.remove(u32::MAX), Some(u32::MAX));
    table.trim();
    assert_eq!(table.get(0), Some(&7));
    assert!(table.storage().1 < 1024);
    assert_eq!(table.remove(0), Some(7));
    assert_eq!(table.storage(), (0, 0));
    Ok(())
}
