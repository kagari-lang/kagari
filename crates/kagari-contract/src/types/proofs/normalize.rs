use crate::{
    language::primitive as intrinsic,
    types::proofs::{Budget, ProofCatalog, host_application, satisfies, search::Search},
};
use kagari_common::{cancellation::CancellationToken, identity::associated_type_id};
use kagari_types::{
    language::Protocol,
    ty::{
        Ty, inheritance,
        matching::projection_output,
        substitution::{TypeTransformError, normalize_projections},
    },
};

impl ProofCatalog<'_> {
    pub fn normalize(&self, ty: &Ty, cancel: &CancellationToken) -> Result<Ty, TypeTransformError> {
        self.normalize_with(ty, &Budget::new(cancel), 0)
    }

    pub(super) fn normalize_with(
        &self,
        ty: &Ty,
        budget: &Budget<'_>,
        depth: usize,
    ) -> Result<Ty, TypeTransformError> {
        budget.step(depth)?;
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
                        self.trait_contract(&interface.declaration),
                    )? {
                        return Ok(Some(output));
                    }
                    if Protocol::from_id(&interface.declaration) == Some(Protocol::Iterable)
                        && let Some(required) = intrinsic::identity_iterator(interface, receiver)
                        && self.prove(
                            &required,
                            receiver,
                            &[],
                            &mut Search::default(),
                            budget,
                            depth + 1,
                        )?
                    {
                        if *member == associated_type_id(&interface.declaration, "Iter") {
                            return Ok(Some(receiver.clone()));
                        }
                        if *member == associated_type_id(&interface.declaration, "Item") {
                            return Ok(Some(Ty::Projection {
                                receiver: Box::new(receiver.clone()),
                                member: associated_type_id(&required.declaration, "Item"),
                                interface: Box::new(required),
                                arguments: vec![],
                            }));
                        }
                    }
                    if let Ty::Trait(view) = receiver {
                        return Ok(inheritance::interface_views(
                            view,
                            receiver,
                            budget.cancel,
                            &|id| self.contracts.get(id).copied(),
                        )?
                        .into_iter()
                        .find(|parent| satisfies(parent, interface))
                        .and_then(|parent| parent.associated_types.get(member).cloned()));
                    }
                }
                let mut selected = None;
                for implementation in &self.implementations {
                    budget.step(depth)?;
                    if let Some(output) =
                        projection_output(
                            implementation
                                .pattern(implementation.interface().and_then(|interface| {
                                    self.trait_contract(&interface.declaration)
                                }))
                                .ok_or(TypeTransformError::InvalidContract)?,
                            implementation.families(),
                            interface,
                            receiver,
                            member,
                            arguments,
                            budget.cancel,
                        )?
                    {
                        if selected.is_some() {
                            return Ok(None);
                        }
                        selected = Some(output);
                    }
                }
                if arguments.is_empty()
                    && let Ty::Host(id) = receiver
                {
                    for host in &self.hosts {
                        if host.id != *id {
                            continue;
                        }
                        for implementation in &host.trait_implementations {
                            budget.step(depth)?;
                            let applied = host_application(implementation);
                            if satisfies(&applied, interface)
                                && let Some(output) = applied.associated_types.get(member)
                            {
                                if selected.is_some() {
                                    return Ok(None);
                                }
                                selected = Some(output.clone());
                            }
                        }
                    }
                }
                Ok(selected)
            },
            budget.cancel,
        )
    }
}
