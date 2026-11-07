use super::{MAX_SCOPES, ReachabilityCache};
use crate::{imports::SourceDeclRef, resolver::table::NameTable};
use std::{collections::HashSet, sync::Arc};

#[test]
fn aggregate_capacity_rejects_memo_only_and_retired_owners_release_it() {
    let cache = ReachabilityCache::default();
    let first = Arc::new(NameTable::default());
    let second = Arc::new(NameTable::default());
    // Empty but reserved sets distinguish requested storage from initialized length.
    let large = Arc::new(HashSet::<SourceDeclRef>::with_capacity(40_000));
    let kept = cache.insert(&first, large.clone());
    assert!(Arc::ptr_eq(&kept, &large));
    assert!(Arc::ptr_eq(&cache.get(&first).unwrap(), &large));
    let complete = Arc::new(HashSet::<SourceDeclRef>::with_capacity(40_000));
    assert!(Arc::ptr_eq(
        &cache.insert(&second, complete.clone()),
        &complete
    ));
    assert!(cache.get(&second).is_none());
    assert!(cache.clone().get(&first).is_none());

    drop(first);
    cache.insert(&second, complete.clone());
    assert!(Arc::ptr_eq(&cache.get(&second).unwrap(), &complete));
    cache.clear();
    assert!(cache.get(&second).is_none());
    assert_eq!(Arc::strong_count(&complete), 1);
    assert!(
        Arc::ptr_eq(&kept, &large),
        "retirement does not alter an admitted result"
    );
}

#[test]
fn many_empty_scopes_are_bounded_and_dead_keys_do_not_block_admission() {
    let cache = ReachabilityCache::default();
    let sources = Arc::new(HashSet::new());
    let names = (0..MAX_SCOPES)
        .map(|_| Arc::new(NameTable::default()))
        .collect::<Vec<_>>();
    for scope in &names {
        cache.insert(scope, sources.clone());
        assert!(cache.get(scope).is_some());
    }
    let overflow = Arc::new(NameTable::default());
    let returned = cache.insert(&overflow, sources.clone());
    assert!(Arc::ptr_eq(&returned, &sources));
    assert!(cache.get(&overflow).is_none());
    drop(names);
    cache.insert(&overflow, sources.clone());
    assert!(cache.get(&overflow).is_some());
    assert_eq!(Arc::strong_count(&sources), 3);
}
