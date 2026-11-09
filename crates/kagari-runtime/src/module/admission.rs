//! Installation-local identity adoption, without copying executable products.
use crate::{
    error::{RuntimeError, RuntimeErrorKind},
    module::VerifiedProgram,
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        map::DefinitionContext,
        mapping::{DefinitionMapper, DefinitionMappingError},
        scoped::RemapDefinitions,
        table::{DefinitionId, DefinitionRemap, DefinitionTable},
    },
};
use std::{collections::HashMap, sync::Arc};

/// Identity correspondence is metadata only. Registry contracts and installation
/// ownership are checked separately before any mapped identity grants a binding.
#[derive(Debug)]
pub(crate) struct DefinitionAdmission {
    source: DefinitionTable,
    target: DefinitionTable,
    forward: DefinitionRemap,
    reverse: HashMap<DefinitionId, DefinitionId>,
}

impl DefinitionAdmission {
    pub(crate) fn prepare(
        program: &VerifiedProgram,
        context: &DefinitionContext,
        cancel: &CancellationToken,
    ) -> Result<Arc<Self>, RuntimeError> {
        cancel
            .check()
            .map_err(|_| invalid(DefinitionMappingError::Cancelled))?;
        let source = program.definitions().clone();
        let forward = context
            .import(&source, program.definition_references.iter().copied())
            .map_err(|error| invalid(error.into()))?;
        let mut reverse = HashMap::new();
        for &reference in program.definition_references.iter() {
            let mut next = Some(reference);
            while let Some(id) = next {
                cancel
                    .check()
                    .map_err(|_| invalid(DefinitionMappingError::Cancelled))?;
                let mapped = forward.map(id).map_err(|error| invalid(error.into()))?;
                if reverse.insert(mapped, id).is_some() {
                    break;
                }
                next = source.parent(id).map_err(|error| invalid(error.into()))?;
            }
        }
        cancel
            .check()
            .map_err(|_| invalid(DefinitionMappingError::Cancelled))?;
        Ok(Arc::new(Self {
            source,
            target: context.snapshot(),
            forward,
            reverse,
        }))
    }

    pub(crate) fn source(&self) -> &DefinitionTable {
        &self.source
    }

    pub(crate) fn equivalent<T: RemapDefinitions + PartialEq>(
        &self,
        source: &Vec<T>,
        canonical: &[T],
    ) -> bool {
        source == canonical
            || self
                .runtime(source, &CancellationToken::default())
                .as_ref()
                .is_ok_and(|mapped| mapped == canonical)
    }

    pub(crate) fn target(&self) -> &DefinitionTable {
        &self.target
    }

    pub(crate) fn runtime_id(&self, id: DefinitionId) -> Result<DefinitionId, RuntimeError> {
        self.forward.map(id).map_err(|error| invalid(error.into()))
    }

    pub(crate) fn product_id(&self, id: DefinitionId) -> Option<DefinitionId> {
        self.reverse.get(&id).copied()
    }

    pub(crate) fn lexical_id(&self, id: DefinitionId) -> Option<DefinitionId> {
        self.source
            .resolve(id)
            .ok()
            .map(|_| id)
            .or_else(|| self.product_id(id))
    }

    pub(crate) fn product<T: RemapDefinitions>(
        &self,
        records: &T,
        cancel: &CancellationToken,
    ) -> Result<T, RuntimeError> {
        records
            .remap_definitions(&mut DefinitionMapper::new(
                &mut |id| {
                    self.lexical_id(*id)
                        .ok_or(DefinitionMappingError::InvalidContract)
                },
                &self.source,
                cancel,
            ))
            .map_err(invalid)
    }

    /// Cold contract adoption and closed type preparation use this mapping;
    /// executable instructions and lexical binders remain in their source scope.
    pub(crate) fn runtime<T: RemapDefinitions>(
        &self,
        records: &T,
        cancel: &CancellationToken,
    ) -> Result<T, RuntimeError> {
        self.runtime_in(records, &self.target, cancel)
    }

    /// A closed argument may carry a later accepted prefix from this same
    /// append-only Runtime scope. It never widens the product's lexical prefix.
    pub(crate) fn runtime_in<T: RemapDefinitions>(
        &self,
        records: &T,
        target: &DefinitionTable,
        cancel: &CancellationToken,
    ) -> Result<T, RuntimeError> {
        if target.id() != self.target.id() {
            return Err(RuntimeError::module_validation(
                "foreign closed identity scope",
            ));
        }
        records
            .remap_definitions(&mut DefinitionMapper::new(
                &mut |id| {
                    if target.resolve(*id).is_ok() {
                        return Ok(*id);
                    }
                    self.forward.map(*id).map_err(DefinitionMappingError::from)
                },
                target,
                cancel,
            ))
            .map_err(invalid)
    }
}

fn invalid(error: DefinitionMappingError) -> RuntimeError {
    match error {
        DefinitionMappingError::Cancelled => RuntimeError::new(
            RuntimeErrorKind::Cancelled,
            "program identity admission cancelled",
        ),
        error => RuntimeError::module_validation(error.to_string()),
    }
}
