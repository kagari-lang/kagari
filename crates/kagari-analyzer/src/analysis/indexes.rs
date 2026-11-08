//! Bounded signature relation shards with exact context and source provenance.
pub(super) mod cache;

#[cfg(test)]
mod tests;

use crate::workspace::{
    ContextCapture, ContextKey, WorkspaceCapture,
    budget::{InputBudget, InputLease},
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{DefinitionMapper, DefinitionMappingError},
        scoped::RemapDefinitions,
        table::DefinitionId,
    },
};
use kagari_hir::{
    analysis::signatures::{
        SignatureSnapshot,
        relations::{
            ImplementationLocation, ImplementationRelations, ImplementationTarget, RelationError,
            RelationLimits,
        },
    },
    declarations::DeclarationId,
    typeck::GenericBounds,
    types::TypeId,
};
use std::{collections::BTreeMap, fmt, iter, sync::Arc};

/// Explicit search coverage failure, never an exhaustive empty answer.
#[derive(Debug)]
pub(crate) enum IndexFailure {
    Cancelled,
    Capacity(String),
    Incomplete(String),
}

impl fmt::Display for IndexFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled => formatter.write_str("implementation index cancelled"),
            Self::Capacity(message) | Self::Incomplete(message) => message.fmt(formatter),
        }
    }
}

impl From<RelationError> for IndexFailure {
    fn from(error: RelationError) -> Self {
        match error {
            RelationError::Cancelled => Self::Cancelled,
            RelationError::LimitExceeded => Self::Capacity(error.to_string()),
            _ => Self::Incomplete(error.to_string()),
        }
    }
}

struct RelationShard {
    relations: ImplementationRelations,
    _lease: InputLease,
}

/// Navigation destination interpreted with the context and source that own it.
pub(crate) struct IndexedLocation {
    pub context: ContextKey,
    pub location: ImplementationLocation,
}

/// One immutable admitted scope, filled by at most one context per worker turn.
/// A consumer retains this owner while later session generations are acquired.
pub(crate) struct ImplementationIndex {
    capture: Arc<WorkspaceCapture>,
    environment_epoch: u64,
    limits: RelationLimits,
    shards: BTreeMap<ContextKey, RelationShard>,
    row_bytes: usize,
}

impl ImplementationIndex {
    pub fn new(
        capture: Arc<WorkspaceCapture>,
        environment_epoch: u64,
        limits: RelationLimits,
    ) -> Result<Self, IndexFailure> {
        if !capture.errors.is_empty()
            || capture
                .contexts
                .iter()
                .any(|context| !context.discovery_complete || !context.semantic_available)
        {
            return Err(IndexFailure::Incomplete(
                "implementation search scope has unavailable declared modules or package inputs"
                    .into(),
            ));
        }
        Ok(Self {
            capture,
            environment_epoch,
            limits,
            shards: BTreeMap::new(),
            row_bytes: 0,
        })
    }

    pub fn generation(&self) -> u64 {
        self.capture.generation
    }

    pub fn environment_epoch(&self) -> u64 {
        self.environment_epoch
    }

    pub fn next_context(&self) -> Option<&ContextCapture> {
        self.capture
            .contexts
            .iter()
            .find(|context| !self.shards.contains_key(&context.key))
    }

    /// Atomically adds one complete context shard; failures retain earlier shards.
    pub fn insert(
        &mut self,
        context: &ContextKey,
        signatures: SignatureSnapshot,
        budget: &InputBudget,
        available_bytes: usize,
        cancel: &CancellationToken,
    ) -> Result<(), IndexFailure> {
        check(cancel)?;
        if self.shards.contains_key(context)
            || !self
                .capture
                .contexts
                .iter()
                .any(|candidate| &candidate.key == context)
        {
            return Err(IndexFailure::Incomplete(
                "implementation shard is duplicated or outside the admitted scope".into(),
            ));
        }
        let mut limits = self.limits;
        limits.bytes = limits
            .bytes
            .saturating_sub(self.row_bytes)
            .min(available_bytes);
        let relations = signatures.implementation_relations(limits, cancel)?;
        if self
            .implementation_count()
            .saturating_add(relations.implementation_count())
            > self.limits.implementations
        {
            return Err(IndexFailure::Capacity(
                "implementation declaration limit exceeded across contexts".into(),
            ));
        }
        let sources = signatures.declaration_snapshot().files().flat_map(|file| {
            iter::once(file.source())
                .chain(file.source().copied_sources())
                .cloned()
                .map(Arc::new)
        });
        let lease = budget
            .retain(sources)
            .map_err(|error| IndexFailure::Capacity(error.to_string()))?;
        check(cancel)?;
        self.row_bytes += relations.retained_bytes();
        self.shards.insert(
            context.clone(),
            RelationShard {
                relations,
                _lease: lease,
            },
        );
        Ok(())
    }

    pub fn retained_bytes(&self) -> usize {
        self.row_bytes
    }

    pub fn implementation_count(&self) -> usize {
        self.shards
            .values()
            .map(|shard| shard.relations.implementation_count())
            .sum()
    }

    pub fn signatures(&self, context: &ContextKey) -> Option<&SignatureSnapshot> {
        self.shards
            .get(context)
            .map(|shard| shard.relations.signatures())
    }

    /// Queries all required contexts only after every declaration shard is complete.
    pub fn query(
        &self,
        selected: &ContextKey,
        signatures: &SignatureSnapshot,
        target: &ImplementationTarget,
        cancel: &CancellationToken,
    ) -> Result<Vec<IndexedLocation>, IndexFailure> {
        check(cancel)?;
        if self.next_context().is_some() {
            return Err(IndexFailure::Incomplete(
                "implementation declaration coverage is not complete".into(),
            ));
        }
        let source_context = self
            .capture
            .contexts
            .iter()
            .find(|context| &context.key == selected)
            .ok_or_else(|| {
                IndexFailure::Incomplete(
                    "selected context is outside the admitted implementation scope".into(),
                )
            })?;
        let mut result = Vec::new();
        for context in &self.capture.contexts {
            check(cancel)?;
            let shard = &self.shards[&context.key];
            let target = if context.key == *selected
                && signatures.definitions().id() == shard.relations.signatures().definitions().id()
            {
                target.clone()
            } else {
                let Some(target) = map_target(
                    source_context,
                    signatures,
                    context,
                    shard.relations.signatures(),
                    target,
                    cancel,
                )?
                else {
                    continue;
                };
                target
            };
            for location in shard.relations.query(&target, cancel)? {
                if result.len() >= self.limits.results {
                    return Err(IndexFailure::Capacity(
                        "implementation result limit exceeded across contexts".into(),
                    ));
                }
                let source = shard
                    .relations
                    .signatures()
                    .declaration_snapshot()
                    .source(location.selection.file)
                    .ok_or_else(|| {
                        IndexFailure::Incomplete(
                            "implementation destination has no retained physical source".into(),
                        )
                    })?;
                if source.revision() != location.selection.revision
                    || location.extent.file != location.selection.file
                {
                    return Err(IndexFailure::Incomplete(
                        "implementation destination source provenance is inconsistent".into(),
                    ));
                }
                result.push(IndexedLocation {
                    context: context.key.clone(),
                    location,
                });
            }
        }
        check(cancel)?;
        Ok(result)
    }
}

fn map_target(
    source_context: &ContextCapture,
    source: &SignatureSnapshot,
    target_context: &ContextCapture,
    destination: &SignatureSnapshot,
    target: &ImplementationTarget,
    cancel: &CancellationToken,
) -> Result<Option<ImplementationTarget>, IndexFailure> {
    let receiver = target
        .receiver
        .as_ref()
        .and_then(receiver_anchor)
        .filter(|id| Some(*id) != target.declaration);
    for anchor in [target.declaration, receiver].into_iter().flatten() {
        let path = source
            .definitions()
            .resolve(anchor)
            .map_err(|error| IndexFailure::Incomplete(error.to_string()))?
            .to_path();
        let Some(mapped) = destination.definitions().lookup(&path) else {
            return Ok(None);
        };
        if !same_provider(
            source_context,
            source,
            anchor,
            target_context,
            destination,
            mapped,
        )? {
            return Ok(None);
        }
    }
    let mut mappings = BTreeMap::new();
    let mut mismatch = false;
    let mut missing = false;
    let mut convert = |id: &DefinitionId| {
        if let Some(mapped) = mappings.get(id) {
            return Ok(*mapped);
        }
        let path = source.definitions().resolve(*id)?.to_path();
        let Some(mapped) = destination.definitions().lookup(&path) else {
            missing = true;
            return Err(DefinitionMappingError::InvalidContract);
        };
        if !same_provenance(
            source_context,
            source,
            *id,
            target_context,
            destination,
            mapped,
        ) {
            mismatch = true;
            return Err(DefinitionMappingError::InvalidContract);
        }
        mappings.insert(*id, mapped);
        Ok(mapped)
    };
    let mapped = (|| {
        let mut mapper = DefinitionMapper::new(&mut convert, destination.definitions(), cancel);
        let declaration = target
            .declaration
            .map(|id| mapper.reference(&id))
            .transpose()?;
        let receiver = target
            .receiver
            .as_ref()
            .map(|receiver| receiver.remap_definitions(&mut mapper))
            .transpose()?;
        let mut assumptions = GenericBounds::new();
        for (ty, bounds) in &target.assumptions {
            mapper.check()?;
            let ty = ty.remap_definitions(&mut mapper)?;
            let bounds = bounds.remap_definitions(&mut mapper)?;
            if assumptions.insert(ty, bounds).is_some() {
                return Err(DefinitionMappingError::InvalidContract);
            }
        }
        Ok(ImplementationTarget {
            declaration,
            receiver,
            assumptions,
        })
    })();
    match mapped {
        Ok(target) => Ok(Some(target)),
        Err(DefinitionMappingError::Cancelled) => Err(IndexFailure::Cancelled),
        Err(_) if mismatch => Err(IndexFailure::Incomplete(
            "workspace contexts disagree about declaration source provenance".into(),
        )),
        Err(_) if missing => Err(IndexFailure::Incomplete(
            "implementation query bindings cannot be translated into a relevant workspace root"
                .into(),
        )),
        Err(error) => Err(IndexFailure::Incomplete(error.to_string())),
    }
}

fn receiver_anchor(receiver: &TypeId) -> Option<DefinitionId> {
    match receiver {
        TypeId::Struct(nominal)
        | TypeId::Enum(nominal)
        | TypeId::NativeObject(nominal)
        | TypeId::Trait(nominal) => Some(nominal.declaration),
        TypeId::Reference { target, .. } => receiver_anchor(target),
        _ => None,
    }
}

fn same_provenance(
    left_context: &ContextCapture,
    left: &SignatureSnapshot,
    left_id: DefinitionId,
    right_context: &ContextCapture,
    right: &SignatureSnapshot,
    right_id: DefinitionId,
) -> bool {
    let find = |signatures: &SignatureSnapshot, id| {
        signatures
            .declaration_snapshot()
            .declaration(&DeclarationId::Definition(id))
            .map(|declaration| declaration.location)
    };
    let (Some(left_site), Some(right_site)) = (find(left, left_id), find(right, right_id)) else {
        return false;
    };
    if left_site.range != right_site.range {
        return false;
    }
    let (Some(left_source), Some(right_source)) = (
        left.declaration_snapshot().source(left_site.file),
        right.declaration_snapshot().source(right_site.file),
    ) else {
        return false;
    };
    if left_source.text() != right_source.text()
        || left_source.module_identity() != right_source.module_identity()
    {
        return false;
    }
    match (
        left_context.origins.get(&left_site.file),
        right_context.origins.get(&right_site.file),
    ) {
        (Some(left), Some(right)) => {
            left.provider_key == right.provider_key && left.module == right.module
        }
        (None, None) => left_source.name() == right_source.name(),
        _ => false,
    }
}

/// Equal portable paths in independent physical copies are distinct anchors.
/// A shared provider still requires the stricter byte/module/site proof above.
fn same_provider(
    left_context: &ContextCapture,
    left: &SignatureSnapshot,
    left_id: DefinitionId,
    right_context: &ContextCapture,
    right: &SignatureSnapshot,
    right_id: DefinitionId,
) -> Result<bool, IndexFailure> {
    let find = |signatures: &SignatureSnapshot, id| {
        signatures
            .declaration_snapshot()
            .declaration(&DeclarationId::Definition(id))
            .map(|declaration| declaration.location)
    };
    let (Some(left_site), Some(right_site)) = (find(left, left_id), find(right, right_id)) else {
        return Err(IndexFailure::Incomplete(
            "implementation semantic anchor has no retained declaration provenance".into(),
        ));
    };
    match (
        left_context.origins.get(&left_site.file),
        right_context.origins.get(&right_site.file),
    ) {
        (Some(left), Some(right)) => Ok(left.provider_key == right.provider_key),
        (None, None) => {
            let left = left.source(left_site.file).ok_or_else(|| {
                IndexFailure::Incomplete("implementation anchor source is absent".into())
            })?;
            let right = right.source(right_site.file).ok_or_else(|| {
                IndexFailure::Incomplete("implementation anchor source is absent".into())
            })?;
            Ok(left.name() == right.name())
        }
        _ => Ok(false),
    }
}

fn check(cancel: &CancellationToken) -> Result<(), IndexFailure> {
    cancel.check().map_err(|_| IndexFailure::Cancelled)
}
