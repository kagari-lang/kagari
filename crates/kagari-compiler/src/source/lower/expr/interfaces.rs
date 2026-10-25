//! Lower checked interface coercions through declared, bound or existing-view evidence.
use crate::source::lower::{MirLoweringError, state::FunctionLowerer};
use kagari_abi::representation::ValueType;
use kagari_contract::callable::construction::evidence::{
    ConstructionRequirement, ConstructionSelection,
};
use kagari_hir::{
    hir::ids::ExprId,
    typeck::table::{ResolvedInterfaceCoercion, ResolvedInterfaceImplementation},
    types::{TypeId, TypeSubstitution, semantic::raise_type},
};
use kagari_mir::instruction::{ConstructionTarget, Instruction, MirValue};
use kagari_types::ty::Ty;

impl FunctionLowerer<'_, '_> {
    pub(super) fn lower_interface_coercion(
        &mut self,
        expr_id: ExprId,
        mut value: MirValue,
        mut coercion: ResolvedInterfaceCoercion,
    ) -> Result<MirValue, MirLoweringError> {
        let span = self.analyzed.lowered.source_map.expr_span(expr_id);

        let selected_from_bound = matches!(
            coercion.implementation,
            ResolvedInterfaceImplementation::Bound { .. }
        );
        if let ResolvedInterfaceImplementation::Bound { interface: proven } =
            &coercion.implementation
        {
            let types = self.planner.arguments(
                &[
                    coercion.concrete_type.clone(),
                    TypeId::Trait(proven.clone()),
                ],
                &self.instance.substitution,
                span,
            )?;
            let TypeId::Trait(interface) = &types[1] else {
                return Err(MirLoweringError::MissingBinding(
                    "bound construction target",
                ));
            };
            let requirement = ConstructionRequirement {
                receiver: self.planner.encode_type(&types[0]),
                interface: self.planner.encode_nominal(interface),
            };
            let selected = self
                .planner
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
                    self.function
                        .semantic
                        .registers
                        .insert(value.temp.index(), selected.requirement.receiver.clone());
                    let source = selected.requirement.interface.clone();
                    let dst = self.alloc_temp(ValueType::HeapObject);
                    self.emit(Instruction::MakeInterface {
                        dst,
                        value,
                        target: ConstructionTarget::Bound(selected.requirement),
                    });
                    self.function
                        .semantic
                        .registers
                        .insert(dst.temp.index(), Ty::Trait(source.clone()));
                    let target = self.planner.arguments(
                        &[TypeId::Trait(coercion.interface_type)],
                        &self.instance.substitution,
                        span,
                    )?;
                    let Ty::Trait(target) = self.planner.encode_type(&target[0]) else {
                        return Err(MirLoweringError::MissingBinding("bound construction view"));
                    };
                    if source == target {
                        return Ok(dst);
                    }
                    let view = self.alloc_temp(ValueType::HeapObject);
                    self.emit(Instruction::UpcastInterface {
                        dst: view,
                        value: dst,
                        source,
                        target,
                    });
                    return Ok(view);
                }
            };
            // Synthesized intrinsic tables have no HIR implementation declaration.
            if let ResolvedInterfaceImplementation::Declared { declaration, .. } =
                &coercion.implementation
                && self
                    .planner
                    .catalog
                    .implementation_signature(declaration)
                    .is_none()
            {
                coercion.implementation = ResolvedInterfaceImplementation::Intrinsic;
            }
        }
        if matches!(
            coercion.implementation,
            ResolvedInterfaceImplementation::Upcast
        ) {
            let types = self.planner.arguments(
                &[
                    coercion.concrete_type,
                    TypeId::Trait(coercion.interface_type),
                ],
                &self.instance.substitution,
                span,
            )?;
            let [Ty::Trait(source), Ty::Trait(target)] = [
                self.planner.encode_type(&types[0]),
                self.planner.encode_type(&types[1]),
            ] else {
                return Err(MirLoweringError::MissingBinding("interface upcast types"));
            };
            let dst = self.alloc_temp(ValueType::HeapObject);
            self.emit(Instruction::UpcastInterface {
                dst,
                value,
                source,
                target,
            });
            return Ok(dst);
        }
        let types = self.planner.arguments(
            &[
                coercion.concrete_type.clone(),
                TypeId::Trait(coercion.interface_type.clone()),
            ],
            &self.instance.substitution,
            span,
        )?;
        let TypeId::Trait(interface) = &types[1] else {
            return Err(MirLoweringError::MissingBinding("interface demand type"));
        };
        // Boxing changes representation; retain the checked input type
        // before replacing the expression's register with the interface.
        self.function
            .semantic
            .registers
            .insert(value.temp.index(), self.planner.encode_type(&types[0]));
        self.planner
            .require_parent_interfaces(&types[0], interface, &Default::default(), span)?;
        let (implementation, arguments, constructed, operations, constructions) =
            match coercion.implementation {
                ResolvedInterfaceImplementation::Upcast => unreachable!("upcast handled above"),
                ResolvedInterfaceImplementation::Bound { .. } => {
                    unreachable!("bound construction selected above")
                }
                ResolvedInterfaceImplementation::Intrinsic => {
                    let mut types = self
                        .planner
                        .arguments(
                            &[
                                coercion.concrete_type,
                                TypeId::Trait(coercion.interface_type),
                            ],
                            &self.instance.substitution,
                            span,
                        )?
                        .into_iter();
                    let receiver = types.next().expect("receiver argument");
                    let applied = types.next().expect("interface argument");
                    let TypeId::Trait(interface) = applied else {
                        return Err(MirLoweringError::MissingBinding("intrinsic interface type"));
                    };
                    (
                        self.planner
                            .intrinsic_interface_bridge(&receiver, &interface, span)?,
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
                        self.planner
                            .arguments(&arguments, &self.instance.substitution, span)?
                    };
                    let evidence =
                        self.planner
                            .implementation_evidence(&declaration, &arguments, span)?;
                    self.planner
                        .record_interface(&declaration, &arguments, span)?;
                    let contract = self
                        .planner
                        .catalog
                        .implementation_signature(&declaration)
                        .ok_or(MirLoweringError::MissingBinding(
                            "interface construction contract",
                        ))?;
                    let substitution: TypeSubstitution = contract
                        .generic_params
                        .iter()
                        .cloned()
                        .zip(arguments.iter().cloned())
                        .collect();
                    let constructed = contract.trait_type.instantiate(&substitution);
                    (
                        declaration,
                        arguments
                            .iter()
                            .map(|ty| self.planner.encode_type(ty))
                            .collect(),
                        constructed,
                        evidence.operations,
                        evidence.constructions,
                    )
                }
            };
        let dst = self.alloc_temp(ValueType::HeapObject);
        self.emit(Instruction::MakeInterface {
            dst,
            value,
            target: ConstructionTarget::Declared {
                implementation,
                arguments,
                operations,
                constructions,
            },
        });
        value = dst;
        // The table constructs its exact outputs. The value's bounded
        // iterator-result view is a separate checked adaptation.
        let source = self.planner.encode_nominal(&constructed);
        let Ty::Trait(target) = self.planner.encode_type(&TypeId::Trait(interface.clone())) else {
            unreachable!("interface value type")
        };
        if source != target {
            self.function
                .semantic
                .registers
                .insert(value.temp.index(), Ty::Trait(source.clone()));
            let dst = self.alloc_temp(ValueType::HeapObject);
            self.emit(Instruction::UpcastInterface {
                dst,
                value,
                source,
                target,
            });
            value = dst;
        }
        Ok(value)
    }
}
