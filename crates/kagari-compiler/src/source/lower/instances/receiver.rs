//! Select receiver comparison while the checked source catalog is available.
use crate::source::lower::{MirLoweringError, abi::checked_bounds, instances::InstancePlanner};
use kagari_common::{identity::DefinitionKind, span::Span};
use kagari_contract::{
    callable::{
        receiver::{ReceiverComparison, ReceiverEligibility, object_receiver},
        witness::{OperationWitness, SharedMethodWitness},
    },
    types::ConcreteFunctionIdentity,
};
use kagari_hir::{
    aggregates::implementations::ImplementationSearchLimits,
    language::semantics::ProtocolSemantics,
    typeck::{GenericBounds, constraints::type_satisfies_standard_constraint},
    types::{
        TypeId,
        semantic::{raise_nominal_type, raise_type},
    },
};
use kagari_types::{
    declaration::requirement::NativeCallableRequirement,
    language::Protocol,
    ty::{Constraint, GenericBound, Ty, substitution::TypeSubstitution},
};

impl InstancePlanner<'_> {
    pub(super) fn receiver_comparison(
        &mut self,
        receiver: &TypeId,
        assumptions: &GenericBounds,
        span: Span,
    ) -> Result<Option<ReceiverComparison>, MirLoweringError> {
        let ty = self.encode_type(receiver);
        if !object_receiver(&ty) {
            return Ok(None);
        }
        let definitions = self.definitions();
        let interface = Protocol::PartialEq
            .nominal(&definitions)
            .ok_or(MirLoweringError::MissingBinding("object equality protocol"))?;
        let conditional = !ty.is_concrete()
            && self
                .catalog
                .concrete_interface_implementation(
                    &definitions,
                    &interface,
                    receiver,
                    assumptions,
                    ImplementationSearchLimits {
                        max_checks: 100_000,
                        max_depth: 64,
                    },
                    &self.options.cancel,
                )
                .map_err(|_| MirLoweringError::MissingBinding("receiver comparison bounds"))?
                .is_none();
        let member = definitions
            .lookup_child(interface.declaration, DefinitionKind::Method, "eq", 0)
            .ok_or(MirLoweringError::MissingBinding("object equality member"))?;
        let required = NativeCallableRequirement {
            receiver: ty,
            interface: self.encode_nominal(&interface),
            member,
            arguments: vec![],
        };
        let candidate = self.receiver_conditions(&required)?;
        let conditions = if conditional {
            let Some(conditions) = candidate.as_ref() else {
                return Ok(None);
            };
            conditions.clone()
        } else {
            vec![]
        };
        let eligibility = if required.receiver.is_concrete() {
            self.receiver_eligibility(candidate.as_deref().unwrap_or_default())?
        } else {
            vec![]
        };
        let operation = self.receiver_operation(required, span)?;
        // Conditional selections retain exact applicability guards. They cannot
        // execute until the actual argument eligibility is supplied separately.
        if matches!(operation, OperationWitness::Forward(_)) {
            return Ok(None);
        }
        Ok(Some(ReceiverComparison {
            operation,
            conditions,
            eligibility,
        }))
    }

    fn receiver_operation(
        &mut self,
        required: NativeCallableRequirement,
        span: Span,
    ) -> Result<OperationWitness, MirLoweringError> {
        // Closed generic comparisons retain their bound operations as well as
        // their selected method. A method may not use a bound in its body, but
        // later mixed-generation applications still need that checked evidence.
        if required.receiver.is_concrete()
            && let Some((declaration, arguments)) = self
                .catalog
                .concrete_interface_implementation(
                    &self.definitions(),
                    &raise_nominal_type(&required.interface),
                    &raise_type(&required.receiver),
                    &Default::default(),
                    ImplementationSearchLimits {
                        max_checks: 100_000,
                        max_depth: 64,
                    },
                    &self.options.cancel,
                )
                .map_err(|_| MirLoweringError::MissingBinding("receiver comparison selection"))?
            && self
                .catalog
                .implementation_signature(&declaration)
                .is_some_and(|implementation| !implementation.generic_params.is_empty())
        {
            let operations = self.implementation_operations(&declaration, &arguments, span)?;
            self.record_interface(&declaration, &arguments, span)?;
            return Ok(OperationWitness::SharedMethod(Box::new(
                SharedMethodWitness {
                    operations,
                    requirement: required,
                    implementation: ConcreteFunctionIdentity {
                        declaration,
                        arguments: arguments.iter().map(|ty| self.encode_type(ty)).collect(),
                    },
                },
            )));
        }
        self.bind_operations(vec![required], span)?
            .pop()
            .ok_or(MirLoweringError::MissingBinding(
                "selected receiver comparison",
            ))
    }

    fn receiver_conditions(
        &self,
        required: &NativeCallableRequirement,
    ) -> Result<Option<Vec<GenericBound>>, MirLoweringError> {
        let Some((declaration, arguments)) = self.operation_candidate(required)? else {
            return Ok(None);
        };
        let implementation = self.catalog.implementation_signature(&declaration).ok_or(
            MirLoweringError::MissingBinding("conditional receiver implementation"),
        )?;
        let definitions = self.definitions();
        let bounds = checked_bounds(self.catalog, &definitions, &implementation.bounds);
        let arguments = arguments
            .iter()
            .map(|argument| self.encode_type(argument))
            .collect::<Vec<_>>();
        let mut bindings = TypeSubstitution::default();
        for (parameter, argument) in implementation.generic_params.iter().zip(&arguments) {
            bindings.bind(&parameter.owner, parameter.position, argument);
        }
        bindings
            .apply_bounds(&bounds, &definitions, &self.options.cancel)
            .map(Some)
            .map_err(|_| MirLoweringError::MissingBinding("conditional receiver bounds"))
    }

    fn receiver_eligibility(
        &self,
        bounds: &[GenericBound],
    ) -> Result<Vec<ReceiverEligibility>, MirLoweringError> {
        let definitions = self.definitions();
        let mut pending = bounds
            .iter()
            .flat_map(|bound| {
                bound
                    .constraints
                    .iter()
                    .map(|constraint| (bound.ty.clone(), constraint.clone(), 0usize))
            })
            .collect::<Vec<_>>();
        let mut facts: Vec<ReceiverEligibility> = vec![];
        while let Some((ty, constraint, depth)) = pending.pop() {
            self.check()?;
            if depth >= 64 || facts.len() >= 4096 {
                return Err(MirLoweringError::MissingBinding(
                    "receiver eligibility limit",
                ));
            }
            let bound = GenericBound {
                ty,
                constraints: vec![constraint.clone()],
            };
            if facts.iter().any(|fact| fact.bound == bound) {
                continue;
            }
            let actual = raise_type(&bound.ty);
            let satisfied = match &constraint {
                Constraint::Standard(kind) => type_satisfies_standard_constraint(
                    &definitions,
                    &actual,
                    *kind,
                    &Default::default(),
                    Some(self.catalog),
                ),
                Constraint::Trait(interface) => {
                    let interface = raise_nominal_type(interface);
                    let selected = self
                        .catalog
                        .concrete_interface_implementation(
                            &definitions,
                            &interface,
                            &actual,
                            &Default::default(),
                            ImplementationSearchLimits {
                                max_checks: 100_000,
                                max_depth: 64,
                            },
                            &self.options.cancel,
                        )
                        .map_err(|_| {
                            MirLoweringError::MissingBinding("receiver eligibility proof")
                        })?;
                    selected.is_some()
                        || self.catalog.intrinsic_implementation(
                            &definitions,
                            &interface,
                            &actual,
                            &Default::default(),
                        )
                }
            };
            // Structural tuple eligibility is component-wise for these fixed
            // language protocols. Retain each closed component's own outcome.
            if let (Ty::Tuple(items), Constraint::Trait(interface)) = (&bound.ty, &constraint)
                && matches!(
                    Protocol::from_definition(&interface.declaration, &definitions),
                    Some(Protocol::PartialEq | Protocol::Eq | Protocol::Hash)
                )
                && interface.arguments.is_empty()
                && interface.associated_types.is_empty()
            {
                pending.extend(
                    items
                        .iter()
                        .map(|item| (item.clone(), constraint.clone(), depth + 1)),
                );
            }
            facts.push(ReceiverEligibility { bound, satisfied });
        }
        Ok(facts)
    }
}
