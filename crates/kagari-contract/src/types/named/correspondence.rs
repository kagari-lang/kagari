//! Checked-header correspondence across independent definition scopes.

use crate::{
    decode_limits::MAX_TABLE_RECORDS,
    types::named::{NamedContract, NamedDeclaration, wire::NamedDeclarationRecord},
};
use bincode::{DefaultOptions, Options};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionKind, DefinitionPath, ModuleIdentity,
        map::DefinitionContext,
        mapping::{DefinitionMapper, DefinitionMappingError, check_cancel},
        metadata::DefinitionMetadata,
        scoped::{DefinitionReferences, RemapDefinitions},
        table::{DefinitionId, DefinitionTable},
    },
};
use std::collections::HashMap;

/// Reject duplicate members after grouping equivalent semantic impl headers.
/// Different inherent blocks may share a header, but not a member identity. Trait
/// relationships may not be duplicated even when their headers are byte-identical.
pub fn validate_inventory<'a>(
    records: impl IntoIterator<Item = &'a NamedDeclaration>,
    definitions: &DefinitionTable,
    cancel: &CancellationToken,
) -> Result<(), DefinitionMappingError> {
    let mut inventory = Vec::new();
    let mut text_bytes = 0usize;
    for record in records {
        check_cancel(cancel)?;
        if inventory.len() >= MAX_TABLE_RECORDS {
            return Err(DefinitionMappingError::LimitExceeded);
        }
        text_bytes = text_bytes
            .checked_add(record.ambiguous_local_body.as_ref().map_or(0, String::len))
            .ok_or(DefinitionMappingError::LimitExceeded)?;
        if text_bytes > 64 * 1024 * 1024 {
            return Err(DefinitionMappingError::LimitExceeded);
        }
        inventory.push(record.clone());
    }
    let mapped = DeclarationCorrespondence::new()?.import(
        &inventory,
        inventory.iter().cloned(),
        definitions,
        cancel,
    )?;
    let mut seen = HashMap::new();
    for record in mapped.records() {
        check_cancel(cancel)?;
        if let Some(previous) = seen.insert(record.declaration, record) {
            let equivalent_inherent = matches!((&previous.contract, &record.contract), (NamedContract::Implementation(a), NamedContract::Implementation(b)) if a.trait_type.is_none() && b.trait_type.is_none() && a == b);
            if !equivalent_inherent {
                return Err(DefinitionMappingError::InvalidContract);
            }
        }
    }
    Ok(())
}

/// One comparison scope for prior and candidate authored contracts.
/// Equivalent impl headers share an owner after alpha-normalizing their binders.
/// Exact header bytes are keys; neither hashes nor source-order occurrences decide
/// correspondence. Ordinary declarations retain their authored parent/name paths.
#[derive(Debug)]
pub struct DeclarationCorrespondence {
    context: DefinitionContext,
    headers: HashMap<(ModuleIdentity, Vec<u8>), DefinitionId>,
    next_impl: HashMap<ModuleIdentity, u32>,
    remaining: usize,
    remaining_bytes: u64,
}

impl DeclarationCorrespondence {
    pub fn new() -> Result<Self, DefinitionMappingError> {
        Ok(Self {
            context: DefinitionContext::new()?,
            headers: HashMap::new(),
            next_impl: HashMap::new(),
            remaining: MAX_TABLE_RECORDS,
            remaining_bytes: 64 * 1024 * 1024,
        })
    }

    /// Import records using the complete installation's verified impl inventory.
    /// The returned scope is comparison metadata, not an executable publication.
    /// Missing impl correspondence, exhausted work, cancellation or malformed IDs
    /// fail; unrecognized impl occurrences are never paired as a fallback.
    pub fn import<T: RemapDefinitions>(
        &mut self,
        records: &T,
        inventory: impl IntoIterator<Item = NamedDeclaration>,
        definitions: &DefinitionTable,
        cancel: &CancellationToken,
    ) -> Result<DefinitionMetadata<T>, DefinitionMappingError> {
        let mut implementations = HashMap::new();
        for record in inventory {
            check_cancel(cancel)?;
            if !matches!(record.contract, NamedContract::Implementation(_)) {
                continue;
            }
            self.charge()?;
            let view = definitions.resolve(record.declaration)?;
            let module = view.module().clone();
            // A neutral owner is local to header comparison. Binder positions
            // and the checked Ty algebra remain unchanged under this remapping.
            let neutral = DefinitionContext::new()?;
            let root = neutral.intern_root(&module)?;
            let owner = neutral.intern_child(root, DefinitionKind::Impl, "", 0)?;
            let mut convert = |id: &DefinitionId| {
                self.charge()?;
                let path = definitions.resolve(*id)?.to_path();
                if definitions.contains(record.declaration, *id)? {
                    let mut parent = owner;
                    for segment in path.path.iter().skip(1) {
                        parent = neutral.intern_child(
                            parent,
                            segment.kind,
                            &segment.name,
                            segment.occurrence,
                        )?;
                    }
                    Ok(parent)
                } else if path
                    .path
                    .iter()
                    .any(|segment| segment.kind == DefinitionKind::Impl)
                {
                    Err(DefinitionMappingError::InvalidContract)
                } else {
                    Ok(neutral.intern(&path)?)
                }
            };
            // Register references before creating the immutable target prefix.
            record.visit_definition_ids(&mut |id| convert(id).map(|_| ()), cancel)?;
            let normalized = record.remap_definitions(&mut DefinitionMapper::new(
                &mut convert,
                &neutral.snapshot(),
                cancel,
            ))?;
            let table = neutral.snapshot();
            let mut references = Vec::new();
            normalized.visit_definition_ids(
                &mut |id| {
                    references.push(*id);
                    Ok(())
                },
                cancel,
            )?;
            let encoding = table.encode(references)?;
            let wire = NamedDeclarationRecord::encode(&normalized, &encoding, cancel)?;
            let encoded = (encoding.table, wire);
            let codec = DefaultOptions::new()
                .with_fixint_encoding()
                .with_little_endian();
            let size = codec
                .serialized_size(&encoded)
                .map_err(|_| DefinitionMappingError::InvalidContract)?;
            self.remaining_bytes = self
                .remaining_bytes
                .checked_sub(size)
                .ok_or(DefinitionMappingError::LimitExceeded)?;
            let bytes = codec
                .serialize(&encoded)
                .map_err(|_| DefinitionMappingError::InvalidContract)?;
            let key = (module.clone(), bytes);
            let target = if let Some(target) = self.headers.get(&key) {
                *target
            } else {
                let next = self.next_impl.entry(module.clone()).or_default();
                let root = self.context.intern_root(&module)?;
                let target = self
                    .context
                    .intern_child(root, DefinitionKind::Impl, "", *next)?;
                *next = next
                    .checked_add(1)
                    .ok_or(DefinitionMappingError::LimitExceeded)?;
                self.headers.insert(key, target);
                target
            };
            if implementations.insert(record.declaration, target).is_some() {
                return Err(DefinitionMappingError::InvalidContract);
            }
        }
        let mut mapped = HashMap::new();
        records.visit_definition_ids(
            &mut |id| {
                check_cancel(cancel)?;
                self.charge()?;
                if mapped.contains_key(id) {
                    return Ok(());
                }
                let path = definitions.resolve(*id)?.to_path();
                let target = if path
                    .path
                    .first()
                    .is_some_and(|segment| segment.kind == DefinitionKind::Impl)
                {
                    let owner_path = DefinitionPath {
                        module: path.module.clone(),
                        path: path.path[..1].to_vec(),
                    };
                    let owner = definitions
                        .lookup(&owner_path)
                        .ok_or(DefinitionMappingError::InvalidContract)?;
                    let mut parent = *implementations
                        .get(&owner)
                        .ok_or(DefinitionMappingError::InvalidContract)?;
                    for segment in path.path.iter().skip(1) {
                        parent = self.context.intern_child(
                            parent,
                            segment.kind,
                            &segment.name,
                            segment.occurrence,
                        )?;
                    }
                    parent
                } else {
                    self.context.intern(&path)?
                };
                mapped.insert(*id, target);
                Ok(())
            },
            cancel,
        )?;
        let table = self.context.snapshot();
        let records = records.remap_definitions(&mut DefinitionMapper::new(
            &mut |id| {
                mapped
                    .get(id)
                    .copied()
                    .ok_or(DefinitionMappingError::InvalidContract)
            },
            &table,
            cancel,
        ))?;
        DefinitionMetadata::checked(table, records, cancel)
    }

    fn charge(&mut self) -> Result<(), DefinitionMappingError> {
        self.remaining = self
            .remaining
            .checked_sub(1)
            .ok_or(DefinitionMappingError::LimitExceeded)?;
        Ok(())
    }
}
