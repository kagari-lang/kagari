//! Portable construction selections; a method list is not a conformance proof.
use crate::{
    callable::{
        construction::requirements::required_constructions, interface::dynamic_interface_valid,
        witness::OperationWitness,
    },
    types::{ConcreteFunctionIdentity, proofs::ProofCatalog},
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{mapping::DefinitionMappingError, table::DefinitionTable},
};
use kagari_types::{
    declaration::verify::types_in_scope,
    ty::{
        GenericBound, GenericParam, NominalTy, Ty,
        inheritance::interface_upcast_targets,
        substitution::{TypeSubstitution, TypeTransformError},
    },
};

/// For `erase<T: Read>(x: T) -> Read`, receiver is T and interface is Read.
/// Both types belong to the supplying caller's binder scope.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ConstructionRequirement {
    pub receiver: Ty,
    pub interface: NominalTy,
}

impl ConstructionRequirement {
    /// Normalize selected associated outputs using the supplying executable catalog.
    pub fn normalize(
        &self,
        catalog: &ProofCatalog<'_>,
        cancel: &CancellationToken,
    ) -> Result<Self, TypeTransformError> {
        let Ty::Trait(interface) = catalog.normalize(&Ty::Trait(self.interface.clone()), cancel)?
        else {
            return Err(TypeTransformError::InvalidContract);
        };
        Ok(Self {
            receiver: catalog.normalize(&self.receiver, cancel)?,
            interface,
        })
    }

    /// Apply exactly one binder layer, retaining associated-output expressions.
    pub fn apply(
        &self,
        substitution: &TypeSubstitution<'_>,
        cancel: &CancellationToken,
    ) -> Result<Self, TypeTransformError> {
        Ok(Self {
            receiver: substitution.apply(&self.receiver, cancel)?,
            interface: substitution.apply_nominal(&self.interface, cancel)?,
        })
    }

    pub fn structurally_valid(
        &self,
        parameters: &[GenericParam],
        definitions: &DefinitionTable,
        cancel: &CancellationToken,
    ) -> bool {
        let Ok(interface) = TypeSubstitution::default().apply_nominal(&self.interface, cancel)
        else {
            return false;
        };
        self.receiver.within_wire_limits()
            && Ty::Trait(interface.clone()).within_wire_limits()
            && !self.receiver.contains_stored_reference()
            && types_in_scope(
                [&self.receiver, &Ty::Trait(interface)],
                parameters,
                cancel,
                definitions,
            )
    }
}

/// An exact selection supplied by a checked caller, including an empty trait.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ConstructionWitness {
    pub requirement: ConstructionRequirement,
    pub selection: ConstructionSelection,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ConstructionSelection {
    /// Includes existing synthesized intrinsic tables. Dependencies belong to the
    /// selected implementation's receiver bounds, not an arbitrary trait method.
    Declared {
        implementation: ConcreteFunctionIdentity,
        operations: Vec<OperationWitness>,
        constructions: Vec<ConstructionWitness>,
    },
    /// Retain/copy or upcast an already boxed interface through its checked view.
    Interface,
    /// Retain the caller's selected capability, including supplying generations.
    Forward,
}

impl ConstructionWitness {
    /// Bound the combined construction and method dependency tree before recursion.
    pub(crate) fn check_shape(
        &self,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        enum Node<'a> {
            Construction(&'a ConstructionWitness),
            Operation(&'a OperationWitness),
        }
        let mut pending = vec![(Node::Construction(self), 0)];
        let mut count = 0;
        while let Some((node, depth)) = pending.pop() {
            cancel
                .check()
                .map_err(|_| DefinitionMappingError::Cancelled)?;
            count += 1;
            if count > 4096 || depth > 64 {
                return Err(DefinitionMappingError::LimitExceeded);
            }
            match node {
                Node::Construction(witness) => {
                    if let ConstructionSelection::Declared {
                        operations,
                        constructions,
                        ..
                    } = &witness.selection
                    {
                        if operations.len() > 4096 || constructions.len() > 4096 {
                            return Err(DefinitionMappingError::LimitExceeded);
                        }
                        pending.extend(
                            operations
                                .iter()
                                .map(|child| (Node::Operation(child), depth + 1)),
                        );
                        pending.extend(
                            constructions
                                .iter()
                                .map(|child| (Node::Construction(child), depth + 1)),
                        );
                    }
                }
                Node::Operation(operation) => {
                    let children = operation.children();
                    if children.len() > 4096 {
                        return Err(DefinitionMappingError::LimitExceeded);
                    }
                    pending.extend(
                        children
                            .iter()
                            .map(|child| (Node::Operation(child), depth + 1)),
                    );
                }
            }
        }
        Ok(())
    }

    pub fn structurally_valid(
        &self,
        parameters: &[GenericParam],
        definitions: &DefinitionTable,
        cancel: &CancellationToken,
    ) -> bool {
        if self.check_shape(cancel).is_err() {
            return false;
        }
        let mut pending = vec![(self, 0)];
        let mut count = 0;
        while let Some((witness, depth)) = pending.pop() {
            count += 1;
            if count > 4096
                || depth > 64
                || cancel.check().is_err()
                || !witness
                    .requirement
                    .structurally_valid(parameters, definitions, cancel)
            {
                return false;
            }
            match &witness.selection {
                ConstructionSelection::Declared {
                    implementation,
                    operations,
                    constructions,
                } => {
                    if definitions.resolve(implementation.declaration).is_err()
                        || implementation.arguments.len() > 4096
                        || operations.len() > 4096
                        || constructions.len() > 4096
                        || !types_in_scope(
                            &implementation.arguments,
                            parameters,
                            cancel,
                            definitions,
                        )
                        || operations
                            .iter()
                            .any(|operation| !operation.structurally_valid(parameters, definitions))
                    {
                        return false;
                    }
                    pending.extend(constructions.iter().map(|child| (child, depth + 1)));
                }
                ConstructionSelection::Interface => {
                    if !matches!(witness.requirement.receiver, Ty::Trait(_)) {
                        return false;
                    }
                }
                ConstructionSelection::Forward => {
                    if parameters.is_empty() {
                        return false;
                    }
                }
            }
        }
        true
    }

    /// Validate against checked declarations; no source inference or runtime search.
    pub fn valid(
        &self,
        catalog: &ProofCatalog<'_>,
        parameters: &[GenericParam],
        assumptions: &[GenericBound],
        cancel: &CancellationToken,
    ) -> Result<bool, TypeTransformError> {
        cancel.check().map_err(|_| TypeTransformError::Cancelled)?;
        if !self.structurally_valid(parameters, catalog.definitions(), cancel)
            || !dynamic_interface_valid(
                parameters,
                &self.requirement.interface,
                &|id| catalog.trait_contract(id),
                catalog.definitions(),
                cancel,
            )?
        {
            return Ok(false);
        }
        match &self.selection {
            ConstructionSelection::Declared { .. } => {
                catalog.construction_matches(self, parameters, assumptions, cancel)
            }
            ConstructionSelection::Interface => {
                let Ty::Trait(source) = &self.requirement.receiver else {
                    return Ok(false);
                };
                Ok(interface_upcast_targets(
                    source,
                    &self.requirement.receiver,
                    cancel,
                    catalog.definitions(),
                    &|id| catalog.trait_contract(id),
                )?
                .contains(&self.requirement.interface))
            }
            ConstructionSelection::Forward => {
                let available = required_constructions(
                    assumptions,
                    parameters,
                    &|id| catalog.trait_contract(id),
                    catalog.definitions(),
                    cancel,
                )?;
                let required = self.requirement.normalize(catalog, cancel)?;
                for available in available {
                    if available.normalize(catalog, cancel)? == required {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
        }
    }
}

mod mapping;
