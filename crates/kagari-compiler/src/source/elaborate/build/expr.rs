//! Value elaboration preserves structural evaluation order and explicit conversions.

use crate::source::elaborate::{
    ElaborationError,
    build::Builder,
    model::{
        CheckedCall, CheckedCallTarget, CheckedExprKind, CheckedMatchArm, Conversion, IndexRead,
        MethodOperands, OperationId, OperatorSyntax,
    },
};
use kagari_common::span::Span;
use kagari_contract::numeric::NumericConversion;
use kagari_hir::{
    hir::{
        expr::{ExprKind, ops::BinaryOp},
        ids::ExprId,
    },
    language::semantics::callable_signature,
    resolver::resolved::ResolvedName,
    typeck::table::{CallTarget, ResolvedCall},
    types::TypeId,
};
use kagari_types::{language::Protocol, scalar::BuiltinType};

impl Builder<'_, '_> {
    pub(super) fn expression(&mut self, id: ExprId) -> Result<OperationId, ElaborationError> {
        self.check()?;
        if let Some(operation) = self.body.results.get(&id) {
            return Ok(*operation);
        }
        let span = self.require(self.input.expression_span(id), None, "foreign expression")?;
        let kind = self.expression_kind(id, span)?;
        let ty = if matches!(kind, CheckedExprKind::Diverging(_)) {
            TypeId::Builtin(BuiltinType::Never)
        } else {
            self.value_type(id)?
        };
        let mut operation = self.append(id, span, ty.clone(), kind);
        self.body.values.insert(id, operation);
        let facts = self.require(
            self.input.expression_facts(id),
            Some(span),
            "missing expression facts",
        )?;
        if self.completes(id)? {
            if let Some((receiver, interface)) = facts.callable_conversion {
                let result = self.require(
                    callable_signature(self.environment.definitions(), interface),
                    Some(span),
                    "missing callable conversion signature",
                )?;
                let result = self.valid_type(result, Some(span))?;
                operation = self.append(
                    id,
                    span,
                    result,
                    CheckedExprKind::Convert {
                        value: operation,
                        conversion: Conversion::Callable {
                            receiver: receiver.clone(),
                            interface: interface.clone(),
                        },
                    },
                );
            }
            if let Some(conversion) = facts.interface_conversion {
                operation = self.append(
                    id,
                    span,
                    TypeId::Trait(conversion.interface_type.clone()),
                    CheckedExprKind::Convert {
                        value: operation,
                        conversion: Conversion::Interface(conversion.clone()),
                    },
                );
            }
            let _result = self.valid_type(
                self.require(
                    facts.result_type,
                    Some(span),
                    "missing expression result type",
                )?,
                Some(span),
            )?;
        }
        self.body.results.insert(id, operation);
        Ok(operation)
    }

    fn expression_kind(
        &mut self,
        id: ExprId,
        span: Span,
    ) -> Result<CheckedExprKind, ElaborationError> {
        let syntax = self
            .require(self.input.expression(id), Some(span), "foreign expression")?
            .kind
            .clone();
        let facts = self.require(
            self.input.expression_facts(id),
            Some(span),
            "missing expression facts",
        )?;
        if let Some(prefix) = self.diverging_prefix(id, &syntax)? {
            return Ok(CheckedExprKind::Diverging(prefix));
        }
        if matches!(syntax, ExprKind::Name { .. })
            && let Some(call) = &facts.call
            && let CallTarget::Function(function) = call.target
        {
            let declaration = self.require(
                self.environment
                    .definition(ResolvedName::Function(function)),
                Some(span),
                "missing local function declaration",
            )?;
            return Ok(CheckedExprKind::FunctionValue {
                declaration,
                arguments: call.type_arguments.clone(),
            });
        }
        if let Some(constant) = facts.associated_constant {
            return Ok(CheckedExprKind::AssociatedConstant(constant.clone()));
        }
        if let Some(enumeration) = facts.enumeration {
            let variant = *self.require(
                enumeration.variant.as_ref(),
                Some(span),
                "missing enum variant",
            )?;
            let contract = self.require(
                self.environment.aggregates().variant(&variant),
                Some(span),
                "missing variant contract",
            )?;
            let owner = self.require(
                self.environment
                    .aggregates()
                    .enumeration(&enumeration.enumeration),
                Some(span),
                "missing enum contract",
            )?;
            let fields = match syntax {
                ExprKind::Name { .. } => Vec::new(),
                ExprKind::Call { args, .. } => args.to_vec(),
                _ => {
                    return Err(
                        self.error(Some(span), "constructor fact on non-constructor expression")
                    );
                }
            };
            if contract.owner != owner.id || fields.len() != contract.payload.len() {
                return Err(self.error(Some(span), "enum constructor contract mismatch"));
            }
            self.expressions(&fields)?;
            return Ok(CheckedExprKind::Enum {
                declaration: enumeration.enumeration,
                variant,
                fields,
            });
        }
        if let Some(scalar) = facts.scalar {
            return Ok(CheckedExprKind::Constant(scalar.clone()));
        }
        if let Some(host) = facts.host_path {
            if host.root == id {
                return Err(self.error(Some(span), "recursive host read root"));
            }
            self.expression(host.root)?;
            for (_, expr) in &host.dynamic_arguments {
                self.expression(*expr)?;
            }
            return Ok(CheckedExprKind::HostRead(host.clone()));
        }
        Ok(match syntax {
            ExprKind::Missing | ExprKind::Literal(_) => {
                return Err(self.error(Some(span), "missing checked value"));
            }
            ExprKind::Name { .. } => {
                let binding = self.require(facts.binding, Some(span), "missing name binding")?;
                if let Some(value) = self.environment.constant_value(&binding) {
                    CheckedExprKind::Constant(value.clone())
                } else {
                    CheckedExprKind::Read(self.value_binding(&binding, span)?)
                }
            }
            ExprKind::Call { args, .. } => {
                let call = self.require(facts.call, Some(span), "missing call selection")?;
                match call.target {
                    CallTarget::TerminatingCallee => {
                        return Err(self.error(Some(span), "terminating callee completes normally"));
                    }
                    CallTarget::RuntimeHelper(helper) => {
                        let operands = call
                            .receiver
                            .into_iter()
                            .chain(args.iter().copied())
                            .collect::<Vec<_>>();
                        self.expressions(&operands)?;
                        CheckedExprKind::Helper { helper, operands }
                    }
                    _ => CheckedExprKind::Call(self.call(id, call, &args)?),
                }
            }
            ExprKind::Cast { expr, .. } => {
                self.expression(expr)?;
                let (TypeId::Builtin(source), TypeId::Builtin(target)) =
                    (self.value_type(expr)?, self.value_type(id)?)
                else {
                    return Err(self.error(Some(span), "non-numeric cast types"));
                };
                let conversion = NumericConversion { source, target };
                self.require(
                    conversion.contract(),
                    Some(span),
                    "missing numeric conversion contract",
                )?;
                CheckedExprKind::NumericCast {
                    value: expr,
                    conversion,
                }
            }
            ExprKind::Prefix { op, expr } => {
                self.expression(expr)?;
                if let Some(call) = facts.call {
                    CheckedExprKind::Operator {
                        call: self.operator_call(id, call, &[expr])?,
                        syntax: OperatorSyntax::Prefix,
                    }
                } else {
                    CheckedExprKind::Unary { operand: expr, op }
                }
            }
            ExprKind::Binary { lhs, op, rhs } => {
                self.expressions(&[lhs, rhs])?;
                if matches!(op, BinaryOp::AndAnd | BinaryOp::OrOr) {
                    CheckedExprKind::Logical { lhs, op, rhs }
                } else if matches!(op, BinaryOp::Eq | BinaryOp::NotEq) {
                    CheckedExprKind::Equality {
                        lhs,
                        rhs,
                        negate: op == BinaryOp::NotEq,
                        receiver: self.value_type(lhs)?,
                    }
                } else if let Some(call) = facts.call {
                    CheckedExprKind::Operator {
                        call: self.operator_call(id, call, &[lhs, rhs])?,
                        syntax: OperatorSyntax::Binary(op),
                    }
                } else {
                    CheckedExprKind::PrimitiveBinary { lhs, op, rhs }
                }
            }
            ExprKind::Field { receiver, .. } => {
                self.expression(receiver)?;
                CheckedExprKind::Field {
                    receiver,
                    field: *self.require(facts.field, Some(span), "missing selected field")?,
                }
            }
            ExprKind::Index { receiver, index } => {
                self.expressions(&[receiver, index])?;
                let operation = if let Some(call) = facts.call {
                    IndexRead::Method(Box::new(self.operator_call(
                        id,
                        call,
                        &[receiver, index],
                    )?))
                } else {
                    IndexRead::Aggregate
                };
                CheckedExprKind::Index {
                    receiver,
                    index,
                    operation,
                }
            }
            ExprKind::StructInit { fields, base, .. } => {
                let selection = self.require(
                    facts.structure,
                    Some(span),
                    "missing struct constructor selection",
                )?;
                if fields.len() != selection.fields.len() {
                    return Err(self.error(Some(span), "constructor field count mismatch"));
                }
                let mut selected = Vec::with_capacity(fields.len());
                for (field, declaration) in fields.into_iter().zip(&selection.fields) {
                    let declaration =
                        self.require(*declaration, Some(span), "missing initializer field")?;
                    self.expression(field.value)?;
                    selected.push((declaration, field.value));
                }
                if let Some(base) = base {
                    self.expression(base)?;
                }
                CheckedExprKind::Struct {
                    declaration: selection.structure,
                    fields: selected,
                    base,
                    base_fields: selection.base_fields.clone(),
                }
            }
            ExprKind::Tuple(elements) => {
                self.expressions(&elements)?;
                CheckedExprKind::Tuple(elements.to_vec())
            }
            ExprKind::Array { elements, .. } => {
                self.expressions(&elements)?;
                CheckedExprKind::Array(elements.to_vec())
            }
            ExprKind::ArrayRepeat { value, count } => {
                self.expressions(&[value, count])?;
                CheckedExprKind::ArrayRepeat { value, count }
            }
            ExprKind::Range {
                start,
                end,
                inclusive,
            } => {
                if let Some(start) = start {
                    self.expression(start)?;
                }
                if let Some(end) = end {
                    self.expression(end)?;
                }
                CheckedExprKind::Range {
                    start,
                    end,
                    inclusive,
                }
            }
            ExprKind::InterpolatedString(parts) => {
                self.expressions(&parts)?;
                CheckedExprKind::InterpolatedString(parts.to_vec())
            }
            ExprKind::FormatPart { expr, .. } => {
                self.expression(expr)?;
                if let Some(call) = facts.call {
                    CheckedExprKind::Format {
                        call: self.operator_call(id, call, &[expr])?,
                    }
                } else {
                    CheckedExprKind::Identity(expr)
                }
            }
            ExprKind::Await { expr } => {
                self.expression(expr)?;
                CheckedExprKind::Await { future: expr }
            }
            ExprKind::Propagate { expr } => {
                self.expression(expr)?;
                CheckedExprKind::Propagate {
                    value: expr,
                    protocol: Box::new(
                        self.require(
                            facts.propagation,
                            Some(span),
                            "missing propagation contract",
                        )?
                        .clone(),
                    ),
                }
            }
            ExprKind::Block(block) => {
                self.block(block)?;
                CheckedExprKind::Block(block)
            }
            ExprKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.condition(&condition)?;
                self.block(then_branch)?;
                if let Some(branch) = else_branch {
                    self.expression(branch)?;
                }
                CheckedExprKind::If {
                    condition,
                    then_branch,
                    else_branch,
                }
            }
            ExprKind::Match { scrutinee, arms } => {
                self.expression(scrutinee)?;
                let mut checked = Vec::new();
                for (index, arm) in arms.iter().enumerate() {
                    self.pattern(arm.pattern)?;
                    let guard_completes = if let Some(guard) = arm.guard {
                        self.expression(guard)?;
                        self.completes(guard)?
                    } else {
                        true
                    };
                    let value = if guard_completes {
                        self.expression(arm.expr)?;
                        Some(arm.expr)
                    } else {
                        None
                    };
                    checked.push(CheckedMatchArm {
                        pattern: arm.pattern,
                        guard: arm.guard,
                        value,
                    });
                    if !self.match_arm_can_continue(id, index)? {
                        break;
                    }
                }
                CheckedExprKind::Match {
                    scrutinee,
                    arms: checked,
                }
            }
            ExprKind::Loop { body } => {
                self.block(body)?;
                CheckedExprKind::Loop(body)
            }
            ExprKind::Closure { is_async, .. } => {
                let input =
                    self.require(self.input.closure(id), Some(span), "missing closure input")?;
                let captures = self.captures(id)?;
                CheckedExprKind::Closure {
                    key: input.key(),
                    is_async,
                    captures,
                }
            }
            ExprKind::AsyncBlock { .. } => {
                let input =
                    self.require(self.input.closure(id), Some(span), "missing async input")?;
                let captures = self.captures(id)?;
                CheckedExprKind::AsyncBlock {
                    key: input.key(),
                    captures,
                }
            }
        })
    }

    fn expressions(&mut self, values: &[ExprId]) -> Result<(), ElaborationError> {
        for value in values {
            self.expression(*value)?;
        }
        Ok(())
    }

    fn operator_call(
        &mut self,
        id: ExprId,
        mut call: ResolvedCall,
        operands: &[ExprId],
    ) -> Result<CheckedCall, ElaborationError> {
        // Protocol facts may record a semantic receiver independently of a syntax receiver.
        // The operator's operand list is authoritative for once-only evaluation.
        call.receiver = operands.first().copied();
        self.call(id, call, &operands[1..])
    }

    fn call(
        &mut self,
        id: ExprId,
        call: ResolvedCall,
        args: &[ExprId],
    ) -> Result<CheckedCall, ElaborationError> {
        let span = self.input.expression_span(id);
        let signature = self.require(call.signature, span, "missing applied call signature")?;
        for ty in &signature.params {
            self.valid_type(ty.clone(), span)?;
        }
        self.valid_type(signature.return_type.clone(), span)?;
        let mut operands = Vec::new();
        let target = match call.target {
            CallTarget::SourceFunction(declaration) => CheckedCallTarget::Direct {
                declaration,
                arguments: call.type_arguments,
            },
            CallTarget::Function(function) => CheckedCallTarget::Direct {
                declaration: self.require(
                    self.environment
                        .definition(ResolvedName::Function(function)),
                    span,
                    "missing local callable declaration",
                )?,
                arguments: call.type_arguments,
            },
            CallTarget::TraitMethod { method, interface } => {
                let facts = self.require(self.input.expression_facts(id), span, "foreign call")?;
                let receiver = if let Some(receiver) = facts.protocol_receiver {
                    receiver.clone()
                } else {
                    self.value_type(self.require(
                        call.receiver,
                        span,
                        "missing method receiver",
                    )?)?
                };
                let operands = if Protocol::from_definition(
                    &interface.declaration,
                    self.environment.definitions(),
                ) == Some(Protocol::Fn)
                    && matches!(self.input.expression(id).map(|expr| &expr.kind), Some(ExprKind::Call { callee, .. }) if Some(*callee) == call.receiver)
                {
                    MethodOperands::Callable
                } else {
                    MethodOperands::Ordinary
                };
                CheckedCallTarget::Method {
                    operands,
                    method,
                    interface,
                    receiver,
                    arguments: call.type_arguments,
                }
            }
            CallTarget::Value => {
                let callee = self.require(call.receiver, span, "missing callable value")?;
                self.expression(callee)?;
                CheckedCallTarget::Value { callee }
            }
            CallTarget::TerminatingCallee | CallTarget::RuntimeHelper(_) => {
                return Err(self.error(span, "non-callable contract in ordinary call"));
            }
        };
        if !matches!(target, CheckedCallTarget::Value { .. })
            && let Some(receiver) = call.receiver
        {
            operands.push(receiver);
        }
        operands.extend_from_slice(args);
        let parameter_count = if let CheckedCallTarget::Method {
            operands: MethodOperands::Callable,
            interface,
            ..
        } = &target
        {
            let Some(TypeId::Function { params, .. }) =
                callable_signature(self.environment.definitions(), interface)
            else {
                return Err(self.error(span, "missing callable operand signature"));
            };
            // Authored f(a, b) retains flat operands; the selected Fn method takes
            // the receiver and one tuple. Packing belongs to MIR emission.
            params.len() + 1
        } else {
            signature.params.len()
        };
        if operands.len() != parameter_count {
            return Err(self.error(span, "applied call operand count mismatch"));
        }
        self.expressions(&operands)?;
        Ok(CheckedCall {
            target,
            signature,
            operands,
        })
    }
}
