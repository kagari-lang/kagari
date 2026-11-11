//! Instantiate assumptions and select implementations within the compilation budget.
use crate::source::lower::{MirLoweringError, instances::InstancePlanner};
use kagari_common::identity::table::DefinitionId;
use kagari_hir::{
    aggregates::implementations::{ImplementationSearchContext, ImplementationSearchLimits},
    language::semantics::ProtocolSemantics,
    typeck::GenericBounds,
    types::{NominalType, TypeId, TypeSubstitution},
};
use kagari_types::{declaration::requirement::NativeCallableRequirement, language::Protocol};
use std::collections::HashSet;

/// Select only the recursive composition that needs script-aware equality/hash.
pub(crate) enum ProtocolPlan {
    Primitive,
    Method {
        interface: NominalType,
        method: DefinitionId,
    },
    Tuple(Vec<TypeId>),
    Enum(Vec<(usize, Vec<TypeId>)>),
    Runtime,
}

impl InstancePlanner<'_> {
    pub(crate) fn protocol_adapter_result(
        &self,
        interface: &NominalType,
        receiver: &TypeId,
    ) -> Result<TypeId, MirLoweringError> {
        let method = self
            .catalog
            .trait_(&interface.declaration)
            .and_then(|contract| contract.methods.first())
            .ok_or(MirLoweringError::MissingBinding(
                "iteration adapter contract",
            ))?;
        Ok(self.catalog.normalize_type(
            &self.definitions(),
            &method
                .return_type
                .with_self(&method.owner, receiver)
                .with_associated_types(interface, receiver),
        ))
    }

    pub(crate) fn has_protocol_application(
        &self,
        required: &NativeCallableRequirement,
        receiver: &TypeId,
        interface: &NominalType,
    ) -> Result<bool, MirLoweringError> {
        Ok(self
            .catalog
            .implicit_protocol_application(
                &self.definitions(),
                required,
                receiver,
                interface,
                &self.options.cancel,
            )
            .map_err(|_| MirLoweringError::MissingBinding("checked protocol adapter"))?
            .is_some())
    }

    pub(crate) fn protocol_plan(
        &self,
        protocol: Protocol,
        ty: &TypeId,
        bounds: &GenericBounds,
    ) -> Result<ProtocolPlan, MirLoweringError> {
        let dynamic = if let TypeId::Trait(child) = ty {
            self.catalog
                .interface_closure(&self.definitions(), child, ty, &self.options.cancel)?
                .iter()
                .any(|parent| {
                    Protocol::from_definition(&parent.declaration, &self.definitions())
                        == Some(protocol)
                })
        } else {
            false
        };
        if !matches!(ty, TypeId::Generic(_)) && !dynamic && !self.has_custom_protocol(ty, bounds)? {
            return Ok(ProtocolPlan::Primitive);
        }
        let interface =
            protocol
                .nominal(&self.definitions())
                .ok_or(MirLoweringError::MissingBinding(
                    "generic equality protocol",
                ))?;
        let method = self
            .catalog
            .trait_(&interface.declaration)
            .and_then(|contract| contract.methods.first())
            .map(|method| method.id)
            .ok_or(MirLoweringError::MissingBinding("checked protocol method"))?;
        if matches!(ty, TypeId::Generic(_))
            || dynamic
            || self
                .implementation_method(&method, &interface, ty, bounds)?
                .is_some()
        {
            return Ok(ProtocolPlan::Method { interface, method });
        }
        Ok(match ty {
            TypeId::Tuple(members) => ProtocolPlan::Tuple(members.clone()),
            TypeId::Enum(nominal) => {
                let contract = self
                    .catalog
                    .enumeration(&nominal.declaration)
                    .ok_or(MirLoweringError::MissingBinding("enum protocol contract"))?;
                let substitution = contract
                    .generic_params
                    .iter()
                    .cloned()
                    .zip(nominal.arguments.iter().cloned())
                    .collect();
                ProtocolPlan::Enum(
                    contract
                        .variants
                        .iter()
                        .map(|variant| {
                            (
                                variant.slot,
                                variant
                                    .payload
                                    .iter()
                                    .map(|ty| ty.instantiate(&substitution))
                                    .collect(),
                            )
                        })
                        .collect(),
                )
            }
            _ => ProtocolPlan::Runtime,
        })
    }

    pub(crate) fn implementation_bounds(
        &self,
        scope: &GenericBounds,
        substitution: &TypeSubstitution,
    ) -> Result<GenericBounds, MirLoweringError> {
        let mut bounds = GenericBounds::new();
        for (ty, constraints) in scope {
            let actual = self
                .catalog
                .normalize_type(&self.definitions(), &ty.instantiate(substitution));
            bounds.entry(actual).or_default().extend(
                constraints
                    .iter()
                    .map(|interface| interface.instantiate(substitution)),
            );
        }
        Ok(self
            .catalog
            .expanded_bounds(&bounds, &self.options.cancel)?)
    }

    pub(crate) fn implementation_method(
        &self,
        method: &DefinitionId,
        interface: &NominalType,
        ty: &TypeId,
        bounds: &GenericBounds,
    ) -> Result<Option<(DefinitionId, Vec<TypeId>)>, MirLoweringError> {
        Ok(self.catalog.implementation_method(
            &self.definitions(),
            method,
            interface,
            ty,
            ImplementationSearchContext {
                assumptions: bounds,
                limits: ImplementationSearchLimits {
                    max_checks: self.options.max_type_nodes,
                    max_depth: self.options.max_type_depth,
                },
                cancel: &self.options.cancel,
            },
        )?)
    }

    fn has_custom_protocol(
        &self,
        ty: &TypeId,
        bounds: &GenericBounds,
    ) -> Result<bool, MirLoweringError> {
        let mut pending = vec![ty.clone()];
        let mut seen = HashSet::new();
        while let Some(ty) = pending.pop() {
            self.check()?;
            if !seen.insert(ty.clone()) {
                continue;
            }
            if seen.len() > self.options.max_type_nodes {
                return Err(MirLoweringError::UnsupportedExpr(
                    "protocol composition exceeds type node limit",
                ));
            }
            match &ty {
                TypeId::Trait(_) | TypeId::Generic(_) => return Ok(true),
                TypeId::Array(_)
                | TypeId::NativeObject(_)
                | TypeId::Struct(_)
                | TypeId::Enum(_) => {
                    // Primitive member composition needs no method import.
                    // A checked explicit implementation requires its callable.
                    if self
                        .catalog
                        .concrete_interface_implementation(
                            &self.definitions(),
                            &Protocol::PartialEq
                                .nominal(&self.definitions())
                                .expect("registered protocol identity"),
                            &ty,
                            bounds,
                            ImplementationSearchLimits {
                                max_checks: self.options.max_type_nodes,
                                max_depth: self.options.max_type_depth,
                            },
                            &self.options.cancel,
                        )?
                        .is_some()
                    {
                        return Ok(true);
                    }
                    if let TypeId::Enum(n) = &ty
                        && let Some(contract) = self.catalog.enumeration(&n.declaration)
                    {
                        let substitution = contract
                            .generic_params
                            .iter()
                            .cloned()
                            .zip(n.arguments.iter().cloned())
                            .collect();
                        pending.extend(
                            contract.variants.iter().flat_map(|v| {
                                v.payload.iter().map(|t| t.instantiate(&substitution))
                            }),
                        );
                    }
                }
                TypeId::Tuple(members) => pending.extend(members.iter().cloned()),
                _ => {}
            }
        }
        Ok(false)
    }
}
