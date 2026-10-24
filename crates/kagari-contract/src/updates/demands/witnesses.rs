//! Visit the existing checked witness graph; forwarding retains its binder authority.
use crate::{
    callable::{
        construction::evidence::{ConstructionSelection, ConstructionWitness},
        evidence::EvidenceRef,
        generic::GenericBody,
        witness::OperationWitness,
    },
    updates::{
        DemandEnvelope,
        demands::{Walker, apply_instance, requests::Request},
        limits::{Budget, UpdateError},
    },
};
use kagari_types::{callable::CallableImplementation, ty::substitution::TypeSubstitution};

impl Walker<'_, '_> {
    pub(super) fn operation(
        &mut self,
        value: &OperationWitness,
        bindings: &TypeSubstitution<'_>,
        symbolic_scope: Option<&GenericBody>,
        envelope: &DemandEnvelope,
        budget: &mut Budget<'_>,
    ) -> Result<(), UpdateError> {
        self.evidence(
            EvidenceRef::Operation(value),
            bindings,
            symbolic_scope,
            envelope,
            budget,
        )
    }

    pub(super) fn witness(
        &mut self,
        value: &ConstructionWitness,
        bindings: &TypeSubstitution<'_>,
        symbolic_scope: Option<&GenericBody>,
        envelope: &DemandEnvelope,
        budget: &mut Budget<'_>,
    ) -> Result<(), UpdateError> {
        self.evidence(
            EvidenceRef::Construction(value),
            bindings,
            symbolic_scope,
            envelope,
            budget,
        )
    }

    fn evidence(
        &mut self,
        root: EvidenceRef<'_>,
        bindings: &TypeSubstitution<'_>,
        symbolic_scope: Option<&GenericBody>,
        envelope: &DemandEnvelope,
        budget: &mut Budget<'_>,
    ) -> Result<(), UpdateError> {
        root.check_shape(budget.cancel)?;
        let mut pending = vec![root];
        while let Some(node) = pending.pop() {
            budget.visit()?;
            match node {
                EvidenceRef::Operation(OperationWitness::Selected(call)) => {
                    let instance = apply_instance(&call.instance, bindings, budget)?;
                    match call.implementation {
                        CallableImplementation::Script => {
                            self.direct(instance, envelope, budget)?
                        }
                        CallableImplementation::Native(binding) => {
                            let natives = self.find_natives(&instance, Some(binding), budget)?;
                            if natives.is_empty() {
                                self.requests.missing(Request::Native(instance), budget)?;
                            }
                            for index in natives {
                                if self.coverage.natives[index].import.signature != call.signature {
                                    return Err(UpdateError::InvalidEvidence);
                                }
                                self.native(index, bindings, envelope, budget)?;
                            }
                        }
                        _ => return Err(UpdateError::InvalidEvidence),
                    }
                }
                EvidenceRef::Operation(OperationWitness::SharedMethod(method)) => {
                    self.construction(&method.implementation, bindings, symbolic_scope, budget)?
                }
                EvidenceRef::Operation(OperationWitness::SharedAdapter(adapter)) => {
                    // shared() owns the children; do not traverse them a second time.
                    self.shared(&adapter.call, bindings, symbolic_scope, envelope, budget)?;
                    continue;
                }
                EvidenceRef::Construction(witness) => match &witness.selection {
                    ConstructionSelection::Declared { implementation, .. } => {
                        self.construction(implementation, bindings, symbolic_scope, budget)?
                    }
                    ConstructionSelection::Interface => {}
                    ConstructionSelection::Forward if symbolic_scope.is_some() => {}
                    ConstructionSelection::Forward => return Err(UpdateError::InvalidEvidence),
                },
                EvidenceRef::Operation(OperationWitness::Forward(_))
                    if symbolic_scope.is_some() => {}
                EvidenceRef::Operation(OperationWitness::Forward(_)) => {
                    return Err(UpdateError::InvalidEvidence);
                }
            }
            let (operations, constructions) = node.children();
            budget.records(operations.len() + constructions.len())?;
            pending.extend(operations.iter().map(EvidenceRef::Operation));
            pending.extend(constructions.iter().map(EvidenceRef::Construction));
        }
        Ok(())
    }
}
