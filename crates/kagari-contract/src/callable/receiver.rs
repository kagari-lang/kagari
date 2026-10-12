//! Receiver equality retained by checked interface construction.
use crate::{
    callable::witness::OperationWitness,
    types::{ConcreteFunctionIdentity, proofs::ProofCatalog},
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionKind,
        mapping::{DefinitionMapper, DefinitionMappingError, check_cancel},
        scoped::{DefinitionReferences, RemapDefinitions},
        table::DefinitionId,
    },
};
use kagari_types::{
    language::Protocol,
    ty::{GenericBound, GenericParam, Ty, substitution::TypeTransformError},
};

/// A selected object comparison, separate from the boxed interface's bounds.
/// For `val xs: List<i32> = vec`, this carries Vec<i32>'s selected PartialEq
/// operation. It grants neither Eq nor Hash to List<i32>. Both operands must be
/// checked against the retained receiver before invoking the operation.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ReceiverComparison {
    pub operation: OperationWitness,
}

impl ReceiverComparison {
    /// Recheck the receiver, canonical equality member and selected operation.
    /// This authenticates selection only; the executable owner must also check
    /// its target and the runtime must retain that owner's generation.
    pub fn valid(
        &self,
        receiver: &Ty,
        catalog: &ProofCatalog<'_>,
        parameters: &[GenericParam],
        assumptions: &[GenericBound],
        cancel: &CancellationToken,
    ) -> Result<bool, TypeTransformError> {
        if !object_receiver(receiver) {
            return Ok(false);
        }
        let required = self.operation.requirement();
        let definitions = catalog.definitions();
        if required.receiver != *receiver
            || Protocol::from_definition(&required.interface.declaration, definitions)
                != Some(Protocol::PartialEq)
            || !required.interface.arguments.is_empty()
            || !required.interface.associated_types.is_empty()
            || !required.arguments.is_empty()
            || definitions.lookup_child(
                required.interface.declaration,
                DefinitionKind::Method,
                "eq",
                0,
            ) != Some(required.member)
        {
            return Ok(false);
        }
        self.operation
            .valid(catalog, parameters, assumptions, cancel)
    }
}

/// Categories whose equality may be retained as object comparison when boxed.
/// Representation allocation alone does not make scalars, tuples or enums objects.
pub fn object_receiver(receiver: &Ty) -> bool {
    matches!(
        receiver,
        Ty::Struct(_) | Ty::NativeObject(_) | Ty::Array(_) | Ty::Map { .. } | Ty::Set(_, _)
    )
}

/// One demanded interface table and the comparison selected for its receiver.
/// Unused generic templates can omit comparison; executable construction must
/// supply comparison evidence when exposing an object receiver.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct InterfaceInstance {
    pub declaration: DefinitionId,
    pub arguments: Vec<Ty>,
    pub comparison: Option<ReceiverComparison>,
}

impl InterfaceInstance {
    /// The ordinary implementation identity used by existing table linkage.
    pub fn identity(&self) -> ConcreteFunctionIdentity {
        ConcreteFunctionIdentity {
            declaration: self.declaration,
            arguments: self.arguments.clone(),
        }
    }
}

impl RemapDefinitions for ReceiverComparison {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        Ok(Self {
            operation: self.operation.remap_definitions(mapper)?,
        })
    }
}

impl DefinitionReferences for ReceiverComparison {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        self.operation.visit_definition_ids(visit, cancel)
    }
}

impl RemapDefinitions for InterfaceInstance {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        let identity = self.identity().remap_definitions(mapper)?;
        Ok(Self {
            declaration: identity.declaration,
            arguments: identity.arguments,
            comparison: self
                .comparison
                .as_ref()
                .map(|comparison| comparison.remap_definitions(mapper))
                .transpose()?,
        })
    }
}

impl DefinitionReferences for InterfaceInstance {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        self.identity().visit_definition_ids(visit, cancel)?;
        if let Some(comparison) = &self.comparison {
            comparison.visit_definition_ids(visit, cancel)?;
        }
        Ok(())
    }
}
