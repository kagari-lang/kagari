//! Metadata-only correspondence between exact publications of one installation.
use crate::{
    error::{RuntimeError, RuntimeErrorKind},
    module::{
        Installation, VerifiedProgram,
        dispatch::{PublicationToken, invalid},
    },
    reload::updates::headers,
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{DefinitionMapper, DefinitionMappingError},
        metadata::DefinitionMetadata,
        scoped::{DefinitionReferences, RemapDefinitions},
        table::{DefinitionId, DefinitionTable},
    },
};
use kagari_contract::{
    decode_limits::MAX_TABLE_RECORDS,
    types::named::{NamedDeclaration, correspondence::DeclarationCorrespondence},
};
use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

/// A non-reused installation-local identity, including named declaration binders.
/// Its number is not a portable ID or authority to call a private declaration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StableDeclarationId(u64);

/// Cumulative correspondence names and impl headers, never executable payloads.
#[derive(Debug)]
pub(super) struct DeclarationIndex {
    records: DefinitionMetadata<References>,
    stable: Vec<StableDeclarationId>,
    headers: Vec<NamedDeclaration>,
}

#[derive(Debug)]
struct References(Vec<DefinitionId>);

impl References {
    fn for_program(program: &VerifiedProgram) -> Result<Self, RuntimeError> {
        let mut ids = HashSet::new();
        // A child reference also identifies its declaration/binder ancestors.
        // Do not import unrelated prefixes from the runtime-wide identity context.
        for id in program.definition_references.iter().copied() {
            let mut next = Some(id);
            while let Some(id) = next {
                if !ids.insert(id) {
                    break;
                }
                if ids.len() > MAX_TABLE_RECORDS {
                    return Err(RuntimeError::resource_limit(
                        "publication declaration index",
                    ));
                }
                next = program
                    .definitions()
                    .parent(id)
                    .map_err(|error| invalid(error.to_string()))?;
            }
        }
        let mut ids: Vec<_> = ids.into_iter().collect();
        ids.sort_unstable();
        Ok(Self(ids))
    }
}

impl DefinitionReferences for References {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        for id in &self.0 {
            cancel
                .check()
                .map_err(|_| DefinitionMappingError::Cancelled)?;
            visit(id)?;
        }
        Ok(())
    }
}

impl RemapDefinitions for References {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        self.0
            .iter()
            .map(|id| mapper.reference(id))
            .collect::<Result<Vec<_>, _>>()
            .map(Self)
    }
}

/// Original-to-stable references for one publication. Clones retain only metadata.
/// Keep this scope beside types/witnesses copied from that publication: the runtime's
/// shared DefinitionTable alone cannot distinguish reordered semantic impl owners.
#[derive(Debug)]
pub struct PublicationDefinitions {
    pub(super) token: PublicationToken,
    published: AtomicBool,
    original: DefinitionTable,
    canonical: DefinitionTable,
    stable: HashMap<DefinitionId, StableDeclarationId>,
    canonical_ids: HashMap<StableDeclarationId, DefinitionId>,
    originals: HashMap<StableDeclarationId, Vec<DefinitionId>>,
}

impl PublicationDefinitions {
    pub(in crate::module) fn mark_published(&self) {
        self.published.store(true, Ordering::Release);
    }

    pub(super) fn is_published(&self) -> bool {
        self.published.load(Ordering::Acquire)
    }

    pub fn publication_token(&self) -> PublicationToken {
        self.token
    }

    pub fn definitions(&self) -> &DefinitionTable {
        &self.original
    }

    pub fn stable_declaration(
        &self,
        id: DefinitionId,
    ) -> Result<StableDeclarationId, RuntimeError> {
        self.stable
            .get(&id)
            .copied()
            .ok_or_else(|| invalid("unmapped publication declaration"))
    }

    pub(super) fn same_installation(&self, other: &Self) -> bool {
        self.token.registry == other.token.registry
            && self.token.installation == other.token.installation
    }

    pub(super) fn canonicalize<T: RemapDefinitions>(
        &self,
        source: &Self,
        records: &T,
        cancel: &CancellationToken,
    ) -> Result<T, RuntimeError> {
        if !self.same_installation(source) {
            return Err(invalid("foreign declaration correspondence"));
        }
        records
            .remap_definitions(&mut DefinitionMapper::new(
                &mut |id| {
                    let stable = source
                        .stable
                        .get(id)
                        .ok_or(DefinitionMappingError::InvalidContract)?;
                    self.canonical_ids
                        .get(stable)
                        .copied()
                        .ok_or(DefinitionMappingError::InvalidContract)
                },
                &self.canonical,
                cancel,
            ))
            .map_err(mapping_error)
    }

    pub(super) fn original_for(
        &self,
        stable: StableDeclarationId,
        preferred: &HashMap<StableDeclarationId, DefinitionId>,
    ) -> Result<DefinitionId, DefinitionMappingError> {
        if let Some(id) = preferred.get(&stable) {
            return Ok(*id);
        }
        match self.originals.get(&stable).map(Vec::as_slice) {
            Some([id]) => Ok(*id),
            _ => Err(DefinitionMappingError::InvalidContract),
        }
    }
}

/// Checked ID-to-ID transport, not type inference or replacement implementation search.
/// Equivalent inherent impl blocks can have multiple original owner IDs. A selected
/// target supplies its declaration ancestors and binder owners; unresolved ambiguity
/// is an error, never a source-order choice.
#[derive(Debug, Clone)]
pub struct PublicationMapping {
    source: Arc<PublicationDefinitions>,
    target: Arc<PublicationDefinitions>,
    preferred: HashMap<StableDeclarationId, DefinitionId>,
}

impl PublicationMapping {
    pub(super) fn new(
        source: Arc<PublicationDefinitions>,
        target: Arc<PublicationDefinitions>,
        preferred: impl IntoIterator<Item = DefinitionId>,
    ) -> Result<Self, RuntimeError> {
        if !source.same_installation(&target) {
            return Err(invalid("foreign publication mapping"));
        }
        let mut chosen = HashMap::new();
        for id in preferred {
            let stable = target.stable_declaration(id)?;
            if chosen.insert(stable, id).is_some_and(|old| old != id) {
                return Err(invalid("ambiguous selected binder correspondence"));
            }
        }
        Ok(Self {
            source,
            target,
            preferred: chosen,
        })
    }

    pub fn source(&self) -> &PublicationDefinitions {
        &self.source
    }

    pub fn target(&self) -> &PublicationDefinitions {
        &self.target
    }

    pub fn map(&self, id: DefinitionId) -> Result<DefinitionId, RuntimeError> {
        let stable = self.source.stable_declaration(id)?;
        // Exact scope transport must not collapse equivalent, distinct impl blocks.
        if self.source.token == self.target.token {
            self.target
                .original
                .resolve(id)
                .map_err(|error| invalid(error.to_string()))?;
            return Ok(id);
        }
        self.target
            .original_for(stable, &self.preferred)
            .map_err(mapping_error)
    }

    /// Rebuild types, signatures or witnesses in the selected publication's original
    /// scope. Missing/ambiguous references and cancellation fail without inference.
    pub fn remap<T: RemapDefinitions>(
        &self,
        records: &T,
        cancel: &CancellationToken,
    ) -> Result<DefinitionMetadata<T>, RuntimeError> {
        let records = records
            .remap_definitions(&mut DefinitionMapper::new(
                &mut |id| {
                    self.map(*id)
                        .map_err(|_| DefinitionMappingError::InvalidContract)
                },
                &self.target.original,
                cancel,
            ))
            .map_err(mapping_error)?;
        DefinitionMetadata::checked(self.target.original.clone(), records, cancel)
            .map_err(mapping_error)
    }
}

impl DeclarationIndex {
    pub(super) fn prepare(
        installation: &Installation,
        token: PublicationToken,
        previous: Option<&Self>,
        program: &VerifiedProgram,
    ) -> Result<(Self, Arc<PublicationDefinitions>), RuntimeError> {
        let cancel = CancellationToken::default();
        let mut correspondence = DeclarationCorrespondence::new().map_err(mapping_error)?;
        let mut known = HashMap::new();
        let mut retained_headers = Vec::new();
        if let Some(previous) = previous {
            let imported = correspondence
                .import(
                    previous.records.records(),
                    previous.headers.iter().cloned(),
                    previous.records.definitions(),
                    &cancel,
                )
                .map_err(mapping_error)?;
            known.extend(
                imported
                    .records()
                    .0
                    .iter()
                    .copied()
                    .zip(previous.stable.iter().copied()),
            );
            retained_headers = correspondence
                .import(
                    &previous.headers,
                    previous.headers.iter().cloned(),
                    previous.records.definitions(),
                    &cancel,
                )
                .map_err(mapping_error)?
                .into_records();
        }
        let headers = headers(program);
        let references = References::for_program(program)?;
        let imported = correspondence
            .import(
                &references,
                headers.iter().cloned(),
                program.definitions(),
                &cancel,
            )
            .map_err(mapping_error)?;
        let imported_headers = correspondence
            .import(
                &headers,
                headers.iter().cloned(),
                program.definitions(),
                &cancel,
            )
            .map_err(mapping_error)?;
        let canonical = imported_headers.definitions().clone();
        let mut stable = HashMap::new();
        let mut canonical_ids = HashMap::new();
        let mut originals: HashMap<StableDeclarationId, Vec<DefinitionId>> = HashMap::new();
        for (original, canonical) in references.0.into_iter().zip(imported.into_records().0) {
            let identity = match known.get(&canonical) {
                Some(identity) => *identity,
                None => {
                    if known.len() >= MAX_TABLE_RECORDS {
                        return Err(RuntimeError::resource_limit(
                            "installation declaration index",
                        ));
                    }
                    let identity = StableDeclarationId(installation.reserve_declaration()?);
                    known.insert(canonical, identity);
                    identity
                }
            };
            stable.insert(original, identity);
            canonical_ids.insert(identity, canonical);
            originals.entry(identity).or_default().push(original);
        }
        retained_headers.extend(imported_headers.into_records());
        let mut header_index = HashMap::new();
        for header in retained_headers {
            if let Some(previous) = header_index.insert(header.declaration, header.clone())
                && previous != header
            {
                return Err(invalid("changed stable implementation header"));
            }
        }
        // Stable-ID ordering makes rebuilding the metadata index deterministic; IDs
        // reserved by abandoned candidates are holes, never reassigned declarations.
        let mut entries: Vec<_> = known.into_iter().collect();
        entries.sort_by_key(|(_, stable)| stable.0);
        let (records, identities): (Vec<_>, Vec<_>) = entries.into_iter().unzip();
        let mut headers: Vec<_> = header_index.into_values().collect();
        headers.sort_by_key(|header| header.declaration);
        let index = Self {
            records: DefinitionMetadata::checked(canonical.clone(), References(records), &cancel)
                .map_err(mapping_error)?,
            stable: identities,
            headers,
        };
        let scope = PublicationDefinitions {
            token,
            published: AtomicBool::new(false),
            original: program.definitions().clone(),
            canonical,
            stable,
            canonical_ids,
            originals,
        };
        Ok((index, Arc::new(scope)))
    }
}

pub(super) fn mapping_error(error: DefinitionMappingError) -> RuntimeError {
    match error {
        DefinitionMappingError::Cancelled => RuntimeError::new(
            RuntimeErrorKind::Cancelled,
            "declaration correspondence cancelled",
        ),
        DefinitionMappingError::LimitExceeded => {
            RuntimeError::resource_limit("declaration correspondence")
        }
        error => invalid(format!("declaration correspondence: {error}")),
    }
}
