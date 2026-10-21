//! Explicit contextual conversion of identity-bearing metadata records.
use crate::{
    cancellation::CancellationToken,
    identity::table::{DefinitionId, DefinitionTable, DefinitionTableError, module::ModuleHandle},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fmt,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefinitionMappingError {
    Identity(DefinitionTableError),
    Cancelled,
    LimitExceeded,
    InvalidContract,
}

impl fmt::Display for DefinitionMappingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Identity(error) => error.fmt(f),
            Self::Cancelled => f.write_str("identity conversion cancelled"),
            Self::LimitExceeded => f.write_str("identity metadata limit exceeded"),
            Self::InvalidContract => f.write_str("invalid identity metadata contract"),
        }
    }
}

impl Error for DefinitionMappingError {}

impl From<DefinitionTableError> for DefinitionMappingError {
    fn from(error: DefinitionTableError) -> Self {
        Self::Identity(error)
    }
}

/// An ID-to-ID mapping receives its resolver explicitly. The converter checks
/// source ownership and target scope; each reference observes cancellation.
/// Paths and file references are not semantic mapping targets, and no global
/// pool, serialization state or thread-local context supplies identity.
pub struct DefinitionMapper<'a> {
    convert: &'a mut dyn FnMut(&DefinitionId) -> Result<DefinitionId, DefinitionMappingError>,
    target: &'a DefinitionTable,
    cancel: &'a CancellationToken,
}

impl<'a> DefinitionMapper<'a> {
    pub fn new(
        convert: &'a mut dyn FnMut(&DefinitionId) -> Result<DefinitionId, DefinitionMappingError>,
        target: &'a DefinitionTable,
        cancel: &'a CancellationToken,
    ) -> Self {
        Self {
            convert,
            target,
            cancel,
        }
    }

    pub fn cancellation(&self) -> &CancellationToken {
        self.cancel
    }

    pub fn check(&self) -> Result<(), DefinitionMappingError> {
        check_cancel(self.cancel)
    }

    pub fn reference(
        &mut self,
        source: &DefinitionId,
    ) -> Result<DefinitionId, DefinitionMappingError> {
        self.check()?;
        (self.convert)(source)
    }

    /// Maps a module identity and checks that the destination is still a module root.
    pub fn module(&mut self, source: ModuleHandle) -> Result<ModuleHandle, DefinitionMappingError> {
        let id = self.reference(&source.definition())?;
        Ok(self.target.module_handle(id)?)
    }
}

pub fn check_cancel(cancel: &CancellationToken) -> Result<(), DefinitionMappingError> {
    cancel
        .check()
        .map_err(|_| DefinitionMappingError::Cancelled)
}

/// The envelope bounds total input bytes; this bounds individual in-memory
/// collections before a conversion allocates their replacement.
pub fn map_sequence<T, U>(
    source: &[T],
    convert: impl FnMut(&T) -> Result<U, DefinitionMappingError>,
) -> Result<Vec<U>, DefinitionMappingError> {
    if source.len() > 1_000_000 {
        return Err(DefinitionMappingError::LimitExceeded);
    }
    source.iter().map(convert).collect()
}

pub fn map_entries<K: Ord, V>(
    source_len: usize,
    entries: impl Iterator<Item = Result<(K, V), DefinitionMappingError>>,
) -> Result<BTreeMap<K, V>, DefinitionMappingError> {
    if source_len > 1_000_000 {
        return Err(DefinitionMappingError::LimitExceeded);
    }
    let mut mapped = BTreeMap::new();
    for entry in entries {
        let (key, value) = entry?;
        if mapped.insert(key, value).is_some() {
            return Err(DefinitionMappingError::InvalidContract);
        }
    }
    Ok(mapped)
}

pub fn map_set<K: Ord>(
    source_len: usize,
    entries: impl Iterator<Item = Result<K, DefinitionMappingError>>,
) -> Result<BTreeSet<K>, DefinitionMappingError> {
    if source_len > 1_000_000 {
        return Err(DefinitionMappingError::LimitExceeded);
    }
    let mut mapped = BTreeSet::new();
    for entry in entries {
        if !mapped.insert(entry?) {
            return Err(DefinitionMappingError::InvalidContract);
        }
    }
    Ok(mapped)
}
