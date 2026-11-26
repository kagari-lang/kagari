//! Local checked static-expression facts, without transitive dependency protection.

use crate::{
    CheckedAnalysis,
    collections::HashMap,
    declarations::DeclarationId,
    hir::{
        expr::ExprKind,
        ids::{ExprId, HirOwner},
    },
    resolver::resolved::ResolvedName,
    typeck::table::{
        CallTarget, ResolvedCall, ResolvedInterfaceImplementation, ResolvedTypeTarget,
    },
    types::semantic::{lower_nominal_type, lower_type},
    update_source::{BodyBuilder, FactKind, SourceBudget, definition},
};
use kagari_common::{identity::mapping::DefinitionMappingError, span::Span};
use kagari_types::ty::{GenericParam, Ty};

impl BodyBuilder {
    fn reference(
        &mut self,
        module: &CheckedAnalysis,
        span: Span,
        name: ResolvedName,
        budget: &mut SourceBudget<'_>,
    ) -> Result<(), DefinitionMappingError> {
        let target = module.declarations.target(name).or_else(|| {
            if let ResolvedName::Source(source) = &name {
                module
                    .imported_functions
                    .get(name)
                    .map(|function| &function.site)
                    .or_else(|| module.imported_constants.get(source).map(|item| &item.site))
                    .or_else(|| module.imported_statics.get(source).map(|item| &item.site))
                    .or_else(|| {
                        module
                            .declarations
                            .imported_types()
                            .target(*source)
                            .map(|item| &item.declaration)
                    })
            } else {
                None
            }
        });
        if let Some(target) = target {
            match target.id {
                DeclarationId::Definition(id) => {
                    return self.fact(span, FactKind::Reference, vec![id], vec![], budget);
                }
                DeclarationId::GenericParameter { owner, position } => {
                    return self.fact(
                        span,
                        FactKind::Type,
                        vec![],
                        vec![GenericParam { owner, position }.as_type()],
                        budget,
                    );
                }
                DeclarationId::Binding(_) => {}
            }
        }
        let (binding, ty) = match name {
            ResolvedName::Local(id) => (
                module
                    .lowered
                    .module
                    .nodes
                    .local(id)
                    .origin
                    .diagnostic_span(),
                module.typed.type_table.local_type(id),
            ),
            ResolvedName::Param(id) => (
                module.lowered.module.parameter(id).binding_span(),
                module
                    .typed
                    .signatures
                    .parameter(id)
                    .map(|param| param.ty.clone()),
            ),

            ResolvedName::Namespace(_) | ResolvedName::Module(_) => return Ok(()),
            _ => return Err(DefinitionMappingError::InvalidContract),
        };
        self.fact(
            span,
            FactKind::Binding,
            vec![],
            ty.as_ref().map(lower_type).into_iter().collect(),
            budget,
        )?;
        let related = self.site(binding);
        self.body
            .facts
            .last_mut()
            .expect("inserted binding")
            .related
            .push(related);
        Ok(())
    }

    fn call(
        &mut self,
        module: &CheckedAnalysis,
        span: Span,
        call: &ResolvedCall,
        value: bool,
        budget: &mut SourceBudget<'_>,
    ) -> Result<(), DefinitionMappingError> {
        let mut types: Vec<_> = call.type_arguments.iter().map(lower_type).collect();
        let (kind, targets) = match &call.target {
            CallTarget::SourceFunction(id) => (
                if value {
                    FactKind::FunctionValue
                } else {
                    FactKind::Call
                },
                vec![*id],
            ),
            CallTarget::Function(id) => (
                if value {
                    FactKind::FunctionValue
                } else {
                    FactKind::Call
                },
                vec![definition(module, ResolvedName::Function(*id))?],
            ),
            CallTarget::TraitMethod { method, interface } => {
                let receiver = call
                    .receiver
                    .and_then(|id| module.typed.type_table.expr_type(id));
                types.insert(0, Ty::Trait(lower_nominal_type(interface)));
                if let Some(receiver) = receiver {
                    types.insert(0, lower_type(&receiver));
                }

                (FactKind::TraitCall, vec![*method])
            }
            CallTarget::Value => (FactKind::Call, vec![]),
            CallTarget::TerminatingCallee => return Ok(()),
        };
        self.fact(span, kind, targets, types, budget)?;
        if let Some(signature) = &call.signature {
            self.fact(
                span,
                FactKind::Signature,
                vec![],
                signature
                    .params
                    .iter()
                    .map(lower_type)
                    .chain([lower_type(&signature.return_type)])
                    .collect(),
                budget,
            )?;
        }
        Ok(())
    }

    fn expression(
        &mut self,
        module: &CheckedAnalysis,
        id: ExprId,
        kind: &ExprKind,
        budget: &mut SourceBudget<'_>,
    ) -> Result<(), DefinitionMappingError> {
        let table = &module.typed.type_table;
        let span = module.lowered.module.expr(id).origin.diagnostic_span();
        if let Some(ty) = table.expr_type(id) {
            self.fact(span, FactKind::Type, vec![], vec![lower_type(&ty)], budget)?;
        }
        if let Some(name) = module.names.expr_resolution(id) {
            self.reference(module, span, name, budget)?;
        }
        if let Some(call) = table.call_resolution(id) {
            self.call(
                module,
                span,
                &call,
                matches!(kind, ExprKind::Name { .. }),
                budget,
            )?;
        }
        if let Some(receiver) = table.protocol_receiver(id) {
            self.fact(
                span,
                FactKind::Protocol,
                vec![],
                vec![lower_type(receiver)],
                budget,
            )?;
        }
        if let Some(constant) = table.associated_const(id) {
            self.fact(
                span,
                FactKind::AssociatedConstant,
                vec![constant.member],
                vec![
                    lower_type(&constant.receiver),
                    Ty::Trait(lower_nominal_type(&constant.interface)),
                ],
                budget,
            )?;
        }
        if let Some(field) = table.expr_field(id) {
            self.fact(span, FactKind::Member, vec![*field], vec![], budget)?;
        }
        if let Some(init) = table.struct_init(id) {
            self.fact(
                span,
                FactKind::Construction,
                [init.structure]
                    .into_iter()
                    .chain(init.fields.iter().flatten().copied())
                    .chain(init.base_fields.iter().copied())
                    .collect(),
                vec![],
                budget,
            )?;
        }
        if let Some(init) = table.enum_constructor(id) {
            self.fact(
                span,
                FactKind::Construction,
                [init.enumeration].into_iter().chain(init.variant).collect(),
                vec![],
                budget,
            )?;
        }
        if let Some(iteration) = table.iteration(id) {
            self.fact(
                span,
                FactKind::Protocol,
                vec![],
                vec![
                    Ty::Trait(lower_nominal_type(&iteration.into_interface)),
                    lower_type(&iteration.iterator),
                    Ty::Trait(lower_nominal_type(&iteration.next_interface)),
                    lower_type(&iteration.item),
                ],
                budget,
            )?;
        }
        if let Some(propagation) = table.propagation(id) {
            self.call(module, span, &propagation.branch, false, budget)?;
            self.call(module, span, &propagation.from_residual, false, budget)?;
            self.fact(
                span,
                FactKind::Member,
                vec![propagation.break_variant, propagation.continue_variant],
                vec![lower_type(&propagation.return_type)],
                budget,
            )?;
        }
        if let Some((receiver, interface)) = table.callable_coercion(id) {
            self.fact(
                span,
                FactKind::Protocol,
                vec![],
                vec![
                    lower_type(receiver),
                    Ty::Trait(lower_nominal_type(interface)),
                ],
                budget,
            )?;
        }
        if let Some(coercion) = table.interface_coercion(id) {
            let mut types = vec![
                lower_type(&coercion.concrete_type),
                Ty::Trait(lower_nominal_type(&coercion.interface_type)),
            ];
            let targets = match &coercion.implementation {
                ResolvedInterfaceImplementation::Declared {
                    declaration,
                    arguments,
                } => {
                    types.extend(arguments.iter().map(lower_type));
                    vec![*declaration]
                }
                ResolvedInterfaceImplementation::Bound { interface } => {
                    types.push(Ty::Trait(lower_nominal_type(interface)));
                    vec![]
                }
                ResolvedInterfaceImplementation::Intrinsic
                | ResolvedInterfaceImplementation::Upcast => vec![],
            };
            let kind = match &coercion.implementation {
                ResolvedInterfaceImplementation::Declared { .. } => FactKind::DeclaredCoercion,
                ResolvedInterfaceImplementation::Bound { .. } => FactKind::BoundCoercion,
                ResolvedInterfaceImplementation::Intrinsic => FactKind::IntrinsicCoercion,
                ResolvedInterfaceImplementation::Upcast => FactKind::InterfaceUpcast,
            };
            self.fact(span, kind, targets, types, budget)?;
        }
        match kind {
            ExprKind::Await { expr } => {
                let types = table
                    .expr_type(*expr)
                    .as_ref()
                    .map(lower_type)
                    .into_iter()
                    .collect();
                self.fact(span, FactKind::Await, vec![], types, budget)?;
            }
            ExprKind::Closure { .. } | ExprKind::AsyncBlock { .. } => {
                let creation = match kind {
                    ExprKind::AsyncBlock { .. } => FactKind::AsyncBlock,
                    ExprKind::Closure { is_async: true, .. } => FactKind::AsyncClosure,
                    _ => FactKind::Closure,
                };
                self.fact(span, creation, vec![], vec![], budget)?;
                for capture in module.names.closure_captures(id) {
                    self.reference(module, span, *capture, budget)?;
                    self.body
                        .facts
                        .last_mut()
                        .ok_or(DefinitionMappingError::InvalidContract)?
                        .kind = FactKind::Capture;
                }
            }
            ExprKind::Missing => return Err(DefinitionMappingError::InvalidContract),
            _ => {}
        }
        Ok(())
    }
}

pub(super) fn collect(
    module: &CheckedAnalysis,
    owners: &HashMap<HirOwner, usize>,
    builders: &mut [BodyBuilder],
    budget: &mut SourceBudget<'_>,
) -> Result<(), DefinitionMappingError> {
    let hir = &module.lowered.module.nodes;
    let table = &module.typed.type_table;
    for (id, expression) in hir.expressions() {
        budget.charge(1, 0)?;
        if let Some(index) = owners.get(&id.owner()) {
            builders[*index].expression(module, id, &expression.kind, budget)?;
        }
    }
    for (id, place) in hir.places() {
        budget.charge(1, 0)?;
        let Some(index) = owners.get(&id.owner()) else {
            continue;
        };
        let body = &mut builders[*index];
        let span = place.origin.diagnostic_span();
        if let Some(name) = module.names.place_resolution(id) {
            body.reference(module, span, name, budget)?;
        }
        if let Some(ty) = table.place_type(id) {
            body.fact(span, FactKind::Type, vec![], vec![lower_type(&ty)], budget)?;
        }
        if let Some(field) = table.place_field(id) {
            body.fact(span, FactKind::Member, vec![*field], vec![], budget)?;
        }
        if let Some(interface) = table.place_index(id) {
            body.fact(
                span,
                FactKind::Protocol,
                vec![],
                vec![Ty::Trait(lower_nominal_type(interface))],
                budget,
            )?;
        }
        if let Some((interface, method)) = table.place_index_write(id) {
            body.fact(
                span,
                FactKind::Protocol,
                vec![*method],
                vec![Ty::Trait(lower_nominal_type(interface))],
                budget,
            )?;
        }
    }
    for (id, node) in hir.types() {
        budget.charge(1, 0)?;
        let Some(body) = owners.get(&id.owner()) else {
            continue;
        };
        let span = node.origin.diagnostic_span();
        let initializer = builders[*body].span;
        if span.start < initializer.start || span.end > initializer.end {
            continue;
        }
        if let Some(fact) = table.type_ref(id) {
            let target = match &fact.target {
                Some(ResolvedTypeTarget::Source(id) | ResolvedTypeTarget::AssociatedType(id)) => {
                    Some(*id)
                }
                Some(ResolvedTypeTarget::Struct(id)) => {
                    Some(definition(module, ResolvedName::Struct(*id))?)
                }
                Some(ResolvedTypeTarget::Enum(id)) => {
                    Some(definition(module, ResolvedName::Enum(*id))?)
                }
                Some(ResolvedTypeTarget::Trait(id)) => {
                    Some(definition(module, ResolvedName::Trait(*id))?)
                }
                Some(ResolvedTypeTarget::NativeType(id)) => {
                    Some(definition(module, ResolvedName::NativeType(*id))?)
                }
                _ => None,
            };
            builders[*body].fact(
                span,
                FactKind::Type,
                target.into_iter().collect(),
                vec![lower_type(&fact.ty)],
                budget,
            )?;
        }
        if let Some(interface) = table.constraint(id) {
            builders[*body].fact(
                span,
                FactKind::Protocol,
                vec![],
                vec![Ty::Trait(lower_nominal_type(&interface))],
                budget,
            )?;
        }
    }
    for (id, node) in hir.patterns() {
        budget.charge(1, 0)?;
        let Some(body) = owners.get(&id.owner()) else {
            continue;
        };
        let targets = table
            .pattern_fields(id)
            .unwrap_or_default()
            .iter()
            .copied()
            .chain(table.pattern_variant(id).copied())
            .collect();
        builders[*body].fact(
            node.origin.diagnostic_span(),
            FactKind::Member,
            targets,
            vec![],
            budget,
        )?;
    }
    Ok(())
}
