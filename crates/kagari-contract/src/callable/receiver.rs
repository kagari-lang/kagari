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
    declaration::verify::native_bounds_valid,
    language::Protocol,
    ty::{GenericBound, GenericParam, Ty, substitution::TypeTransformError},
};
use std::slice;

/// A selected object comparison, separate from the boxed interface's bounds.
/// For `val xs: List<i32> = vec`, this carries Vec<i32>'s selected PartialEq
/// operation. It grants neither Eq nor Hash to List<i32>. Both operands must be
/// checked against the retained receiver before invoking the operation.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ReceiverComparison {
    pub operation: OperationWitness,
    /// Exact applicability bounds of a unique explicit receiver implementation.
    /// Empty means unconditional under the interface construction assumptions.
    /// Nonempty guards require separately supplied eligibility before execution;
    /// they never add bounds to the exposed interface itself.
    pub conditions: Vec<GenericBound>,
    /// Closed applicability outcomes supplied by this checked construction.
    /// These facts are independent of caller assumptions, including false facts.
    pub eligibility: Vec<ReceiverEligibility>,
}

/// A single closed condition, authenticated against its supplying declarations.
/// A missing record proves nothing; `satisfied: false` is an explicit negative.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ReceiverEligibility {
    pub bound: GenericBound,
    pub satisfied: bool,
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
        if !object_receiver(receiver)
            || self.conditions.len() > 4096
            || self.eligibility.len() > 4096
            || !native_bounds_valid(&self.conditions, parameters, cancel, catalog.definitions())
        {
            return Ok(false);
        }
        for (index, fact) in self.eligibility.iter().enumerate() {
            if fact.bound.constraints.len() != 1
                || !fact.bound.ty.is_concrete()
                || !native_bounds_valid(
                    slice::from_ref(&fact.bound),
                    &[],
                    cancel,
                    catalog.definitions(),
                )
                || self.eligibility[..index]
                    .iter()
                    .any(|other| other.bound == fact.bound)
                || catalog.constraints_hold(&fact.bound.ty, &fact.bound.constraints, &[], cancel)?
                    != fact.satisfied
            {
                return Ok(false);
            }
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
        let mut assumptions = assumptions.to_vec();
        if !self.conditions.is_empty() {
            let OperationWitness::SharedMethod(selected) = &self.operation else {
                return Ok(false);
            };
            if !catalog.receiver_comparison_conditions_match(selected, &self.conditions, cancel)? {
                return Ok(false);
            }
            assumptions.extend(self.conditions.iter().cloned());
        }
        self.operation
            .valid(catalog, parameters, &assumptions, cancel)
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
            conditions: self
                .conditions
                .iter()
                .map(|condition| condition.remap_definitions(mapper))
                .collect::<Result<_, _>>()?,
            eligibility: self
                .eligibility
                .iter()
                .map(|fact| {
                    Ok(ReceiverEligibility {
                        bound: fact.bound.remap_definitions(mapper)?,
                        satisfied: fact.satisfied,
                    })
                })
                .collect::<Result<_, DefinitionMappingError>>()?,
        })
    }
}

impl DefinitionReferences for ReceiverComparison {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        self.operation.visit_definition_ids(visit, cancel)?;
        for condition in &self.conditions {
            condition.visit_definition_ids(visit, cancel)?;
        }
        for fact in &self.eligibility {
            fact.bound.visit_definition_ids(visit, cancel)?;
        }
        Ok(())
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
