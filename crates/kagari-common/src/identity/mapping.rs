//! Explicit contextual conversion of identity-bearing metadata records.
use crate::{
    cancellation::CancellationToken,
    identity::{reference::DefinitionReference, table::DefinitionTableError},
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

/// Each conversion receives its resolver explicitly. A numeric reference is never
/// resolved through a global pool, serialization state or thread-local context.
pub struct DefinitionMapper<'a, I, J> {
    convert: &'a mut dyn FnMut(&I) -> Result<J, DefinitionMappingError>,
    cancel: &'a CancellationToken,
}

impl<'a, I: DefinitionReference, J: DefinitionReference> DefinitionMapper<'a, I, J> {
    pub fn new(
        convert: &'a mut dyn FnMut(&I) -> Result<J, DefinitionMappingError>,
        cancel: &'a CancellationToken,
    ) -> Self {
        Self { convert, cancel }
    }

    pub fn cancellation(&self) -> &CancellationToken {
        self.cancel
    }

    pub fn check(&self) -> Result<(), DefinitionMappingError> {
        check_cancel(self.cancel)
    }

    pub fn reference(&mut self, source: &I) -> Result<J, DefinitionMappingError> {
        self.check()?;
        if !source.within_path_limit() {
            return Err(DefinitionMappingError::LimitExceeded);
        }
        (self.convert)(source)
    }
}

pub trait DefinitionRecord<I: DefinitionReference> {
    type Rebind<J: DefinitionReference>;

    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError>;

    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError>;
}

impl<I: DefinitionReference, T: DefinitionRecord<I>> DefinitionRecord<I> for Vec<T> {
    type Rebind<J: DefinitionReference> = Vec<T::Rebind<J>>;

    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        map_sequence(self, |record| record.map_identities(mapper))
    }

    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        for record in self {
            record.visit_definitions(visit, cancel)?;
        }
        Ok(())
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
