use crate::{
    builtin::traits::{self, StandardTraitSemantics, conversion_requirement},
    hir::{ExprId, ExprKind, TypeKind},
    native::NativeTypeKind,
    typeck::{
        BodyTypeEnv, CallTarget,
        body::BodyChecker,
        ty::{self, TypeContext, resolve_type_in},
    },
    types::{NominalType, TypeId},
};
use kagari_abi::{
    scalar::BuiltinType,
    standard::{
        surface::StandardEnum,
        traits::{self as standard_traits, StandardTrait},
    },
};
use kagari_common::{Diagnostic, DiagnosticKind, identity};

impl BodyChecker<'_> {
    pub(super) fn infer_conversion_call(
        &mut self,
        site: ExprId,
        callee: ExprId,
        args: &[ExprId],
        env: &mut BodyTypeEnv,
        expected: Option<&TypeId>,
    ) -> Option<TypeId> {
        let (protocol, source_expr, target, qualified) =
            match &self.lowered.module.expr(callee).kind {
                ExprKind::Field { receiver, name }
                    if matches!(name.as_str(), "into" | "try_into") =>
                {
                    let protocol = if name == "into" {
                        StandardTrait::Into
                    } else {
                        StandardTrait::TryInto
                    };
                    let target = if protocol.fallible_conversion() {
                        match expected {
                            Some(TypeId::StandardEnum {
                                kind: StandardEnum::Result,
                                args,
                            }) => args.first().cloned(),
                            _ => None,
                        }
                    } else {
                        expected.cloned()
                    };
                    (protocol, Some(*receiver), target, None)
                }
                ExprKind::Name {
                    name,
                    explicit_type,
                } => {
                    let (owner, member) = if explicit_type.is_some_and(|id| {
                        matches!(
                            self.lowered.module.type_ref(id).kind,
                            TypeKind::Projection { .. }
                        )
                    }) {
                        ("", name.as_str())
                    } else {
                        name.rsplit_once("::")?
                    };
                    let protocol = match member {
                        "from_str" => StandardTrait::FromStr,
                        "from" => StandardTrait::From,
                        "try_from" => StandardTrait::TryFrom,
                        "from_iter" => StandardTrait::FromIterator,
                        "sum" => StandardTrait::Sum,
                        "product" => StandardTrait::Product,
                        _ => return None,
                    };
                    let context = TypeContext {
                        declarations: self.declarations,
                        generics: &env.generics,
                        self_type: None,
                        implementation: None,
                    };
                    let mut native_constructor = None;
                    let (mut target, qualified) = if let Some(id) = explicit_type {
                        if let TypeKind::Projection {
                            receiver,
                            trait_ref,
                            ..
                        } = &self.lowered.module.type_ref(*id).kind
                        {
                            let target = resolve_type_in(
                                &self.lowered.module,
                                *receiver,
                                context,
                                self.type_table,
                                self.cancel,
                            );
                            let interface = resolve_type_in(
                                &self.lowered.module,
                                *trait_ref,
                                context,
                                self.type_table,
                                self.cancel,
                            );
                            (target, Some(interface))
                        } else {
                            (
                                resolve_type_in(
                                    &self.lowered.module,
                                    *id,
                                    context,
                                    self.type_table,
                                    self.cancel,
                                ),
                                None,
                            )
                        }
                    } else if owner == "Self" {
                        (env.self_type.clone().unwrap_or(TypeId::Error), None)
                    } else {
                        let reference = ty::resolve_named_type(owner, context);
                        native_constructor = ty::native_type(reference.target, self.declarations);
                        (reference.ty, None)
                    };
                    if protocol == StandardTrait::FromIterator
                        && explicit_type.is_none()
                        && let Some(
                            constructor @ (NativeTypeKind::ArrayList
                            | NativeTypeKind::LinkedHashSet
                            | NativeTypeKind::LinkedHashMap),
                        ) = native_constructor
                        && let Some(source) = args.first()
                    {
                        let input = self.infer_expr_type(*source, env);
                        if let Some(item) = self.infer_iteration(site, &input, env) {
                            let arguments = if constructor == NativeTypeKind::LinkedHashMap {
                                match item {
                                    TypeId::Tuple(items) if items.len() == 2 => items,
                                    _ => vec![],
                                }
                            } else {
                                vec![item]
                            };
                            target = constructor.apply(&arguments).unwrap_or(TypeId::Error);
                        }
                    }
                    if target.is_unresolved() {
                        return None;
                    }
                    (protocol, None, Some(target), qualified)
                }
                _ => return None,
            };
        let protocol_id = standard_traits::identity(protocol);
        let contract = self.aggregates.trait_(&protocol_id)?;
        let [method] = contract.methods.as_slice() else {
            return None;
        };
        let method_id = method.id.clone();
        let method_name = method.name.clone();
        let arity = method
            .params
            .len()
            .saturating_sub(usize::from(source_expr.is_some()));
        if args.len() != arity {
            self.infer_call_args(args, env);
            self.diagnostics.push(
                Diagnostic::error(DiagnosticKind::CallArityMismatch {
                    function_name: protocol.name().into(),
                    expected: arity,
                    found: args.len(),
                })
                .with_span(self.lowered.source_map.expr_span(site)),
            );
            return Some(TypeId::Error);
        }
        let input = self.infer_expr_type(source_expr.unwrap_or_else(|| args[0]), env);

        if source_expr.is_some()
            && self.trait_bounds_for(&input, env).iter().any(|interface| {
                StandardTrait::from_id(&interface.declaration).is_none()
                    && self
                        .aggregates
                        .trait_(&interface.declaration)
                        .is_some_and(|contract| {
                            contract
                                .methods
                                .iter()
                                .any(|method| method.name == method_name)
                        })
            })
        {
            return None;
        }
        let target = target.filter(|ty| !ty.is_unresolved()).or_else(|| {
            let mut targets = Vec::new();
            for bound in self.trait_bounds_for(&input, env) {
                if bound.declaration == protocol_id
                    && bound.arguments.len() == 1
                    && !targets.contains(&bound.arguments[0])
                {
                    targets.push(bound.arguments[0].clone());
                }
            }
            (targets.len() == 1).then(|| targets.remove(0))
        });
        let Some(target) = target else {
            self.diagnostics.push(
                Diagnostic::error(DiagnosticKind::CannotInferGenericArgument {
                    function_name: protocol.name().into(),
                    parameter: "Target (annotate the result)".into(),
                })
                .with_span(self.lowered.source_map.expr_span(site)),
            );
            return Some(TypeId::Error);
        };
        let receiver = if protocol.reverse_conversion() {
            input.clone()
        } else {
            target.clone()
        };
        let input_argument = if protocol == StandardTrait::FromIterator || protocol.aggregation() {
            match self.infer_iteration(site, &input, env) {
                Some(item) => item,
                None => {
                    self.conversion_error(site, "from_iter requires Iterable");
                    return Some(TypeId::Error);
                }
            }
        } else {
            input.clone()
        };
        let mut interface = protocol.nominal();
        if protocol == StandardTrait::FromStr {
            if input != TypeId::Builtin(BuiltinType::String) {
                self.conversion_error(site, "from_str requires a String");
                return Some(TypeId::Error);
            }
        } else {
            interface.arguments.push(if protocol.reverse_conversion() {
                target.clone()
            } else {
                input_argument
            });
        }
        if qualified.as_ref().is_some_and(|ty|!matches!(ty,TypeId::Trait(n) if n.declaration==interface.declaration && n.arguments==interface.arguments)) {
            self.conversion_error(site,"qualified conversion does not match the source type");
            return Some(TypeId::Error);
        }
        if !self.conversion_holds(&interface, &receiver, env) {
            self.conversion_error(
                site,
                "requires a matching From/TryFrom implementation or bound",
            );
            return Some(TypeId::Error);
        }
        let error = if protocol.fallible_conversion() || protocol == StandardTrait::FromStr {
            let own = identity::associated_type_id(
                &interface.declaration,
                if protocol == StandardTrait::FromStr {
                    "Err"
                } else {
                    "Error"
                },
            );
            let bound_error = self
                .trait_bounds_for(&receiver, env)
                .into_iter()
                .find(|b| {
                    b.declaration == interface.declaration && b.arguments == interface.arguments
                })
                .and_then(|b| b.associated_types.get(&own).cloned());
            let projected = TypeId::Projection {
                receiver: Box::new(receiver.clone()),
                interface: Box::new(interface.clone()),
                member: own,
                arguments: vec![],
            };
            Some(bound_error.unwrap_or_else(|| self.aggregates.normalize_type(&projected)))
        } else {
            None
        };
        self.type_table
            .insert_protocol_receiver(site, receiver.clone());
        self.type_table.insert_call(
            site,
            CallTarget::TraitMethod {
                method: method_id,
                interface,
            },
            source_expr,
        );
        if protocol == StandardTrait::FromIterator || protocol.aggregation() {
            self.type_table.insert_type_arguments(site, vec![input]);
        }
        let result = match error {
            Some(error) => TypeId::StandardEnum {
                kind: StandardEnum::Result,
                args: vec![target, error],
            },
            None => target,
        };
        self.record_protocol_application(site, &receiver, result.clone());
        Some(result)
    }

    fn conversion_error(&mut self, site: ExprId, reason: &str) {
        self.diagnostics.push(
            Diagnostic::error(DiagnosticKind::InvalidCallTarget {
                type_name: reason.into(),
            })
            .with_span(self.lowered.source_map.expr_span(site)),
        );
    }

    pub(super) fn conversion_holds(
        &self,
        interface: &NominalType,
        receiver: &TypeId,
        env: &BodyTypeEnv,
    ) -> bool {
        if self
            .trait_bounds_for(receiver, env)
            .iter()
            .any(|b| b.satisfies(interface))
        {
            return true;
        }
        let (required, target) = conversion_requirement(interface, receiver)
            .unwrap_or((interface.clone(), receiver.clone()));
        self.trait_bounds_for(&target, env)
            .iter()
            .any(|b| b.satisfies(&required))
            || traits::intrinsic_applies(
                &required,
                &target,
                Some(self.aggregates),
                &env.generic_bounds,
            )
            || self
                .aggregates
                .concrete_interface_implementation(
                    &required,
                    &target,
                    &env.generic_bounds,
                    4096,
                    64,
                    self.cancel,
                )
                .is_ok_and(|selected| selected.is_some())
    }
}
