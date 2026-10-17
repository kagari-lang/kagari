//! Checked rebuilding of identity-bearing analysis indexes.
use kagari_common::identity::mapping::DefinitionMappingError;
use std::{collections::HashMap, hash::Hash};

pub(crate) fn map_hash_entries<K: Eq + Hash, V>(
    count: usize,
    entries: impl IntoIterator<Item = Result<(K, V), DefinitionMappingError>>,
) -> Result<HashMap<K, V>, DefinitionMappingError> {
    if count > 1_000_000 {
        return Err(DefinitionMappingError::LimitExceeded);
    }
    let mut result = HashMap::with_capacity(count);
    for entry in entries {
        let (key, value) = entry?;
        if result.insert(key, value).is_some() {
            return Err(DefinitionMappingError::InvalidContract);
        }
    }
    Ok(result)
}
