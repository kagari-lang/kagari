//! Select a method recipe after substituting the caller's checked arguments.
//!
//! Recipes contain no MIR values. Emission owns operand evaluation and control flow;
//! this planner owns implementation search, native/default selection and shared evidence.

use crate::source::{
    lower::{MirLoweringError, instances::InstancePlanner},
    types,
};
use kagari_common::{identity::table::DefinitionId, span::Span};
use kagari_contract::{
    callable::interface::InterfaceCallContract,
    numeric::{NumericConversion, NumericOperation},
    operations::{BinaryOp, UnaryOp},
    standard::RuntimePrimitive,
};
use kagari_hir::{
    aggregates::traits::MethodDefault,
    callable::AppliedCallSignature,
    hir::expr::ops::BinaryOp as HirBinaryOp,
    language::semantics as traits,
    native::NativeBinding,
    typeck::GenericBounds,
    types::{NominalType, TypeId, TypeSubstitution},
};
use kagari_mir::instruction::CallTarget;
use kagari_types::{
    conversion, declaration::conversion::ConversionAdapter, integer::IntegerOp, language::Protocol,
    scalar::BuiltinType,
};

use std::slice;

pub(crate) struct MethodApplication<'a> {
    pub interface: &'a NominalType,
    pub receiver: &'a TypeId,
    pub method: &'a DefinitionId,
    pub arguments: &'a [TypeId],
}

pub(crate) struct MethodPlan {
    pub result: TypeId,
    pub operation: MethodOperation,
}

pub(crate) enum MethodOperation {
    Call(CallTarget),
    /// A function value receives the unpacked tuple elements.
    Callable {
        params: Vec<TypeId>,
    },
    Identity,
    Protocol {
        protocol: Protocol,
        receiver: TypeId,
    },
    Convert(NumericConversion),
    Numeric(NumericOperation),
    NegateSmallInteger(NumericOperation),
    Binary(BinaryOp),
    Unary(UnaryOp),
    Index,
}

impl InstancePlanner<'_> {
    pub(crate) fn method_plan(
        &mut self,
        application: MethodApplication<'_>,
        substitution: &TypeSubstitution,
        bounds: &GenericBounds,
        span: Span,
    ) -> Result<MethodPlan, MirLoweringError> {
        let MethodApplication {
            interface,
            receiver,
            method,
            arguments: method_arguments,
        } = application;
        let arguments = self.arguments(method_arguments, substitution, span)?;
        let receiver = self
            .arguments(slice::from_ref(receiver), substitution, span)?
            .remove(0);
        let TypeId::Trait(interface) = self
            .arguments(&[TypeId::Trait(interface.clone())], substitution, span)?
            .remove(0)
        else {
            unreachable!()
        };
        self.applied_method_plan(interface, receiver, method, &arguments, bounds, span)
    }

    fn applied_method_plan(
        &mut self,
        mut interface: NominalType,
        receiver: TypeId,
        method: &DefinitionId,
        arguments: &[TypeId],
        bounds: &GenericBounds,
        span: Span,
    ) -> Result<MethodPlan, MirLoweringError> {
        self.check()?;
        if let Some((required, target)) =
            traits::conversion_requirement(&interface, &receiver, Some(self.catalog))
        {
            let method = self
                .catalog
                .trait_(&interface.declaration)
                .and_then(|contract| contract.conversion_adapter.as_ref())
                .and_then(|adapter| match adapter {
                    ConversionAdapter::Reverse { method, .. } => Some(*method),
                    _ => None,
                })
                .ok_or(MirLoweringError::MissingBinding(
                    "registered forward conversion member",
                ))?;
            return self.applied_method_plan(required, target, &method, arguments, bounds, span);
        }
        let protocol = Protocol::from_definition(&interface.declaration, &self.definitions());
        if protocol == Some(Protocol::Fn)
            && let TypeId::Function { params, result } = &receiver
        {
            return Ok(MethodPlan {
                result: *result.clone(),
                operation: MethodOperation::Callable {
                    params: params.clone(),
                },
            });
        }
        let parents = if let TypeId::Trait(child) = &receiver {
            self.catalog.interface_closure(
                &self.definitions(),
                child,
                &receiver,
                &self.options.cancel,
            )?
        } else {
            Vec::new()
        };
        if let Some(parent) = parents.iter().find(|parent| parent.satisfies(&interface)) {
            interface = parent.clone();
        }
        let signature = self
            .catalog
            .trait_method(method)
            .ok_or(MirLoweringError::MissingBinding("operator signature"))?
            .clone();
        let native_default = match &signature.default {
            Some(MethodDefault::Native(binding)) => Some(binding.clone()),
            _ => None,
        };
        let contract = self
            .catalog
            .trait_(&interface.declaration)
            .ok_or(MirLoweringError::MissingBinding("operator trait"))?;
        let local_parameters = &signature.generic_params[contract.generic_params.len()..];
        if local_parameters.len() != arguments.len() {
            return Err(MirLoweringError::MissingBinding("method arguments"));
        }
        let mut substitution: TypeSubstitution = contract
            .generic_params
            .iter()
            .cloned()
            .zip(interface.arguments.iter().cloned())
            .chain(
                local_parameters
                    .iter()
                    .cloned()
                    .zip(arguments.iter().cloned()),
            )
            .collect();
        substitution.insert_receiver(contract.id, receiver.clone());
        let instantiate = |ty: &TypeId| {
            self.catalog.normalize_type(
                &self.definitions(),
                &ty.with_self(&contract.id, &receiver)
                    .instantiate(&substitution)
                    .with_associated_types(&interface, &receiver),
            )
        };
        let result = instantiate(&signature.return_type);
        let params = signature
            .params
            .iter()
            .map(|param| instantiate(&param.ty))
            .collect::<Vec<_>>();
        if matches!(receiver, TypeId::Generic(_))
            || ((native_default.is_none()
                || matches!(native_default, Some(NativeBinding::Default(_))))
                && parents.contains(&interface))
        {
            let evidence = self.method_evidence(method, &interface, &receiver, arguments, span)?;
            let normalizations =
                self.method_normalizations(method, &interface, &receiver, arguments)?;
            return Ok(MethodPlan {
                result,
                operation: MethodOperation::Call(CallTarget::InterfaceMethod(Box::new(
                    InterfaceCallContract {
                        constructions: evidence.constructions,
                        normalizations,
                        receiver: matches!(receiver, TypeId::Generic(_))
                            .then(|| self.encode_type(&receiver)),
                        operations: evidence.operations,
                        arguments: arguments.iter().map(|ty| self.encode_type(ty)).collect(),
                        interface: self.encode_nominal(&interface),
                        method_slot: signature.slot as u32,
                    },
                ))),
            });
        }
        if let TypeId::Builtin(input) = &receiver {
            let op = match protocol {
                Some(Protocol::BitAnd) => Some(IntegerOp::BitAnd),
                Some(Protocol::BitOr) => Some(IntegerOp::BitOr),
                Some(Protocol::BitXor) => Some(IntegerOp::BitXor),
                Some(Protocol::Shl) => Some(IntegerOp::Shl),
                Some(Protocol::Shr) => Some(IntegerOp::Shr),
                Some(Protocol::Not) if input.integer_layout().is_some() => Some(IntegerOp::BitNot),
                _ => None,
            };
            if let Some(op) = op {
                let rhs = match interface.arguments.first() {
                    Some(TypeId::Builtin(rhs)) => Some(*rhs),
                    None => None,
                    _ => return Err(MirLoweringError::MissingBinding("numeric rhs type")),
                };
                let operation = NumericOperation {
                    op,
                    input: *input,
                    rhs,
                };
                operation
                    .contract()
                    .ok_or(MirLoweringError::MissingBinding("numeric contract"))?;
                return Ok(MethodPlan {
                    result,
                    operation: MethodOperation::Numeric(operation),
                });
            }
        }
        let implementation = self.implementation_method(method, &interface, &receiver, bounds)?;
        if !matches!(native_default, Some(NativeBinding::Default(_)))
            && let Some(protocol) = protocol
            && protocol.equality_protocol()
            && !matches!(receiver, TypeId::Trait(_) | TypeId::Generic(_))
            && implementation.is_none()
        {
            return Ok(MethodPlan {
                result,
                operation: MethodOperation::Protocol { protocol, receiver },
            });
        }
        if native_default.is_some() && implementation.is_none() {
            return self.native_default_plan(&receiver, &interface, method, arguments, span);
        }
        if protocol == Some(Protocol::From) {
            if interface.arguments.as_slice() == [receiver.clone()] {
                return Ok(MethodPlan {
                    result,
                    operation: MethodOperation::Identity,
                });
            }
            if let (TypeId::Builtin(target), [TypeId::Builtin(source)]) =
                (&receiver, interface.arguments.as_slice())
                && conversion::lossless_from(*source, *target)
            {
                let conversion = NumericConversion {
                    source: *source,
                    target: *target,
                };
                conversion
                    .contract()
                    .ok_or(MirLoweringError::MissingBinding(
                        "numeric conversion contract",
                    ))?;
                return Ok(MethodPlan {
                    result,
                    operation: MethodOperation::Convert(conversion),
                });
            }
        }
        if let Some((declaration, mut implementation_arguments)) = implementation {
            implementation_arguments.extend_from_slice(arguments);
            if self.native_function(&declaration).is_some() {
                return self.native_implementation_plan(
                    &declaration,
                    &implementation_arguments,
                    &receiver,
                    &result,
                    span,
                );
            }
            let target = self.direct_call(
                declaration,
                &implementation_arguments,
                &AppliedCallSignature {
                    params,
                    return_type: result.clone(),
                },
                &Default::default(),
                span,
            )?;
            return Ok(MethodPlan {
                result,
                operation: MethodOperation::Call(target),
            });
        }
        if protocol == Some(Protocol::Iterable)
            && result == receiver
            && traits::iterator_requirement(&self.definitions(), &interface, &receiver).is_some_and(
                |required| {
                    traits::intrinsic_applies(
                        &self.definitions(),
                        &required,
                        &receiver,
                        Some(self.catalog),
                        &Default::default(),
                    )
                },
            )
        {
            return Ok(MethodPlan {
                result,
                operation: MethodOperation::Identity,
            });
        }
        let operation = match protocol {
            Some(protocol) if protocol.binary_operator() => {
                let (op, source_op) = match protocol {
                    Protocol::Add => (BinaryOp::Add, HirBinaryOp::Add),
                    Protocol::Sub => (BinaryOp::Sub, HirBinaryOp::Sub),
                    Protocol::Mul => (BinaryOp::Mul, HirBinaryOp::Mul),
                    Protocol::Div => (BinaryOp::Div, HirBinaryOp::Div),
                    Protocol::Rem => (BinaryOp::Rem, HirBinaryOp::Rem),
                    _ => unreachable!(),
                };
                if let TypeId::Builtin(
                    input @ (BuiltinType::I8
                    | BuiltinType::I16
                    | BuiltinType::U8
                    | BuiltinType::U16
                    | BuiltinType::U32),
                ) = receiver
                {
                    MethodOperation::Numeric(
                        types::lower_numeric_operation(source_op, input, input)
                            .expect("checked integer arithmetic"),
                    )
                } else {
                    MethodOperation::Binary(op)
                }
            }
            Some(protocol @ (Protocol::Neg | Protocol::Not)) => {
                if protocol == Protocol::Neg
                    && let TypeId::Builtin(input @ (BuiltinType::I8 | BuiltinType::I16)) = receiver
                {
                    MethodOperation::NegateSmallInteger(NumericOperation {
                        op: IntegerOp::CheckedSub,
                        input,
                        rhs: Some(input),
                    })
                } else {
                    MethodOperation::Unary(if protocol == Protocol::Neg {
                        UnaryOp::Neg
                    } else {
                        UnaryOp::Not
                    })
                }
            }
            Some(Protocol::Index) => MethodOperation::Index,
            Some(Protocol::PartialOrd) => MethodOperation::Call(CallTarget::RuntimePrimitive(
                RuntimePrimitive::ValuePartialCmp,
            )),
            Some(Protocol::Ord) => {
                MethodOperation::Call(CallTarget::RuntimePrimitive(RuntimePrimitive::ValueCmp))
            }
            _ => return Err(MirLoweringError::MissingBinding("builtin operator")),
        };
        Ok(MethodPlan { result, operation })
    }
}
