use crate::standard::intrinsic;
use crate::standard::traits::StandardTrait;
use crate::types::AbiType;
use crate::types::matching;
use crate::types::proofs::search::Search;
use crate::types::proofs::{Budget, ProofCatalog, host_application, satisfies};
use crate::types::substitution::{TypeTransformError, normalize_projections};
use kagari_common::cancellation::CancellationToken;
use kagari_common::identity::associated_type_id;

impl ProofCatalog<'_> {
    pub fn normalize(
        &self,
        ty: &AbiType,
        cancel: &CancellationToken,
    ) -> Result<AbiType, TypeTransformError> {
        self.normalize_with(ty, &Budget::new(cancel), 0)
    }

    pub(super) fn normalize_with(
        &self,
        ty: &AbiType,
        budget: &Budget<'_>,
        depth: usize,
    ) -> Result<AbiType, TypeTransformError> {
        budget.step(depth)?;
        normalize_projections(
            ty,
            &|interface, receiver, member, arguments| {
                budget.step(depth)?;
                if arguments.is_empty() {
                    if let Some(output) =
                        intrinsic::associated_output(interface, receiver, member, budget.cancel)?
                    {
                        return Ok(Some(output));
                    }
                    if StandardTrait::from_id(&interface.declaration)
                        == Some(StandardTrait::Iterable)
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
                            return Ok(Some(AbiType::Projection {
                                receiver: Box::new(receiver.clone()),
                                member: associated_type_id(&required.declaration, "Item"),
                                interface: Box::new(required),
                                arguments: vec![],
                            }));
                        }
                    }
                    if let AbiType::Trait(view) = receiver {
                        return Ok(self
                            .ancestry(view, receiver, budget.cancel)?
                            .into_iter()
                            .find(|parent| satisfies(parent, interface))
                            .and_then(|parent| parent.associated_types.get(member).cloned()));
                    }
                }
                let mut selected = None;
                for table in &self.tables {
                    budget.step(depth)?;
                    if let Some(output) = matching::projection_output(
                        table,
                        interface,
                        receiver,
                        member,
                        arguments,
                        budget.cancel,
                    )? {
                        if selected.is_some() {
                            return Ok(None);
                        }
                        selected = Some(output);
                    }
                }
                if arguments.is_empty()
                    && let AbiType::Host(id) = receiver
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
