//! Derive selected construction evidence through the shared linked proof owner.
use crate::program::updates::context::Context;
use kagari_common::identity::DefinitionKind;
use kagari_contract::{
    callable::{
        construction::evidence::{ConstructionRequirement, ConstructionWitness},
        evidence::EvidenceRef,
        generic::GenericBody,
    },
    types::{
        ConcreteFunctionIdentity, PublicItem,
        proofs::{
            ProofCatalog, derivation::ConstructionDerivationError, implementation::Implementation,
        },
    },
    updates::limits::{Budget, MAX_NESTED, UpdateError},
};

impl<'a> Context<'a> {
    pub(super) fn construction(
        &self,
        requirement: &ConstructionRequirement,
        implementation: &ConcreteFunctionIdentity,
        body: Option<&GenericBody>,
        budget: &mut Budget<'_>,
    ) -> Result<ConstructionWitness, UpdateError> {
        let witness = self
            .catalog(budget)?
            .derive_selected_construction(
                requirement,
                implementation,
                body,
                MAX_NESTED,
                self.cancel,
            )
            .map_err(|error| match error {
                ConstructionDerivationError::Transform(error) => UpdateError::from(error),
                ConstructionDerivationError::InvalidSelection => UpdateError::InvalidEvidence,
                ConstructionDerivationError::MissingConstruction(_)
                | ConstructionDerivationError::MissingOperation(_)
                | ConstructionDerivationError::AmbiguousImplementation(_) => {
                    UpdateError::MissingCoverage
                }
            })?;
        // Derivation bounds its temporary tree. Charge every returned node before
        // retaining it in the program-wide certificate, including nested operations.
        budget.records(1)?;
        let mut pending = vec![EvidenceRef::Construction(&witness)];
        while let Some(node) = pending.pop() {
            budget.visit()?;
            let (operations, constructions) = node.children();
            budget.records(operations.len() + constructions.len())?;
            pending.extend(operations.iter().map(EvidenceRef::Operation));
            pending.extend(constructions.iter().map(EvidenceRef::Construction));
        }
        Ok(witness)
    }

    fn catalog(&self, budget: &mut Budget<'_>) -> Result<&ProofCatalog<'a>, UpdateError> {
        if let Some(catalog) = self.construction_catalog.get() {
            return Ok(catalog);
        }
        let mut implementations = Vec::new();
        let mut contracts = Vec::new();
        let mut enumerations = Vec::new();
        let mut natives = Vec::new();
        for module in self.modules {
            for item in &module.public_items {
                budget.visit()?;
                match item {
                    PublicItem::InterfaceTable(table) => {
                        budget.records(1)?;
                        implementations.push(Implementation::Interface(table));
                    }
                    PublicItem::Trait(contract) => {
                        let declaration = self
                            .definitions
                            .lookup_root(&module.identity)
                            .and_then(|root| {
                                self.definitions.lookup_child(
                                    root,
                                    DefinitionKind::Trait,
                                    &contract.name,
                                    0,
                                )
                            })
                            .ok_or(UpdateError::InvalidEvidence)?;
                        budget.records(1)?;
                        contracts.push((declaration, contract));
                    }
                    _ => {}
                }
            }
            for contract in &module.trait_contracts {
                budget.visit()?;
                budget.records(1)?;
                contracts.push((contract.declaration, &contract.abi));
            }
            for enumeration in &module.enumerations {
                budget.visit()?;
                budget.records(1)?;
                enumerations.push(enumeration);
            }
            for native in &module.native_declarations {
                budget.visit()?;
                budget.records(1)?;
                natives.push(native);
            }
        }
        let catalog = ProofCatalog::new(
            implementations,
            enumerations,
            contracts,
            natives,
            self.definitions,
            self.cancel,
        )?;
        self.construction_catalog
            .set(catalog)
            .map_err(|_| UpdateError::InvalidEvidence)?;
        Ok(self
            .construction_catalog
            .get()
            .expect("initialized construction catalog"))
    }
}
