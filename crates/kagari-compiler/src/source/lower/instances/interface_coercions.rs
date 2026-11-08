//! Select interface construction evidence and its separately bounded view.
use crate::source::lower::{MirLoweringError, instances::InstancePlanner};
use kagari_common::span::Span;
use kagari_contract::callable::construction::evidence::{
    ConstructionRequirement, ConstructionSelection,
};
use kagari_hir::{
    typeck::table::{ResolvedInterfaceCoercion, ResolvedInterfaceImplementation},
    types::{
        NominalType, TypeId, TypeSubstitution,
        semantic::{raise_nominal_type, raise_type},
    },
};
use kagari_mir::instruction::ConstructionTarget;
use kagari_types::ty::{NominalTy, Ty};

pub(crate) struct InterfacePlan {
    pub receiver: Ty,
    pub construction: Option<ConstructionTarget>,
    pub source: NominalTy,
    pub target: NominalTy,
}

impl InstancePlanner<'_> {
    /// Construction proofs keep exact implementation outputs; executable views
    /// additionally fill omitted associated outputs such as Iterable::Iter.
    fn interface_value(&self, interface: &NominalType) -> Result<NominalTy, MirLoweringError> {
        let Ty::Trait(value) = self.encode_type(&TypeId::Trait(interface.clone())) else {
            return Err(MirLoweringError::MissingBinding("interface value type"));
        };
        Ok(value)
    }

    pub(crate) fn interface_coercion(
        &mut self,
        mut coercion: ResolvedInterfaceCoercion,
        substitution: &TypeSubstitution,
        span: Span,
    ) -> Result<InterfacePlan, MirLoweringError> {
        let selected_from_bound = matches!(
            coercion.implementation,
            ResolvedInterfaceImplementation::Bound { .. }
        );
        if let ResolvedInterfaceImplementation::Bound { interface: proven } =
            &coercion.implementation
        {
            let types = self.arguments(
                &[
                    coercion.concrete_type.clone(),
                    TypeId::Trait(proven.clone()),
                ],
                substitution,
                span,
            )?;
            let TypeId::Trait(interface) = &types[1] else {
                return Err(MirLoweringError::MissingBinding(
                    "bound construction target",
                ));
            };
            let requirement = ConstructionRequirement {
                receiver: self.encode_type(&types[0]),
                interface: self.encode_nominal(interface),
            };
            let selected = self
                .bind_constructions(vec![requirement], span)?
                .pop()
                .ok_or(MirLoweringError::MissingBinding(
                    "bound construction selection",
                ))?;
            coercion.implementation = match selected.selection {
                ConstructionSelection::Declared { implementation, .. } => {
                    ResolvedInterfaceImplementation::Declared {
                        declaration: implementation.declaration,
                        arguments: implementation.arguments.iter().map(raise_type).collect(),
                    }
                }
                ConstructionSelection::Interface => ResolvedInterfaceImplementation::Upcast,
                ConstructionSelection::Forward => {
                    let target = self.arguments(
                        &[TypeId::Trait(coercion.interface_type)],
                        substitution,
                        span,
                    )?;
                    let Ty::Trait(target) = self.encode_type(&target[0]) else {
                        return Err(MirLoweringError::MissingBinding("bound construction view"));
                    };
                    return Ok(InterfacePlan {
                        receiver: selected.requirement.receiver.clone(),
                        source: self.interface_value(&raise_nominal_type(
                            &selected.requirement.interface,
                        ))?,
                        target,
                        construction: Some(ConstructionTarget::Bound(selected.requirement)),
                    });
                }
            };
            // Synthesized intrinsic tables have no HIR implementation declaration.
            if let ResolvedInterfaceImplementation::Declared { declaration, .. } =
                &coercion.implementation
                && self.catalog.implementation_signature(declaration).is_none()
            {
                coercion.implementation = ResolvedInterfaceImplementation::Intrinsic;
            }
        }
        if matches!(
            coercion.implementation,
            ResolvedInterfaceImplementation::Upcast
        ) {
            let types = self.arguments(
                &[
                    coercion.concrete_type,
                    TypeId::Trait(coercion.interface_type),
                ],
                substitution,
                span,
            )?;
            let [Ty::Trait(source), Ty::Trait(target)] =
                [self.encode_type(&types[0]), self.encode_type(&types[1])]
            else {
                return Err(MirLoweringError::MissingBinding("interface upcast types"));
            };
            return Ok(InterfacePlan {
                receiver: Ty::Trait(source.clone()),
                source,
                target,
                construction: None,
            });
        }
        let types = self.arguments(
            &[
                coercion.concrete_type.clone(),
                TypeId::Trait(coercion.interface_type.clone()),
            ],
            substitution,
            span,
        )?;
        let TypeId::Trait(interface) = &types[1] else {
            return Err(MirLoweringError::MissingBinding("interface demand type"));
        };
        // Boxing changes representation; retain the checked input type
        // before replacing the expression's register with the interface.
        self.require_parent_interfaces(&types[0], interface, &Default::default(), span)?;
        let (implementation, arguments, constructed, operations, constructions) =
            match coercion.implementation {
                ResolvedInterfaceImplementation::Upcast => unreachable!("upcast handled above"),
                ResolvedInterfaceImplementation::Bound { .. } => {
                    unreachable!("bound construction selected above")
                }
                ResolvedInterfaceImplementation::Intrinsic => {
                    let mut types = self
                        .arguments(
                            &[
                                coercion.concrete_type,
                                TypeId::Trait(coercion.interface_type),
                            ],
                            substitution,
                            span,
                        )?
                        .into_iter();
                    let receiver = types.next().expect("receiver argument");
                    let applied = types.next().expect("interface argument");
                    let TypeId::Trait(interface) = applied else {
                        return Err(MirLoweringError::MissingBinding("intrinsic interface type"));
                    };
                    (
                        self.intrinsic_interface_bridge(&receiver, &interface, span)?,
                        Vec::new(),
                        interface,
                        Vec::new(),
                        Vec::new(),
                    )
                }
                ResolvedInterfaceImplementation::Declared {
                    declaration,
                    arguments,
                } => {
                    let arguments = if selected_from_bound {
                        arguments
                    } else {
                        self.arguments(&arguments, substitution, span)?
                    };
                    let evidence = self.implementation_evidence(&declaration, &arguments, span)?;
                    self.record_interface(&declaration, &arguments, span)?;
                    let contract = self.catalog.implementation_signature(&declaration).ok_or(
                        MirLoweringError::MissingBinding("interface construction contract"),
                    )?;
                    let substitution: TypeSubstitution = contract
                        .generic_params
                        .iter()
                        .cloned()
                        .zip(arguments.iter().cloned())
                        .collect();
                    let constructed = contract.trait_type.instantiate(&substitution);
                    (
                        declaration,
                        arguments.iter().map(|ty| self.encode_type(ty)).collect(),
                        constructed,
                        evidence.operations,
                        evidence.constructions,
                    )
                }
            };
        let source = self.interface_value(&constructed)?;
        let target = self.interface_value(interface)?;
        Ok(InterfacePlan {
            receiver: self.encode_type(&types[0]),
            source,
            target,
            construction: Some(ConstructionTarget::Declared {
                implementation,
                arguments,
                operations,
                constructions,
            }),
        })
    }
}
