use crate::{
    language::primitive as intrinsic,
    types::proofs::{Budget, ProofCatalog, satisfies, search::Search, validate_references},
};
use kagari_common::{cancellation::CancellationToken, identity::table::DefinitionId};
use kagari_types::{
    language::Protocol,
    ty::{
        GenericBound, NominalTy, Ty, inheritance,
        matching::applied_output,
        substitution::{TypeTransformError, normalize_projections},
    },
};
use std::cell::RefCell;

impl ProofCatalog<'_> {
    pub fn normalize(&self, ty: &Ty, cancel: &CancellationToken) -> Result<Ty, TypeTransformError> {
        validate_references(ty, &self.definitions, cancel)?;
        self.normalize_with(ty, &[], &mut Search::default(), &Budget::new(cancel), 0)
    }

    pub(super) fn normalize_with(
        &self,
        ty: &Ty,
        assumptions: &[GenericBound],
        search: &mut Search,
        budget: &Budget<'_>,
        depth: usize,
    ) -> Result<Ty, TypeTransformError> {
        budget.step(depth)?;
        // Nested lookups must see the active proof stack, even through the
        // transform's shared callback. Each lookup reborrows it synchronously.
        let search = RefCell::new(search);
        normalize_projections(
            ty,
            &|interface, receiver, member, arguments| {
                budget.step(depth)?;
                if arguments.is_empty() {
                    if let Some(output) = intrinsic::associated_output(
                        interface,
                        receiver,
                        member,
                        budget.cancel,
                        &self.definitions,
                    )? {
                        return Ok(Some(output));
                    }
                    if Protocol::from_definition(&interface.declaration, &self.definitions)
                        == Some(Protocol::Iterable)
                        && let Some(required) =
                            intrinsic::identity_iterator(interface, receiver, &self.definitions)
                        && self.prove(
                            &required,
                            receiver,
                            assumptions,
                            *search.borrow_mut(),
                            budget,
                            depth + 1,
                        )?
                    {
                        if *member == self.associated(interface.declaration, "Iter")? {
                            return Ok(Some(receiver.clone()));
                        }
                        if *member == self.associated(interface.declaration, "Item")? {
                            return Ok(Some(Ty::Projection {
                                receiver: Box::new(receiver.clone()),
                                member: self.associated(required.declaration, "Item")?,
                                interface: Box::new(required),
                                arguments: vec![],
                            }));
                        }
                    }
                    if let Some(output) = self.assumed_output(
                        interface,
                        receiver,
                        member,
                        assumptions,
                        budget,
                        depth,
                    )? {
                        return Ok(Some(output));
                    }
                    if let Ty::Trait(view) = receiver {
                        return Ok(inheritance::interface_views(
                            view,
                            receiver,
                            budget.cancel,
                            &self.definitions,
                            &|id| self.contracts.get(id).copied(),
                        )?
                        .into_iter()
                        .find(|parent| satisfies(parent, interface))
                        .and_then(|parent| parent.associated_types.get(member).cloned()));
                    }
                }
                let applications = self.explicit_applications(
                    interface,
                    receiver,
                    assumptions,
                    *search.borrow_mut(),
                    budget,
                    depth + 1,
                )?;
                let Ok([selected]) = <[_; 1]>::try_from(applications) else {
                    return Ok(None);
                };
                applied_output(
                    selected
                        .table
                        .interface()
                        .ok_or(TypeTransformError::InvalidContract)?,
                    selected.table.families(),
                    selected.bindings,
                    member,
                    arguments,
                    budget.cancel,
                )
            },
            budget.cancel,
        )
    }

    pub(super) fn assumed_output(
        &self,
        interface: &NominalTy,
        receiver: &Ty,
        member: &DefinitionId,
        assumptions: &[GenericBound],
        budget: &Budget<'_>,
        depth: usize,
    ) -> Result<Option<Ty>, TypeTransformError> {
        let mut selected = None;
        for bound in assumptions.iter().filter(|bound| bound.ty == *receiver) {
            for available in &bound.constraints {
                budget.step(depth)?;
                for parent in self.ancestry(available, receiver, budget.cancel)? {
                    if satisfies(&parent, interface)
                        && let Some(value) = parent.associated_types.get(member)
                    {
                        if selected.as_ref().is_some_and(|old| old != value) {
                            return Err(TypeTransformError::InvalidContract);
                        }
                        selected = Some(value.clone());
                    }
                }
            }
        }
        Ok(selected.filter(|value| {
            let Ty::Projection {
                receiver: owner,
                interface: view,
                member: output,
                arguments,
            } = value
            else {
                return true;
            };
            owner.as_ref() != receiver
                || view.as_ref() != interface
                || output != member
                || !arguments.is_empty()
        }))
    }
}
