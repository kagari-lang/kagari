//! Complete evidence for an already selected executable construction application.
use crate::{
    callable::{
        construction::{
            evidence::{ConstructionRequirement, ConstructionSelection, ConstructionWitness},
            requirements::required_constructions,
        },
        generic::GenericBody,
        witness::{OperationWitness, SharedMethodWitness, is_generic_member, required_operations},
    },
    native_import::callables::normalize_requirement,
    types::{
        ConcreteFunctionIdentity,
        proofs::{Budget, ProofCatalog, search::Search},
    },
};
use kagari_common::cancellation::CancellationToken;
use kagari_types::{
    declaration::{requirement::NativeCallableRequirement, verify::native_bounds_valid},
    ty::{
        GenericBound, GenericParam,
        substitution::{TypeSubstitution, TypeTransformError},
    },
};

/// A failed derivation never supplies a partial witness or relaxes a bound.
#[derive(Debug, PartialEq, Eq)]
pub enum ConstructionDerivationError {
    Transform(TypeTransformError),
    /// The exact selected application, receiver, view, or caller scope is invalid.
    InvalidSelection,
    /// No declared executable selection or caller capability supplies this view.
    /// Structural conformance alone cannot identify a generated bridge.
    MissingConstruction(Box<ConstructionRequirement>),
    /// No checked operation selection is available. A generated adapter requires
    /// its compiler-selected application and evidence, not an invented body ID.
    MissingOperation(Box<NativeCallableRequirement>),
    /// More than one applicable explicit table exists for a nested requirement.
    AmbiguousImplementation(Box<ConstructionRequirement>),
}

impl From<TypeTransformError> for ConstructionDerivationError {
    fn from(error: TypeTransformError) -> Self {
        Self::Transform(error)
    }
}

impl ProofCatalog<'_> {
    /// Derive complete evidence for the exact selected `MakeInterface` application.
    ///
    /// `requirement` names the receiver and requested (possibly inherited) view;
    /// `implementation` is the instruction's selected table and applied arguments.
    /// `body` supplies the already verified enclosing body's binder and assumptions,
    /// or `None` for a concrete body. All IDs must belong to this linked catalog.
    /// The returned requirement is normalized; the selected application is retained.
    ///
    /// For `impl<T: Read> View for Box<T>`, selecting `Box<U>` under `U: Read`
    /// derives both forwarded Read operations and its construction capability.
    /// Concrete dependencies use the catalog's unique explicit-table selection;
    /// boxed-interface views and inherited caller bounds use their existing owners.
    /// This never searches for a replacement for the instruction's selection.
    ///
    /// `max_nodes` bounds the combined operation/construction tree, capped at 4096.
    /// Recursion is bounded to 64 levels; catalog searches retain their bounded work
    /// and cancellation checks. Missing/ambiguous evidence fails closed. In particular,
    /// structural conformance does not authorize synthesizing an adapter or choosing
    /// an unrecorded intrinsic bridge. Such cases need compiler-carried evidence.
    /// This is extraction/admission work, not an execution-time resolver.
    pub fn derive_selected_construction(
        &self,
        requirement: &ConstructionRequirement,
        implementation: &ConcreteFunctionIdentity,
        body: Option<&GenericBody>,
        max_nodes: usize,
        cancel: &CancellationToken,
    ) -> Result<ConstructionWitness, ConstructionDerivationError> {
        cancel.check().map_err(|_| TypeTransformError::Cancelled)?;
        let (parameters, assumptions) = body
            .map(|body| (body.parameters.as_slice(), body.bounds.as_slice()))
            .unwrap_or((&[], &[]));
        if max_nodes == 0 || parameters.len() > 4096 || implementation.arguments.len() > 4096 {
            return Err(TypeTransformError::LimitExceeded.into());
        }
        self.preflight(
            &requirement.interface,
            &requirement.receiver,
            assumptions,
            cancel,
        )?;
        if !native_bounds_valid(assumptions, parameters, cancel, self.definitions()) {
            cancel.check().map_err(|_| TypeTransformError::Cancelled)?;
            return Err(ConstructionDerivationError::InvalidSelection);
        }
        let copier = TypeSubstitution::default();
        for argument in &implementation.arguments {
            copier.apply(argument, cancel)?;
        }
        let mut derivation = Derivation {
            catalog: self,
            parameters,
            assumptions,
            cancel,
            remaining: max_nodes.min(4096),
            proof_budget: Budget::new(cancel),
        };
        derivation.step(0)?;
        let required = requirement.normalize(self, cancel)?;
        let result = derivation.declared(required, implementation, 0);
        cancel.check().map_err(|_| TypeTransformError::Cancelled)?;
        result
    }
}

struct Derivation<'a, 'catalog> {
    catalog: &'a ProofCatalog<'catalog>,
    parameters: &'a [GenericParam],
    assumptions: &'a [GenericBound],
    cancel: &'a CancellationToken,
    remaining: usize,
    proof_budget: Budget<'a>,
}

struct Dependencies {
    operations: Vec<OperationWitness>,
    constructions: Vec<ConstructionWitness>,
}

impl Derivation<'_, '_> {
    fn step(&mut self, depth: usize) -> Result<(), ConstructionDerivationError> {
        self.proof_budget.step(depth)?;
        self.remaining = self
            .remaining
            .checked_sub(1)
            .ok_or(TypeTransformError::LimitExceeded)?;
        Ok(())
    }

    fn declared(
        &mut self,
        requirement: ConstructionRequirement,
        implementation: &ConcreteFunctionIdentity,
        depth: usize,
    ) -> Result<ConstructionWitness, ConstructionDerivationError> {
        let bounds = self
            .catalog
            .selected_construction_bounds(
                &requirement,
                implementation,
                self.assumptions,
                self.cancel,
            )?
            .ok_or(ConstructionDerivationError::InvalidSelection)?;
        let dependencies = self.dependencies(&bounds, depth + 1)?;
        let witness = ConstructionWitness {
            requirement,
            selection: ConstructionSelection::Declared {
                implementation: implementation.clone(),
                operations: dependencies.operations,
                constructions: dependencies.constructions,
            },
        };
        if !witness.valid(self.catalog, self.parameters, self.assumptions, self.cancel)? {
            return Err(ConstructionDerivationError::InvalidSelection);
        }
        Ok(witness)
    }

    fn unique_application(
        &self,
        requirement: &ConstructionRequirement,
        depth: usize,
    ) -> Result<Option<ConcreteFunctionIdentity>, ConstructionDerivationError> {
        let mut applications = self.catalog.explicit_applications(
            &requirement.interface,
            &requirement.receiver,
            self.assumptions,
            &mut Search::default(),
            &self.proof_budget,
            depth,
        )?;
        if applications.len() > 1 {
            return Err(ConstructionDerivationError::AmbiguousImplementation(
                Box::new(requirement.clone()),
            ));
        }
        Ok(applications.pop())
    }

    fn construction(
        &mut self,
        requirement: ConstructionRequirement,
        depth: usize,
    ) -> Result<ConstructionWitness, ConstructionDerivationError> {
        self.step(depth)?;
        for selection in [
            ConstructionSelection::Forward,
            ConstructionSelection::Interface,
        ] {
            let witness = ConstructionWitness {
                requirement: requirement.clone(),
                selection,
            };
            if witness.valid(self.catalog, self.parameters, self.assumptions, self.cancel)? {
                return Ok(witness);
            }
        }
        let implementation = self
            .unique_application(&requirement, depth)?
            .ok_or_else(|| {
                ConstructionDerivationError::MissingConstruction(Box::new(requirement.clone()))
            })?;
        self.declared(requirement, &implementation, depth)
    }

    fn operation(
        &mut self,
        requirement: NativeCallableRequirement,
        depth: usize,
    ) -> Result<OperationWitness, ConstructionDerivationError> {
        self.step(depth)?;
        let forwarded = OperationWitness::Forward(Box::new(requirement.clone()));
        if forwarded.valid(self.catalog, self.parameters, self.assumptions, self.cancel)? {
            return Ok(forwarded);
        }
        if !is_generic_member(&requirement, self.catalog)
            && let Some(selected) = self.catalog.select_callable(&requirement, self.cancel)?
        {
            let operation = OperationWitness::Selected(Box::new(selected));
            if operation.valid(self.catalog, self.parameters, self.assumptions, self.cancel)? {
                return Ok(operation);
            }
        }
        let construction = ConstructionRequirement {
            receiver: requirement.receiver.clone(),
            interface: requirement.interface.clone(),
        };
        if requirement.arguments.is_empty()
            && let Some(implementation) = self.unique_application(&construction, depth)?
        {
            let table = self
                .catalog
                .implementations
                .iter()
                .find(|table| *table.declaration() == implementation.declaration)
                .expect("application selected from this catalog");
            let mut substitution = TypeSubstitution::default();
            for (parameter, argument) in table.parameters().iter().zip(&implementation.arguments) {
                substitution.bind(&parameter.owner, parameter.position, argument);
            }
            let bounds = substitution.apply_bounds(
                table.bounds(),
                self.catalog.definitions(),
                self.cancel,
            )?;
            let dependencies = self.dependencies(&bounds, depth + 1)?;
            let operation = OperationWitness::SharedMethod(Box::new(SharedMethodWitness {
                requirement: requirement.clone(),
                implementation,
                operations: dependencies.operations,
                constructions: dependencies.constructions,
            }));
            if operation.valid(self.catalog, self.parameters, self.assumptions, self.cancel)? {
                return Ok(operation);
            }
        }
        Err(ConstructionDerivationError::MissingOperation(Box::new(
            requirement,
        )))
    }

    fn dependencies(
        &mut self,
        bounds: &[GenericBound],
        depth: usize,
    ) -> Result<Dependencies, ConstructionDerivationError> {
        let required = required_operations(
            bounds,
            &|id| self.catalog.trait_contract(id),
            self.cancel,
            self.catalog.definitions(),
        )?;
        let constructions =
            required_constructions(bounds, self.parameters, self.catalog, self.cancel)?;
        if required.len().saturating_add(constructions.len()) > self.remaining {
            return Err(TypeTransformError::LimitExceeded.into());
        }
        let mut operations = Vec::new();
        for required in required {
            let required = normalize_requirement(&required, self.catalog, self.cancel)?;
            operations.push(self.operation(required, depth)?);
        }
        let mut witnesses = Vec::new();
        for requirement in constructions {
            witnesses.push(self.construction(requirement, depth)?);
        }
        Ok(Dependencies {
            operations,
            constructions: witnesses,
        })
    }
}

#[cfg(test)]
mod tests;
