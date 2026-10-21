//! Prepare constructor capabilities beside the existing bound method environment.
use crate::source::lower::{MirLoweringError, instances::InstancePlanner};
use kagari_common::span::Span;
use kagari_contract::{
    callable::{
        construction::evidence::{
            ConstructionRequirement, ConstructionSelection, ConstructionWitness,
        },
        witness::OperationWitness,
    },
    types::ConcreteFunctionIdentity,
};
use kagari_hir::{
    aggregates::implementations::ImplementationSearchLimits,
    types::{
        NominalType, TypeId,
        semantic::{raise_nominal_type, raise_type},
    },
};
use kagari_types::ty::Ty;
use std::cmp::Ordering;

pub(in crate::source::lower) struct BoundEvidence {
    pub(in crate::source::lower) operations: Vec<OperationWitness>,
    pub(in crate::source::lower) constructions: Vec<ConstructionWitness>,
}

impl InstancePlanner<'_> {
    pub(super) fn construction_requirement(
        &self,
        receiver: &TypeId,
        interface: &NominalType,
    ) -> Result<Option<ConstructionRequirement>, MirLoweringError> {
        let invalid = || MirLoweringError::MissingBinding("construction bound declaration");
        let contract = self
            .catalog
            .trait_(&interface.declaration)
            .ok_or_else(invalid)?;
        let mut applied = interface.clone();
        for member in contract.associated_types.keys() {
            if !contract.associated_type_parameters.contains_key(member) {
                applied
                    .associated_types
                    .entry(*member)
                    .or_insert_with(|| TypeId::Projection {
                        receiver: Box::new(receiver.clone()),
                        interface: Box::new(interface.clone()),
                        member: *member,
                        arguments: vec![],
                    });
            }
        }
        let definitions = self.definitions();
        let TypeId::Trait(applied) = self
            .catalog
            .normalize_type(&definitions, &TypeId::Trait(applied))
        else {
            return Err(invalid());
        };
        if self
            .catalog
            .interface_incompatibility(&definitions, &applied, &self.options.cancel)
            .map_err(|_| invalid())?
            .is_some()
        {
            return Ok(None);
        }
        Ok(Some(ConstructionRequirement {
            receiver: self.encode_type(&self.catalog.normalize_type(&definitions, receiver)),
            interface: self.encode_nominal(&applied),
        }))
    }

    pub(crate) fn bind_constructions(
        &mut self,
        mut requirements: Vec<ConstructionRequirement>,
        span: Span,
    ) -> Result<Vec<ConstructionWitness>, MirLoweringError> {
        if self.operation_depth >= 64 {
            return Err(MirLoweringError::MissingBinding(
                "construction composition depth",
            ));
        }
        let definitions = self.definitions();
        let mut failure = None;
        requirements.sort_by(|left, right| {
            let order = left
                .receiver
                .canonical_cmp(&right.receiver, &definitions, &self.options.cancel)
                .and_then(|order| {
                    if order.is_eq() {
                        Ty::Trait(left.interface.clone()).canonical_cmp(
                            &Ty::Trait(right.interface.clone()),
                            &definitions,
                            &self.options.cancel,
                        )
                    } else {
                        Ok(order)
                    }
                });
            order.unwrap_or_else(|error| {
                failure = Some(error);
                Ordering::Equal
            })
        });
        if failure.is_some() {
            return Err(MirLoweringError::MissingBinding(
                "construction requirement ordering",
            ));
        }
        requirements.dedup();
        self.operation_depth += 1;
        let result = requirements
            .into_iter()
            .map(|required| self.bind_construction(required, span))
            .collect();
        self.operation_depth -= 1;
        result
    }

    fn bind_construction(
        &mut self,
        required: ConstructionRequirement,
        span: Span,
    ) -> Result<ConstructionWitness, MirLoweringError> {
        self.check()?;
        let invalid = || MirLoweringError::MissingBinding("selected interface construction");
        let receiver = raise_type(&required.receiver);
        let interface = raise_nominal_type(&required.interface);
        if let TypeId::Trait(source) = &receiver
            && self
                .catalog
                .interface_closure(&self.definitions(), source, &receiver, &self.options.cancel)
                .map_err(|_| invalid())?
                .iter()
                .any(|view| view.satisfies(&interface))
        {
            return Ok(ConstructionWitness {
                requirement: required,
                selection: ConstructionSelection::Interface,
            });
        }
        if matches!(
            receiver,
            TypeId::Generic(_) | TypeId::Projection { .. } | TypeId::SelfType(_)
        ) {
            return Ok(ConstructionWitness {
                requirement: required,
                selection: ConstructionSelection::Forward,
            });
        }
        let mut selected = self
            .catalog
            .concrete_interface_implementation(
                &self.definitions(),
                &interface,
                &receiver,
                &Default::default(),
                ImplementationSearchLimits {
                    max_checks: 100_000,
                    max_depth: 64,
                },
                &self.options.cancel,
            )
            .map_err(|_| invalid())?;
        if selected.is_none() && !receiver.is_concrete() {
            selected = self.implementation_candidate(&required.receiver, &required.interface)?;
        }
        let selection = if let Some((declaration, arguments)) = selected {
            let evidence = self.implementation_evidence(&declaration, &arguments, span)?;
            self.record_interface(&declaration, &arguments, span)?;
            ConstructionSelection::Declared {
                implementation: ConcreteFunctionIdentity {
                    declaration,
                    arguments: arguments.iter().map(|ty| self.encode_type(ty)).collect(),
                },
                operations: evidence.operations,
                constructions: evidence.constructions,
            }
        } else if self.catalog.intrinsic_implementation(
            &self.definitions(),
            &interface,
            &receiver,
            &Default::default(),
        ) {
            let declaration = self.intrinsic_interface_bridge(&receiver, &interface, span)?;
            ConstructionSelection::Declared {
                implementation: ConcreteFunctionIdentity {
                    declaration,
                    arguments: vec![],
                },
                operations: vec![],
                constructions: vec![],
            }
        } else if !receiver.is_concrete() {
            ConstructionSelection::Forward
        } else {
            return Err(invalid());
        };
        Ok(ConstructionWitness {
            requirement: required,
            selection,
        })
    }
}
